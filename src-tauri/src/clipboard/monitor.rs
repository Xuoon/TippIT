use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::platform::{self, ForegroundApp};
use crate::state::AppState;
use crate::storage::crypto;
use crate::storage::db::{self, EntryRow, TouchSource, KIND_FILES, KIND_IMAGE, KIND_TEXT};
use crate::storage::paths::AppPaths;

use super::read::{read_clipboard, ClipContent};

/// Startet den Clipboard-Monitor: die Plattform-Schicht liefert Änderungs-
/// Signale (Windows: Format-Listener-Events, macOS: changeCount-Polling),
/// der Capture-Worker verarbeitet sie entkoppelt.
pub fn start(app: AppHandle) {
    let (tx, rx) = mpsc::channel::<()>();
    platform::watch_clipboard(tx);
    std::thread::spawn(move || capture_worker(app, rx));
}

fn capture_worker(app: AppHandle, rx: Receiver<()>) {
    while rx.recv().is_ok() {
        // Debounce: manche Apps feuern eine Kopie als mehrere Updates.
        while rx.recv_timeout(Duration::from_millis(150)).is_ok() {}
        if let Err(e) = capture(&app) {
            tracing::warn!("Clipboard-Capture fehlgeschlagen: {e}");
        }
    }
}

fn capture(app: &AppHandle) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    let _rotation = state.rotation_lock.lock().unwrap();

    // Pause stoppt auch die Erfassung — sonst landet z. B. ein bewusst
    // "unbeobachtet" kopiertes Passwort doch in Historie und Sync.
    if state.paused.load(Ordering::SeqCst) {
        return Ok(());
    }

    let seq = platform::clipboard_seq();

    // Eigener Write (Copy aus der Historie / Kopplungscode): genau diese Sequenz
    // überspringen. Hat der Nutzer danach schon wieder kopiert, ist seq neuer
    // und die Kopie wird normal erfasst.
    if seq == state.own_clip_seq.load(Ordering::SeqCst) {
        state.last_clip_seq.store(seq, Ordering::SeqCst);
        return Ok(());
    }

    // Sequence-Number-Dedupe: gleiche Sequenz = schon verarbeitet.
    if state.last_clip_seq.swap(seq, Ordering::SeqCst) == seq {
        return Ok(());
    }

    let (capture_images, capture_files, max_entries) = {
        let s = state.settings.read().unwrap();
        (
            s.history.capture_images,
            s.history.capture_files,
            s.history.max_entries,
        )
    };

    let Some(content) = read_clipboard(capture_images, capture_files) else {
        return Ok(());
    };

    // Best-effort Source-App — Capture scheitert nie daran.
    let app_info = platform::foreground_app_info();
    if app_info.is_none() {
        tracing::debug!("foreground_app_info: None bei Capture");
    } else if let Some(ref a) = app_info {
        if !a.is_self {
            tracing::debug!(app = %a.name, "clipboard source");
        }
    }

    let (kind, plain, thumb) = match content {
        ClipContent::Text(t) => (KIND_TEXT, t.into_bytes(), None),
        ClipContent::Files(files) => (KIND_FILES, serde_json::to_vec(&files)?, None),
        ClipContent::Image { png, thumb_png } => (KIND_IMAGE, png, Some(thumb_png)),
    };
    let hash = crypto::sha256(&plain);

    let db = state.db.lock().unwrap();
    let now_ms = now_ms();

    if let Some(existing) = db::find_by_hash(&db, &hash)? {
        // Duplikat: nach oben; Source: Some→Set, Self→Clear, None→Keep.
        let lamport = db::next_lamport(&db)?;
        let policy = touch_policy(&app_info);
        db::touch(&db, &existing, now_ms, lamport, policy)?;
        match &app_info {
            Some(a) if a.is_self => {
                state
                    .index
                    .write()
                    .unwrap()
                    .touch_with_source(&existing, now_ms, None, None);
            }
            Some(a) => {
                state.index.write().unwrap().touch_with_source(
                    &existing,
                    now_ms,
                    Some(a.id.clone()),
                    Some(a.name.clone()),
                );
                ensure_app_icon_cached(&state.paths, a);
            }
            None => {
                state.index.write().unwrap().touch(&existing, now_ms);
            }
        }
    } else {
        let uuid = uuid::Uuid::now_v7().to_string();
        let keys = state.keys.read().unwrap().clone();
        let cipher = crypto::encrypt(&keys, &uuid, kind, &plain)?;
        let thumb_cipher = thumb
            .map(|t| crypto::encrypt(&keys, &uuid, kind, &t))
            .transpose()?;
        let (source_app_id, source_app_name) = match &app_info {
            Some(a) if !a.is_self => (Some(a.id.clone()), Some(a.name.clone())),
            _ => (None, None),
        };
        let row = EntryRow {
            uuid,
            kind,
            cipher: Some(cipher),
            thumb: thumb_cipher,
            size_bytes: plain.len() as i64,
            hash: hash.to_vec(),
            created_at: now_ms,
            pinned: false,
            deleted: false,
            device_id: state.device_id.clone(),
            lamport: db::next_lamport(&db)?,
            source_app_id,
            source_app_name,
            first_created_at: now_ms,
            copy_count: 1,
        };
        db::insert(&db, &row)?;
        state.index.write().unwrap().upsert(&row, &keys);
        if let Some(a) = &app_info {
            if !a.is_self {
                ensure_app_icon_cached(&state.paths, a);
            }
        }

        for pruned in db::prune(&db, max_entries)? {
            state.index.write().unwrap().remove(&pruned);
        }
    }
    drop(db);

    state.notify_push();
    let _ = app.emit("history-changed", ());
    Ok(())
}

fn touch_policy<'a>(app_info: &'a Option<ForegroundApp>) -> TouchSource<'a> {
    match app_info {
        Some(a) if a.is_self => TouchSource::Clear,
        Some(a) => TouchSource::Set {
            id: &a.id,
            name: &a.name,
        },
        None => TouchSource::Keep,
    }
}

fn ensure_app_icon_cached(paths: &AppPaths, app: &ForegroundApp) {
    let Some(png) = app.icon_png.as_ref() else {
        return;
    };
    let path = paths.app_icon_file(&app.id);
    if path.exists() {
        return;
    }
    if let Err(e) = std::fs::create_dir_all(paths.app_icons_dir()) {
        tracing::debug!("app-icons dir: {e}");
        return;
    }
    if let Err(e) = std::fs::write(&path, png) {
        tracing::debug!("app-icon write: {e}");
    }
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
