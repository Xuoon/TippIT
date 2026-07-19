use std::sync::atomic::Ordering;
use std::time::Duration;

use data_encoding::BASE64;
use tauri::{AppHandle, Emitter, Manager, State};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow;

use crate::clipboard::monitor::now_ms;
use crate::clipboard::read::write_image_to_clipboard;
use crate::sound;
use crate::state::AppState;
use crate::storage::db::{self, KIND_IMAGE};
use crate::storage::index::EntryDto;
use crate::storage::{crypto, settings::Settings};
use crate::{typing, windows_util};

#[tauri::command]
pub fn search_history(
    state: State<'_, AppState>,
    query: String,
    kind: Option<u8>,
) -> Vec<EntryDto> {
    state.index.write().unwrap().search(&query, kind, 200)
}

/// Thumbnail als data-URL (entschlüsselt on demand; lädt bewusst NICHT die
/// volle Zeile — der cipher-Blob kann bei Bildern mehrere MB groß sein).
#[tauri::command]
pub fn entry_thumb(state: State<'_, AppState>, uuid: String) -> Option<String> {
    let (kind, thumb) = {
        let db = state.db.lock().unwrap();
        db::get_thumb(&db, &uuid).ok().flatten()?
    };
    let thumb_cipher = thumb?;
    let png = crypto::decrypt(&state.keys.read().unwrap(), &uuid, kind, &thumb_cipher).ok()?;
    Some(format!("data:image/png;base64,{}", BASE64.encode(&png)))
}

/// Voller Textinhalt (für die Detail-Vorschau).
#[tauri::command]
pub fn entry_text(state: State<'_, AppState>, uuid: String) -> Option<String> {
    let row = {
        let db = state.db.lock().unwrap();
        db::get(&db, &uuid).ok().flatten()?
    };
    let cipher = row.cipher?;
    let plain = crypto::decrypt(&state.keys.read().unwrap(), &row.uuid, row.kind, &cipher).ok()?;
    db::payload_to_text(row.kind, &plain)
}

/// Eintrag zurück in die Zwischenablage kopieren (600-Hz-Beep, Fenster zu, nach oben schieben).
#[tauri::command]
pub fn copy_entry(app: AppHandle, uuid: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let row = {
        let db = state.db.lock().unwrap();
        db::get(&db, &uuid).map_err(err)?.ok_or("Eintrag fehlt")?
    };
    let cipher = row.cipher.as_ref().ok_or("Eintrag hat keinen Inhalt")?;
    let plain =
        crypto::decrypt(&state.keys.read().unwrap(), &row.uuid, row.kind, cipher).map_err(err)?;

    match row.kind {
        KIND_IMAGE => write_image_to_clipboard(&plain).map_err(err)?,
        _ => {
            let text = db::payload_to_text(row.kind, &plain).ok_or("Payload unlesbar")?;
            arboard::Clipboard::new()
                .and_then(|mut c| c.set_text(text))
                .map_err(err)?;
        }
    }
    // Nach erfolgreichem Write: Sequenz merken, damit der Monitor die eigene
    // Kopie nicht erfasst (schlägt der Write fehl, wird nichts unterdrückt).
    crate::clipboard::read::mark_own_write(&state);

    // Explizit nach oben schieben (der Monitor ist ja unterdrückt).
    {
        let db = state.db.lock().unwrap();
        let lamport = db::next_lamport(&db).map_err(err)?;
        let now = now_ms();
        db::touch(&db, &uuid, now, lamport).map_err(err)?;
        state.index.write().unwrap().touch(&uuid, now);
    }

    if state.settings.read().unwrap().sounds {
        sound::beep(600, 100);
    }
    // Fenster bleibt bewusst offen — schließen nur über X/Esc/Hotkey.
    state.notify_push();
    let _ = app.emit("history-changed", ());
    Ok(())
}

