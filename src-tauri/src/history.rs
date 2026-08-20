use std::sync::atomic::Ordering;
use std::time::Duration;

use data_encoding::BASE64;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::clipboard::monitor::now_ms;
use crate::clipboard::read::{write_html_to_clipboard, write_image_to_clipboard};
use crate::platform;
use crate::sound;
use crate::state::AppState;
use crate::storage::db::{self, TouchSource, KIND_FILES, KIND_IMAGE, KIND_TEXT};
use crate::storage::index::EntryDto;
use crate::storage::portable;
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

/// Bild-Eintrag entschlüsselt als PNG laden (gemeinsamer Kern von OCR und QR).
fn image_png(state: &AppState, uuid: &str) -> Result<Vec<u8>, String> {
    let row = {
        let db = state.db.lock().unwrap();
        db::get(&db, uuid).map_err(err)?.ok_or("Eintrag fehlt")?
    };
    if row.kind != KIND_IMAGE {
        return Err("Nur für Bilder verfügbar".into());
    }
    let cipher = row.cipher.as_ref().ok_or("Eintrag hat keinen Inhalt")?;
    crypto::decrypt(&state.keys, &row.uuid, row.kind, cipher).map_err(err)
}

/// OCR: Text aus Bild-Eintrag extrahieren (macOS Vision, Windows WinRT-OCR).
#[tauri::command]
pub async fn ocr_entry(state: State<'_, AppState>, uuid: String) -> Result<OcrResult, String> {
    let png = image_png(&state, &uuid)?;
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

/// QR-Codes in einem Bild-Eintrag erkennen und dekodieren (rqrr, plattformneutral).
/// Liefert die Klartext-Inhalte aller lesbaren Codes.
#[tauri::command]
pub async fn qr_entry(state: State<'_, AppState>, uuid: String) -> Result<Vec<String>, String> {
    let png = image_png(&state, &uuid)?;
    tauri::async_runtime::spawn_blocking(move || {
        let luma = image::load_from_memory(&png)
            .map_err(|e| format!("Bild unlesbar: {e}"))?
            .to_luma8();
        let mut prepared = rqrr::PreparedImage::prepare(luma);
        let contents: Vec<String> = prepared
            .detect_grids()
            .into_iter()
            .filter_map(|grid| grid.decode().ok().map(|(_meta, content)| content))
            .filter(|c| !c.is_empty())
            .collect();
        Ok(contents)
    })
    .await
    .map_err(err)?
}

/// http(s)-Link öffnen (z. B. dekodierter QR-Inhalt) — bewusst keine anderen
/// Schemes, Parität zur Link-Prüfung in `open_entry`.
#[tauri::command]
pub fn open_link(url: String) -> Result<(), String> {
    let url = url.trim();
    let lower = url.to_ascii_lowercase();
    if !(lower.starts_with("http://") || lower.starts_with("https://")) {
        return Err("Kein Link zum Öffnen".into());
    }
    platform::open_external(url).map_err(err)
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
    let (kind, thumb) = {
        let db = state.db.lock().unwrap();
        db::get_thumb(&db, &uuid).ok().flatten()?
    };
    let thumb_cipher = thumb?;
    let png = crypto::decrypt(&state.keys, &uuid, kind, &thumb_cipher).ok()?;
    Some(format!("data:image/png;base64,{}", BASE64.encode(&png)))
}

/// Volles Bild als data-URL (für die Detail-Vorschau in voller Auflösung; das
/// Thumbnail bleibt für die Listenzeilen). Lädt bewusst NUR für den ausgewählten
/// Eintrag, der cipher-Blob kann mehrere MB groß sein.
#[tauri::command]
pub fn entry_image(state: State<'_, AppState>, uuid: String) -> Option<String> {
    let row = {
        let db = state.db.lock().unwrap();
        db::get(&db, &uuid).ok().flatten()?
    };
    if row.kind != KIND_IMAGE {
        return None;
    }
    let cipher = row.cipher?;
    let png = crypto::decrypt(&state.keys, &row.uuid, row.kind, &cipher).ok()?;
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
    let plain = crypto::decrypt(&state.keys, &row.uuid, row.kind, &cipher).ok()?;
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
    let plain = crypto::decrypt(&state.keys, &row.uuid, row.kind, cipher).map_err(err)?;

    match row.kind {
        KIND_IMAGE => write_image_to_clipboard(&plain).map_err(err)?,
        _ => {
            let text = resolve_text(
                &row,
                db::payload_to_text(row.kind, &plain).ok_or("Payload unlesbar")?,
            );
            // Mit Formatierung, wenn welche gespeichert ist — der Klartext geht
            // immer mit, Zielprogramme ohne HTML bekommen also weiterhin etwas.
            match decrypt_html(&state, &row) {
                Some(html) => write_html_to_clipboard(&html, &text).map_err(err)?,
                None => arboard::Clipboard::new()
                    .and_then(|mut c| c.set_text(text))
                    .map_err(err)?,
            }
        }
    }
    // Nach erfolgreichem Write: Sequenz merken, damit der Monitor die eigene
    // Kopie nicht erfasst (schlägt der Write fehl, wird nichts unterdrückt).
    crate::clipboard::read::mark_own_write(&state);

    // Explizit nach oben schieben (der Monitor ist ja unterdrückt).
    // Source-App: Keep — History-Copy darf Chrome nicht mit TippIT/NULL überschreiben.
    {
        let db = state.db.lock().unwrap();
        let now = now_ms();
        db::touch(&db, &uuid, now, TouchSource::Keep).map_err(err)?;
        state.index.write().unwrap().touch(&uuid, now);
    }

    if state.settings.read().unwrap().sounds {
        sound::beep(600, 100);
    }
    // Fenster bleibt bewusst offen — schließen nur über X/Esc/Hotkey.
    let _ = app.emit("history-changed", ());
    Ok(())
}

/// Beliebigen Text ins zuvor fokussierte Fenster tippen: Fokus-Restore →
/// Vordergrund-Verifikation → Injektion. Gemeinsamer Kern von `type_entry`
/// (DB-Inhalt) und `type_text` (Frontend-Text, z. B. der TOTP-Code).
/// `inject` überschreibt für diesen einen Vorgang, wie der Inhalt ins Zielfenster
/// kommt (Historie: Doppelklick fügt ein, Strg-Doppelklick tippt zeichenweise);
/// ohne Angabe gilt der eingestellte Tippmodus.
fn spawn_type(app: &AppHandle, text: String, inject: Option<typing::Inject>) {
    let state = app.state::<AppState>();
    let (cfg, sounds) = {
        let s = state.settings.read().unwrap();
        (s.typing.clone(), s.sounds)
    };
    let inject = inject.unwrap_or_else(|| cfg.mode.clone().into());
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
        // ESC während Fokus-Restore/Verifikation: nicht mehr weitermachen.
        if !typing::alive(&app2, generation) {
            return;
        }
        if sounds {
            sound::beep_blocking(440, 200);
        }
        if !typing::alive(&app2, generation) {
            return;
        }
        typing::type_text(&app2, &text, &cfg, generation, inject);
    });
}

