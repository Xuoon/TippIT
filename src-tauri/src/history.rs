use std::sync::atomic::Ordering;
use std::time::Duration;

use data_encoding::BASE64;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::clipboard::monitor::now_ms;
use crate::clipboard::read::write_image_to_clipboard;
use crate::platform;
use crate::sound;
use crate::state::AppState;
use crate::storage::db::{self, TouchSource, KIND_FILES, KIND_IMAGE, KIND_TEXT};
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

/// Ziel-App für „In … einfügen" (vor dem Öffnen der Historie gemerkt).
#[tauri::command]
pub fn history_target_app(state: State<'_, AppState>) -> Option<TargetAppDto> {
    state
        .prev_target_app
        .lock()
        .unwrap()
        .as_ref()
        .map(|(name, id)| TargetAppDto {
            name: name.clone(),
            id: id.clone(),
        })
}

#[derive(serde::Serialize)]
pub struct TargetAppDto {
    pub name: String,
    pub id: String,
}

#[derive(serde::Serialize)]
pub struct OcrBlock {
    pub text: String,
    /// Normalisiert [0,1], Ursprung oben-links — direkt als CSS-Prozente nutzbar.
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

#[derive(serde::Serialize)]
pub struct OcrResult {
    /// Zeilen als Klartext (Panel + Kopieren) — Single Source ist `blocks`.
    pub text: String,
    /// Positionierte Zeilen für das markierbare Overlay über dem Bild.
    pub blocks: Vec<OcrBlock>,
}

/// OCR: Text aus Bild-Eintrag extrahieren (macOS Vision; Windows derzeit nicht unterstützt).
#[tauri::command]
pub async fn ocr_entry(state: State<'_, AppState>, uuid: String) -> Result<OcrResult, String> {
    let png = {
        let _rotation = state.rotation_lock.lock().unwrap();
        let row = {
            let db = state.db.lock().unwrap();
            db::get(&db, &uuid).map_err(err)?.ok_or("Eintrag fehlt")?
        };
        if row.kind != KIND_IMAGE {
            return Err("OCR nur für Bilder".into());
        }
        let cipher = row.cipher.as_ref().ok_or("Eintrag hat keinen Inhalt")?;
        crypto::decrypt(&state.keys.read().unwrap(), &row.uuid, row.kind, cipher).map_err(err)?
    };
    let lines = tauri::async_runtime::spawn_blocking(move || platform::ocr_png(&png))
        .await
        .map_err(err)?
        .map_err(err)?;
    let text = lines
        .iter()
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let blocks = lines
        .into_iter()
        .map(|l| OcrBlock {
            text: l.text,
            x: l.x,
            y: l.y,
            w: l.w,
            h: l.h,
        })
        .collect();
    Ok(OcrResult { text, blocks })
}

/// Beliebigen UI-Text in die Zwischenablage schreiben, ohne ihn erneut zu erfassen.
#[tauri::command]
pub fn copy_text(app: AppHandle, text: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text))
        .map_err(err)?;
    crate::clipboard::read::mark_own_write(&state);
    Ok(())
}

/// Quellanwendungs-Icon als data-URL (Disk-Cache unter app-icons/; hash-safe).
#[tauri::command]
pub fn source_app_icon(state: State<'_, AppState>, app_id: String) -> Option<String> {
    if app_id.is_empty() {
        return None;
    }
    let path = state.paths.app_icon_file(&app_id);
    let png = std::fs::read(path).ok()?;
    if png.is_empty() {
        return None;
    }
    Some(format!("data:image/png;base64,{}", BASE64.encode(&png)))
}

/// Thumbnail als data-URL (entschlüsselt on demand; lädt bewusst NICHT die
/// volle Zeile — der cipher-Blob kann bei Bildern mehrere MB groß sein).
#[tauri::command]
pub fn entry_thumb(state: State<'_, AppState>, uuid: String) -> Option<String> {
    let _rotation = state.rotation_lock.lock().unwrap();
    let (kind, thumb) = {
        let db = state.db.lock().unwrap();
        db::get_thumb(&db, &uuid).ok().flatten()?
    };
    let thumb_cipher = thumb?;
    let png = crypto::decrypt(&state.keys.read().unwrap(), &uuid, kind, &thumb_cipher).ok()?;
    Some(format!("data:image/png;base64,{}", BASE64.encode(&png)))
}