/// Eintrag als Tastatureingaben ins zuvor fokussierte Fenster tippen.
#[tauri::command]
pub fn type_entry(app: AppHandle, uuid: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let row = {
        let db = state.db.lock().unwrap();
        db::get(&db, &uuid).map_err(err)?.ok_or("Eintrag fehlt")?
    };
    if row.kind == KIND_IMAGE {
        return Err("Bilder können nicht getippt werden".into());
    }
    let cipher = row.cipher.as_ref().ok_or("Eintrag hat keinen Inhalt")?;
    let plain =
        crypto::decrypt(&state.keys.read().unwrap(), &row.uuid, row.kind, cipher).map_err(err)?;
    let text = db::payload_to_text(row.kind, &plain).ok_or("Payload unlesbar")?;

    let (cfg, sounds) = {
        let s = state.settings.read().unwrap();
        (s.typing.clone(), s.sounds)
    };
    let prev_hwnd = state.prev_hwnd.load(Ordering::SeqCst);
    windows_util::hide_history(&app);

    std::thread::spawn(move || {
        let target = HWND(prev_hwnd as *mut core::ffi::c_void);
        if prev_hwnd != 0 {
            let _ = unsafe { SetForegroundWindow(target) };
        }
        // Nicht blind tippen: verifizieren, dass das Ziel wirklich im Vordergrund
        // ist (SetForegroundWindow kann scheitern — Foreground-Lock, RDP) — sonst
        // landet der Inhalt (z. B. ein Passwort) im falschen Fenster.
        if prev_hwnd == 0 || !windows_util::wait_foreground(target, Duration::from_millis(1500)) {
            tracing::warn!("Zielfenster nicht im Vordergrund — tippe nicht");
            if sounds {
                sound::beep_blocking(220, 300);
            }
            return;
        }
        if sounds {
            sound::beep_blocking(440, 200);
        }
        typing::type_text(&text, &cfg);
    });
    Ok(())
}

#[tauri::command]
pub fn pin_entry(app: AppHandle, uuid: String, pinned: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    {
        let db = state.db.lock().unwrap();
        let lamport = db::next_lamport(&db).map_err(err)?;
        db::set_pinned(&db, &uuid, pinned, lamport).map_err(err)?;
    }
    state.index.write().unwrap().set_pinned(&uuid, pinned);
    state.notify_push();
    let _ = app.emit("history-changed", ());
    Ok(())
}

#[tauri::command]
pub fn delete_entry(app: AppHandle, uuid: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    {
        let db = state.db.lock().unwrap();
        let lamport = db::next_lamport(&db).map_err(err)?;
        db::mark_deleted(&db, &uuid, lamport).map_err(err)?;
    }
    state.index.write().unwrap().remove(&uuid);
    state.notify_push();
    let _ = app.emit("history-changed", ());
    Ok(())
}

/// Alle ungepinnten Einträge löschen.
#[tauri::command]
pub fn clear_history(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let removed = {
        let db = state.db.lock().unwrap();
        db::clear_unpinned(&db).map_err(err)?
    };
    {
        let mut index = state.index.write().unwrap();
        for uuid in &removed {
            index.remove(uuid);
        }
    }
    state.notify_push();
    let _ = app.emit("history-changed", ());
    Ok(())
}

#[tauri::command]
pub fn hide_history_window(app: AppHandle) {
    windows_util::hide_history(&app);
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.read().unwrap().clone()
}

#[tauri::command]
pub fn set_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    let state = app.state::<AppState>();
    let (hotkeys_changed, sync_runtime_changed) = {
        let mut current = state.settings.write().unwrap();
        let hotkeys = current.hotkeys.paste != settings.hotkeys.paste
            || current.hotkeys.history != settings.hotkeys.history;
        // Nur diese beiden Felder werden beim Session-Start eingefroren; alle
        // anderen Sync-Einstellungen liest die laufende Loop live aus AppState.
        let sync_runtime = current.sync.deployment_url != settings.sync.deployment_url
            || current.sync.interval_minutes != settings.sync.interval_minutes;
        *current = settings.clone();
        (hotkeys, sync_runtime)
    };
    settings.save(&state.paths).map_err(err)?;
    if hotkeys_changed {
        crate::hotkeys::reregister_all(&app);
    }
    if sync_runtime_changed {
        crate::sync::restart(&app);
    }
    crate::tray::refresh_from_settings(&app);
    state.settings_dirty.store(true, Ordering::SeqCst);
    state.notify_push();
    let _ = app.emit("settings-changed", settings);
    Ok(())
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}