/// Eintrag als Tastatureingaben ins zuvor fokussierte Fenster tippen.
#[tauri::command]
pub fn type_entry(
    app: AppHandle,
    uuid: String,
    mode: Option<typing::Inject>,
) -> Result<(), String> {
    let state = app.state::<AppState>();
    let row = {
        let db = state.db.lock().unwrap();
        db::get(&db, &uuid).map_err(err)?.ok_or("Eintrag fehlt")?
    };
    if row.kind == KIND_IMAGE {
        return Err("Bilder können nicht getippt werden".into());
    }
    let cipher = row.cipher.as_ref().ok_or("Eintrag hat keinen Inhalt")?;
    let plain = crypto::decrypt(&state.keys, &row.uuid, row.kind, cipher).map_err(err)?;
    let text = resolve_text(
        &row,
        db::payload_to_text(row.kind, &plain).ok_or("Payload unlesbar")?,
    );
    spawn_type(&app, text, mode);
    Ok(())
}

/// Beliebigen Text tippen (z. B. den generierten TOTP-Code) — Inhalt kommt vom
/// Frontend, nicht aus der DB.
#[tauri::command]
pub fn type_text(app: AppHandle, text: String, mode: Option<typing::Inject>) -> Result<(), String> {
    if text.is_empty() {
        return Err("Kein Text zum Tippen".into());
    }
    spawn_type(&app, text, mode);
    Ok(())
}