/// Volles Bild als data-URL (für die Detail-Vorschau in voller Auflösung; das
/// Thumbnail bleibt für die Listenzeilen). Lädt bewusst NUR für den ausgewählten
/// Eintrag, der cipher-Blob kann mehrere MB groß sein.
#[tauri::command]
pub fn entry_image(state: State<'_, AppState>, uuid: String) -> Option<String> {
    let _rotation = state.rotation_lock.lock().unwrap();
    let row = {
        let db = state.db.lock().unwrap();
        db::get(&db, &uuid).ok().flatten()?
    };
    if row.kind != KIND_IMAGE {
        return None;
    }
    let cipher = row.cipher?;
    let png = crypto::decrypt(&state.keys.read().unwrap(), &row.uuid, row.kind, &cipher).ok()?;
    Some(format!("data:image/png;base64,{}", BASE64.encode(&png)))
}

/// Voller Textinhalt (für die Detail-Vorschau).
#[tauri::command]
pub fn entry_text(state: State<'_, AppState>, uuid: String) -> Option<String> {
    let _rotation = state.rotation_lock.lock().unwrap();
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
    let _rotation = state.rotation_lock.lock().unwrap();
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
    // Source-App: Keep — History-Copy darf Chrome nicht mit TippIT/NULL überschreiben.
    {
        let db = state.db.lock().unwrap();
        let lamport = db::next_lamport(&db).map_err(err)?;
        let now = now_ms();
        db::touch(&db, &uuid, now, lamport, TouchSource::Keep).map_err(err)?;
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

/// Beliebigen Text ins zuvor fokussierte Fenster tippen: Fokus-Restore →
/// Vordergrund-Verifikation → Injektion. Gemeinsamer Kern von `type_entry`
/// (DB-Inhalt) und `type_text` (Frontend-Text, z. B. der TOTP-Code).
fn spawn_type(app: &AppHandle, text: String) {
    let state = app.state::<AppState>();
    let (cfg, sounds) = {
        let s = state.settings.read().unwrap();
        (s.typing.clone(), s.sounds)
    };
    let prev_target = state.prev_target.load(Ordering::SeqCst);
    windows_util::hide_history(app);
    // Preemption: einen evtl. laufenden Vorgang zum Abbruch anstoßen, damit er den
    // typing_lock zeitnah freigibt. Die eigene Generation wird bewusst ERST nach
    // Lock-Erwerb gezogen (s. AppState::typing_gen) — sonst könnte der terminale
    // Bump des Vorgängers sie noch entwerten.
    state.typing_gen.fetch_add(1, Ordering::SeqCst);
    let app2 = app.clone();

    std::thread::spawn(move || {
        let state2 = app2.state::<AppState>();
        // Blockierend statt try_lock: der Pre-Lock-Bump stößt den Vorgänger an,
        // gibt den Lock also zeitnah frei — Warten ist hier korrekt (kein Deadlock)
        // und verhindert, dass „Eintrag tippen" bei einem laufenden Vorgang still
        // nichts tut.
        let _guard = state2.typing_lock.lock().unwrap();
        let generation = state2.typing_gen.fetch_add(1, Ordering::SeqCst) + 1;
        // ESC bricht ab — schon während Fokus-Restore und Vorbereitungs-Beep.
        let _esc = typing::EscCancelGuard::new(&app2);
        if prev_target != 0 {
            platform::activate_target(prev_target);
        }
        // Nicht blind tippen: verifizieren, dass das Ziel wirklich im Vordergrund
        // ist (die Aktivierung kann scheitern — Foreground-Lock, RDP) — sonst
        // landet der Inhalt (z. B. ein Passwort) im falschen Fenster.
        if prev_target == 0 || !platform::wait_foreground(prev_target, Duration::from_millis(1500))
        {
            tracing::warn!("Zielfenster nicht im Vordergrund — tippe nicht");
            if sounds {
                sound::beep_blocking(220, 300);
            }
            return;
        }
        if sounds {
            sound::beep_blocking(440, 200);
        }
        typing::type_text(&app2, &text, &cfg, generation);
    });
}

/// Eintrag als Tastatureingaben ins zuvor fokussierte Fenster tippen.
#[tauri::command]
pub fn type_entry(app: AppHandle, uuid: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let rotation = state.rotation_lock.lock().unwrap();
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
    drop(rotation);
    spawn_type(&app, text);
    Ok(())
}

/// Beliebigen Text tippen (z. B. den generierten TOTP-Code) — Inhalt kommt vom
/// Frontend, nicht aus der DB.
#[tauri::command]
pub fn type_text(app: AppHandle, text: String) -> Result<(), String> {
    if text.is_empty() {
        return Err("Kein Text zum Tippen".into());
    }
    spawn_type(&app, text);
    Ok(())
}

/// Datei(en) bzw. Link eines Eintrags im Standard-Handler öffnen. Öffnet nur
/// KIND_FILES-Pfade und http(s)-Links — keine beliebigen Schemes aus Text.
#[tauri::command]
pub fn open_entry(state: State<'_, AppState>, uuid: String) -> Result<(), String> {
    let _rotation = state.rotation_lock.lock().unwrap();
    let row = {
        let db = state.db.lock().unwrap();
        db::get(&db, &uuid).map_err(err)?.ok_or("Eintrag fehlt")?
    };
    let cipher = row.cipher.as_ref().ok_or("Eintrag hat keinen Inhalt")?;
    let plain =
        crypto::decrypt(&state.keys.read().unwrap(), &row.uuid, row.kind, cipher).map_err(err)?;
    match row.kind {
        KIND_FILES => {
            let paths: Vec<String> = serde_json::from_slice(&plain).map_err(err)?;
            if paths.is_empty() {
                return Err("Keine Pfade zum Öffnen".into());
            }
            for path in &paths {
                platform::open_external(path).map_err(err)?;
            }
            Ok(())
        }
        KIND_TEXT => {
            let text = String::from_utf8_lossy(&plain).trim().to_string();
            if !(text.starts_with("http://") || text.starts_with("https://")) {
                return Err("Kein Link zum Öffnen".into());
            }
            platform::open_external(&text).map_err(err)
        }
        _ => Err("Eintrag lässt sich nicht öffnen".into()),
    }
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

/// Auslieferungs-Defaults fürs Frontend (Standard-Markierungen an Slidern etc.).
/// Einzige Quelle sind `defaults.json` + die Rust-Default-Impls — das Frontend
/// dupliziert keine Werte.
#[tauri::command]
pub fn default_settings() -> Settings {
    Settings::shipped_defaults()
}

#[tauri::command]
pub fn set_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    let state = app.state::<AppState>();
    let sync_settings_changed = {
        let current = state.settings.read().unwrap();
        crate::sync::settings_sync_bytes(&current).map_err(err)?
            != crate::sync::settings_sync_bytes(&settings).map_err(err)?
    };
    let (hotkeys_changed, sync_runtime_changed, sync_scope_expanded) = {
        let mut current = state.settings.write().unwrap();
        let hotkeys = current.hotkeys.paste != settings.hotkeys.paste
            || current.hotkeys.history != settings.hotkeys.history;
        // Nur diese beiden Felder werden beim Session-Start eingefroren; alle
        // anderen Sync-Einstellungen liest die laufende Loop live aus AppState.
        let sync_runtime = current.sync.deployment_url != settings.sync.deployment_url
            || current.sync.interval_minutes != settings.sync.interval_minutes;
        // Zuvor ausgeschlossene Zeilen wurden als lokal erledigt markiert. Wird
        // der Scope erweitert, müssen sie erneut durch die Scope-Prüfung laufen.
        let sync_scope = (!current.sync.sync_text && settings.sync.sync_text)
            || (!current.sync.sync_images && settings.sync.sync_images)
            || (settings.sync.sync_images
                && settings.sync.image_max_bytes > current.sync.image_max_bytes);
        *current = settings.clone();
        (hotkeys, sync_runtime, sync_scope)
    };
    settings.save(&state.paths).map_err(err)?;
    if sync_scope_expanded {
        let db = state.db.lock().unwrap();
        db::mark_all_dirty(&db).map_err(err)?;
    }
    if hotkeys_changed {
        crate::hotkeys::reregister_all(&app);
    }
    if sync_runtime_changed {
        crate::sync::restart(&app);
    }
    crate::tray::refresh_from_settings(&app);
    if !settings.sync.sync_settings {
        crate::sync::clear_settings_dirty(&state);
    } else if sync_settings_changed {
        crate::sync::mark_settings_dirty(&state);
        state.notify_push();
    }
    let _ = app.emit("settings-changed", settings);
    Ok(())
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}
