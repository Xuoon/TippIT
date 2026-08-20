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

    // Pause stoppt auch die Erfassung — sonst landet z. B. ein bewusst
    // "unbeobachtet" kopiertes Passwort doch in der Historie.
    if state.paused.load(Ordering::SeqCst) {
        return Ok(());
    }

    let seq = platform::clipboard_seq();

    // Eigener Write (Copy aus der Historie, Import): genau diese Sequenz
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

    let (capture_images, capture_files, capture_html, max_entries, excluded) = {
        let s = state.settings.read().unwrap();
        (
            s.history.capture_images,
            s.history.capture_files,
            s.history.capture_html,
            s.history.max_entries,
            s.history.excluded_apps.clone(),
        )
    };

    // Best-effort Source-App — Capture scheitert nie daran.
    let app_info = platform::foreground_app_info();
    if app_info.is_none() {
        tracing::debug!("foreground_app_info: None bei Capture");
    }
    // Ausgeschlossene Quelle: gar nicht erst lesen. Gesucht wird als Teilstring
    // in der Plattform-ID UND im Anzeigenamen, damit in den Einstellungen
    // „KeePass" genügt — der Anzeigename ist unter Windows die Dateibeschreibung
    // („KeePass Password Safe 2"), ein Gleichheitsvergleich ginge dort ins
    // Leere und würde still weiter erfassen. Eine unbekannte
    // Vordergrund-App (None) lässt sich nicht ausschließen — im Zweifel wird
    // erfasst, sonst ließe ein Erkennungsfehler die Historie still leerlaufen.
    if let Some(a) = &app_info {
        let id = a.id.to_lowercase();
        let name = a.name.to_lowercase();
        if excluded.iter().any(|e| {
            let needle = e.trim().to_lowercase();
            !needle.is_empty() && (id.contains(&needle) || name.contains(&needle))
        }) {
            tracing::debug!(app = %a.name, "Quelle ausgeschlossen — nicht erfasst");
            return Ok(());
        }
    }

    let Some(content) = read_clipboard(capture_images, capture_files, capture_html) else {
        return Ok(());
    };

    let (kind, plain, thumb, html) = match content {
        ClipContent::Text { text, html } => (KIND_TEXT, text.into_bytes(), None, html),
        ClipContent::Files(files) => (KIND_FILES, serde_json::to_vec(&files)?, None, None),
        ClipContent::Image { png, thumb_png } => (KIND_IMAGE, png, Some(thumb_png), None),
    };
    let hash = crypto::sha256(&plain);

    let db = state.db.lock().unwrap();
    let now_ms = now_ms();

    if let Some(existing) = db::find_by_hash(&db, &hash)? {
        // Duplikat: nach oben; Source: Some→Set, Self→Clear, None→Keep.
        db::touch(&db, &existing, now_ms, touch_policy(&app_info))?;
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
        let keys = state.keys.clone();
        let cipher = crypto::encrypt(&keys, &uuid, kind, &plain)?;
        let thumb_cipher = thumb
            .map(|t| crypto::encrypt(&keys, &uuid, kind, &t))
            .transpose()?;
        // Eigenes AAD-Byte: der HTML-Blob ist nicht gegen den Klartext-Blob tauschbar.
        let html_cipher = html
            .map(|h| crypto::encrypt(&keys, &uuid, db::AAD_HTML, h.as_bytes()))
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
            html: html_cipher,
            size_bytes: plain.len() as i64,
            hash: hash.to_vec(),
            created_at: now_ms,
            pinned: false,
            trashed_at: 0,
            snippet: false,
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

    let _ = app.emit("history-changed", ());
    Ok(())
}

/// Aufräumen beim Start: abgelaufene Einträge in den Papierkorb, abgelaufenen
/// Papierkorb endgültig leeren. Bewusst nicht bei jeder Kopie — die Fristen
/// bewegen sich in Tagen, ein Lauf pro Programmstart reicht dafür aus.
pub fn run_retention(app: &AppHandle) {
    let state = app.state::<AppState>();
    let retention_days = state.settings.read().unwrap().history.retention_days as i64;
    let now = now_ms();
    let db = state.db.lock().unwrap();

    if retention_days > 0 {
        let cutoff = now - retention_days * 86_400_000;
        match db::trash_older_than(&db, cutoff, now) {
            Ok(uuids) if !uuids.is_empty() => {
                let mut index = state.index.write().unwrap();
                for uuid in &uuids {
                    index.remove(uuid);
                }
                tracing::info!("{} Einträge wegen Aufbewahrungsfrist entfernt", uuids.len());
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("Aufbewahrungsfrist: {e}"),
        }
    }

    let trash_cutoff = now - db::TRASH_RETENTION_DAYS * 86_400_000;
    match db::purge_trash(&db, trash_cutoff) {
        Ok(n) if n > 0 => tracing::info!("{n} Einträge endgültig aus dem Papierkorb entfernt"),
        Ok(_) => {}
        Err(e) => tracing::warn!("Papierkorb aufräumen: {e}"),
    }
    drop(db);
    let _ = app.emit("history-changed", ());
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