/// Datenverzeichnis (`~/.labi/tippit/`) im Dateimanager öffnen — Ziel des
/// Log-Pfads in der Hilfe.
#[tauri::command]
pub fn open_data_dir(state: State<'_, AppState>) -> Result<(), String> {
    platform::open_external(&state.paths.root.to_string_lossy()).map_err(err)
}

/// Datei(en) bzw. Link eines Eintrags im Standard-Handler öffnen. Öffnet nur
/// KIND_FILES-Pfade und http(s)-Links — keine beliebigen Schemes aus Text.
#[tauri::command]
pub fn open_entry(state: State<'_, AppState>, uuid: String) -> Result<(), String> {
    let row = {
        let db = state.db.lock().unwrap();
        db::get(&db, &uuid).map_err(err)?.ok_or("Eintrag fehlt")?
    };
    let cipher = row.cipher.as_ref().ok_or("Eintrag hat keinen Inhalt")?;
    let plain = crypto::decrypt(&state.keys, &row.uuid, row.kind, cipher).map_err(err)?;
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
            // Schemes sind laut RFC 3986 case-insensitiv; das Frontend (`isLink`)
            // erkennt Links ebenfalls case-insensitiv, also hier gleichziehen.
            let lower = text.to_ascii_lowercase();
            if !(lower.starts_with("http://") || lower.starts_with("https://")) {
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
        db::set_pinned(&db, &uuid, pinned).map_err(err)?;
    }
    state.index.write().unwrap().set_pinned(&uuid, pinned);
    let _ = app.emit("history-changed", ());
    Ok(())
}

/// Löschen heißt: ab in den Papierkorb. Endgültig entfernt wird erst durch
/// `purge_entry`, `empty_trash` oder die 30-Tage-Frist beim nächsten Start.
#[tauri::command]
pub fn delete_entry(app: AppHandle, uuid: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    {
        let db = state.db.lock().unwrap();
        db::trash(&db, &uuid, now_ms()).map_err(err)?;
    }
    state.index.write().unwrap().remove(&uuid);
    let _ = app.emit("history-changed", ());
    Ok(())
}

/// Eintrag aus dem Papierkorb zurückholen.
#[tauri::command]
pub fn restore_entry(app: AppHandle, uuid: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    let row = {
        let db = state.db.lock().unwrap();
        db::restore(&db, &uuid).map_err(err)?;
        db::get(&db, &uuid).map_err(err)?.ok_or("Eintrag fehlt")?
    };
    let keys = state.keys.clone();
    state.index.write().unwrap().upsert(&row, &keys);
    let _ = app.emit("history-changed", ());
    Ok(())
}

/// Einzelnen Eintrag endgültig entfernen.
#[tauri::command]
pub fn purge_entry(app: AppHandle, uuid: String) -> Result<(), String> {
    let state = app.state::<AppState>();
    {
        let db = state.db.lock().unwrap();
        db::purge(&db, &uuid).map_err(err)?;
    }
    state.index.write().unwrap().remove(&uuid);
    let _ = app.emit("history-changed", ());
    Ok(())
}

#[derive(serde::Serialize)]
pub struct TrashDto {
    pub uuid: String,
    pub kind: u8,
    pub preview: String,
    pub trashed_at: i64,
    pub size_bytes: i64,
}

/// Papierkorb-Inhalt. Wird bei jedem Aufruf frisch entschlüsselt — der
/// Suchindex führt bewusst nur aktive Einträge.
#[tauri::command]
pub fn list_trash(state: State<'_, AppState>) -> Result<Vec<TrashDto>, String> {
    let rows = {
        let db = state.db.lock().unwrap();
        db::list_trashed(&db).map_err(err)?
    };
    let keys = &state.keys;
    Ok(rows
        .into_iter()
        .map(|row| {
            let preview = match row.kind {
                KIND_IMAGE => format!("Bild ({} KB)", (row.size_bytes / 1024).max(1)),
                _ => row
                    .cipher
                    .as_ref()
                    .and_then(|c| crypto::decrypt(keys, &row.uuid, row.kind, c).ok())
                    .and_then(|plain| db::payload_to_text(row.kind, &plain))
                    .map(|text| {
                        text.split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ")
                            .chars()
                            .take(200)
                            .collect::<String>()
                    })
                    .unwrap_or_else(|| "(nicht lesbar)".into()),
            };
            TrashDto {
                uuid: row.uuid,
                kind: row.kind,
                preview,
                trashed_at: row.trashed_at,
                size_bytes: row.size_bytes,
            }
        })
        .collect())
}

/// Papierkorb endgültig leeren.
#[tauri::command]
pub fn empty_trash(app: AppHandle) -> Result<usize, String> {
    let state = app.state::<AppState>();
    let n = {
        let db = state.db.lock().unwrap();
        db::purge_trash(&db, 0).map_err(err)?
    };
    let _ = app.emit("history-changed", ());
    Ok(n)
}

/// Alle ungepinnten Einträge in den Papierkorb legen.
#[tauri::command]
pub fn clear_history(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let removed = {
        let db = state.db.lock().unwrap();
        db::clear_unpinned(&db, now_ms()).map_err(err)?
    };
    {
        let mut index = state.index.write().unwrap();
        for uuid in &removed {
            index.remove(uuid);
        }
    }
    let _ = app.emit("history-changed", ());
    Ok(())
}

/// Eintrag zum dauerhaften Textbaustein machen (oder zurück zur normalen Kopie).
/// Bausteine überleben Limit, Aufbewahrungsfrist und „Historie löschen".
#[tauri::command]
pub fn set_entry_snippet(app: AppHandle, uuid: String, snippet: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    {
        let db = state.db.lock().unwrap();
        db::set_snippet(&db, &uuid, snippet).map_err(err)?;
    }
    state.index.write().unwrap().set_snippet(&uuid, snippet);
    let _ = app.emit("history-changed", ());
    Ok(())
}

/// Neuen Textbaustein aus eingegebenem Text anlegen.
#[tauri::command]
pub fn create_snippet(app: AppHandle, text: String) -> Result<String, String> {
    let text = text.trim().to_string();
    if text.is_empty() {
        return Err("Ein Textbaustein braucht Text".into());
    }
    let state = app.state::<AppState>();
    let uuid = uuid::Uuid::now_v7().to_string();
    let keys = state.keys.clone();
    let plain = text.into_bytes();
    let now = now_ms();
    let row = db::EntryRow {
        uuid: uuid.clone(),
        kind: KIND_TEXT,
        cipher: Some(crypto::encrypt(&keys, &uuid, KIND_TEXT, &plain).map_err(err)?),
        thumb: None,
        html: None,
        size_bytes: plain.len() as i64,
        hash: crypto::sha256(&plain).to_vec(),
        created_at: now,
        pinned: false,
        trashed_at: 0,
        snippet: true,
        source_app_id: None,
        source_app_name: None,
        first_created_at: now,
        copy_count: 1,
    };
    {
        let db = state.db.lock().unwrap();
        db::insert(&db, &row).map_err(err)?;
    }
    state.index.write().unwrap().upsert(&row, &keys);
    let _ = app.emit("history-changed", ());
    Ok(uuid)
}

/// Formatierte Fassung eines Eintrags fürs Vorschau-Rendering. Wird beim
/// Ausliefern ERNEUT sanitisiert: gespeichert wurde zwar bereits gereinigtes
/// HTML, aber eine Historie aus einer älteren Version soll die WebView
/// trotzdem nicht ungefiltert erreichen.
#[tauri::command]
pub fn entry_html(state: State<'_, AppState>, uuid: String) -> Option<String> {
    let row = {
        let db = state.db.lock().unwrap();
        db::get(&db, &uuid).ok().flatten()?
    };
    decrypt_html(&state, &row)
}

fn decrypt_html(state: &AppState, row: &db::EntryRow) -> Option<String> {
    let blob = row.html.as_ref()?;
    let plain = crypto::decrypt(&state.keys, &row.uuid, db::AAD_HTML, blob).ok()?;
    crate::clipboard::html::sanitize(&String::from_utf8_lossy(&plain))
}

/// Datei-Dialoge laufen bewusst in Rust statt im Frontend: so bleibt die
/// WebView-Capability auf `core:default` und das Frontend braucht kein
/// Dialog-Plugin. Rückgabe `None` = Nutzer hat abgebrochen.
async fn ask_path(
    app: &AppHandle,
    save: bool,
    suggestion: &str,
    filter: (&str, &str),
) -> Option<std::path::PathBuf> {
    use tauri_plugin_dialog::DialogExt;

    let (tx, rx) = tokio::sync::oneshot::channel();
    let builder = app
        .dialog()
        .file()
        .set_file_name(suggestion)
        .add_filter(filter.0, &[filter.1]);
    if save {
        builder.save_file(move |path| {
            let _ = tx.send(path);
        });
    } else {
        builder.pick_file(move |path| {
            let _ = tx.send(path);
        });
    }
    rx.await.ok().flatten().and_then(|p| p.into_path().ok())
}

/// Bild-Eintrag als PNG-Datei ablegen. `None` = abgebrochen.
#[tauri::command]
pub async fn save_entry_image(app: AppHandle, uuid: String) -> Result<Option<String>, String> {
    let png = {
        let state = app.state::<AppState>();
        image_png(&state, &uuid)?
    };
    let Some(path) = ask_path(&app, true, "tippit-bild.png", ("PNG-Bild", "png")).await else {
        return Ok(None);
    };
    std::fs::write(&path, png).map_err(err)?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// Verschlüsselten Export aller Einträge schreiben. `None` = abgebrochen.
#[tauri::command]
pub async fn export_history(app: AppHandle, password: String) -> Result<Option<usize>, String> {
    let Some(path) = ask_path(
        &app,
        true,
        "tippit-historie.tippit",
        ("TippIT-Sicherung", "tippit"),
    )
    .await
    else {
        return Ok(None);
    };
    // Die Schlüsselableitung braucht bewusst rund eine Sekunde — nicht im
    // Command-Thread laufen lassen, sonst steht die Oberfläche.
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let state = app2.state::<AppState>();
        portable::export(&state, &path, &password).map_err(err)
    })
    .await
    .map_err(err)?
    .map(Some)
}

/// Export-Datei einlesen und zusammenführen (vorhandene Einträge bleiben).
/// `None` = abgebrochen.
#[tauri::command]
pub async fn import_history(
    app: AppHandle,
    password: String,
) -> Result<Option<portable::ImportReport>, String> {
    let Some(path) = ask_path(
        &app,
        false,
        "tippit-historie.tippit",
        ("TippIT-Sicherung", "tippit"),
    )
    .await
    else {
        return Ok(None);
    };
    let app2 = app.clone();
    let report = tauri::async_runtime::spawn_blocking(move || {
        let state = app2.state::<AppState>();
        portable::import(&state, &path, &password).map_err(err)
    })
    .await
    .map_err(err)??;
    let _ = app.emit("history-changed", ());
    Ok(Some(report))
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
    let (hotkeys_changed, retention_changed) = {
        let mut current = state.settings.write().unwrap();
        let hotkeys = current.hotkeys.paste != settings.hotkeys.paste
            || current.hotkeys.history != settings.hotkeys.history;
        let retention = current.history.retention_days != settings.history.retention_days;
        *current = settings.clone();
        (hotkeys, retention)
    };
    settings.save(&state.paths).map_err(err)?;
    if hotkeys_changed {
        crate::hotkeys::reregister_all(&app);
    }
    // Eine gerade verkürzte Frist soll sofort greifen, nicht erst beim Neustart.
    if retention_changed {
        crate::clipboard::monitor::run_retention(&app);
    }
    crate::tray::refresh_from_settings(&app);
    let _ = app.emit("settings-changed", settings);
    Ok(())
}

/// Textbausteine dürfen Platzhalter tragen; erfasste Kopien bleiben unangetastet
/// (in einem kopierten Text ist „{datum}" schlicht Text, den man genau so will).
fn resolve_text(row: &db::EntryRow, text: String) -> String {
    if !row.snippet || !text.contains('{') {
        return text;
    }
    let (date, time) = platform::local_date_time();
    text.replace("{datumzeit}", &format!("{date} {time}"))
        .replace("{datum}", &date)
        .replace("{uhrzeit}", &time)
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}
