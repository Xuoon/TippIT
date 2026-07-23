use std::sync::atomic::Ordering;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::state::AppState;
use crate::storage::crypto::{self, Secret};
use crate::storage::db;

use super::{policy, SyncClient, SyncState};

#[derive(Serialize)]
pub struct SyncStatus {
    pub active: bool,
    pub group_id: Option<String>,
    pub deployment_url: String,
    /// Windows-Richtlinie, die den Sync gerade pausiert (nur wenn aktiv).
    pub blocked_reason: Option<policy::BlockReason>,
}

#[tauri::command]
pub fn sync_status(state: State<'_, AppState>) -> SyncStatus {
    let sync_state = SyncState::load(&state.paths);
    let active = sync_state.is_some();
    SyncStatus {
        active,
        group_id: sync_state.map(|s| s.group_id),
        deployment_url: state.settings.read().unwrap().sync.deployment_url.clone(),
        blocked_reason: active
            .then(|| policy::block_reason(&state.settings.read().unwrap().sync))
            .flatten(),
    }
}

#[derive(Serialize)]
pub struct PairingInfo {
    pub code: String,
    pub qr_svg: String,
}

/// Kopplungscode + QR anzeigen (nur sinnvoll, wenn eine Gruppe aktiv ist).
#[tauri::command]
pub fn sync_show_pairing(state: State<'_, AppState>) -> Result<PairingInfo, String> {
    let _rotation = state.rotation_lock.lock().unwrap();
    let secret = Secret::load(&state.paths)
        .map_err(err)?
        .ok_or("Kein Schlüssel vorhanden")?;
    let code = secret.to_pairing_code();
    let qr = qrcode::QrCode::new(code.as_bytes()).map_err(err)?;
    let qr_svg = qr
        .render::<qrcode::render::svg::Color>()
        .min_dimensions(220, 220)
        .quiet_zone(true)
        .build();
    Ok(PairingInfo { code, qr_svg })
}

/// Kopplungscode in die Zwischenablage kopieren (ohne Historie-Erfassung).
#[tauri::command]
pub fn sync_copy_code(state: State<'_, AppState>) -> Result<(), String> {
    let _rotation = state.rotation_lock.lock().unwrap();
    let secret = Secret::load(&state.paths)
        .map_err(err)?
        .ok_or("Kein Code vorhanden")?;
    let code = secret.to_pairing_code();
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(code))
        .map_err(err)?;
    crate::clipboard::read::mark_own_write(&state);
    Ok(())
}

/// Frisches Secret erzeugen, gesamte lokale Historie umschlüsseln, Gruppe verlassen.
/// Gemeinsamer Kern von „Neuen Code erstellen" und „Gruppe verlassen".
fn rotate_to_new_secret(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let _rotation = state.rotation_lock.lock().unwrap();
    state.sync_gen.fetch_add(1, Ordering::SeqCst);
    *state.push_notify.lock().unwrap() = None;

    let new_secret = Secret::generate().map_err(err)?;
    let new_keys = new_secret.derive_keys();
    // Zwei-Phasen-Rotation: neues Secret zuerst als key.bin.new persistieren.
    // Schlägt danach etwas fehl oder crasht die App, entscheidet der nächste
    // Start per Probe-Decrypt, welcher Schlüssel zur DB passt (lib.rs::resolve_secret).
    new_secret.store_pending(&state.paths).map_err(err)?;
    if let Err(e) = reencrypt_all(app, &new_keys) {
        Secret::remove_pending(&state.paths);
        return Err(err(e));
    }
    if let Err(e) = Secret::promote_pending(&state.paths) {
        // DB ist bereits umgeschlüsselt und key.bin.new liegt vor — der nächste
        // Start promotet es; diese Session läuft mit den neuen Keys weiter.
        tracing::warn!("key.bin.new nicht promotet ({e}) — Recovery beim nächsten Start");
    }
    *state.keys.write().unwrap() = new_keys;
    SyncState::remove(&state.paths);
    Ok(())
}

/// Neuen Code (= neues Secret) erzeugen und speichern.
/// Trennt eine aktive Sync-Gruppe; die Bestätigung dafür holt das Frontend ein.
#[tauri::command]
pub async fn sync_new_code(app: AppHandle) -> Result<PairingInfo, String> {
    rotate_to_new_secret(&app)?;
    sync_show_pairing(app.state())
}

/// Gerät 1: Sync-Gruppe aus dem eigenen Secret erstellen.
#[tauri::command]
pub async fn sync_create_group(app: AppHandle) -> Result<SyncStatus, String> {
    let (url, group_id, auth) = {
        let state = app.state::<AppState>();
        let url = state.settings.read().unwrap().sync.deployment_url.clone();
        if url.trim().is_empty() {
            return Err("Bitte zuerst die Convex-Deployment-URL eintragen".into());
        }
        let keys = state.keys.read().unwrap();
        (url, keys.group_id.clone(), keys.auth)
    };

    let mut client = SyncClient::connect(&url, &group_id, &auth)
        .await
        .map_err(err)?;
    client.create_group().await.map_err(err)?;

    let state = app.state::<AppState>();
    let _rotation = state.rotation_lock.lock().unwrap();
    if state.keys.read().unwrap().group_id != group_id {
        return Err("Schlüssel wurden während der Gruppenerstellung geändert".into());
    }
    SyncState {
        group_id: group_id.clone(),
        watermark: 0,
    }
    .save(&state.paths)
    .map_err(err)?;
    {
        let db = state.db.lock().unwrap();
        db::mark_all_dirty(&db).map_err(err)?;
    }
    super::restart(&app);
    Ok(sync_status(app.state()))
}

/// Gerät 2: mit Kopplungscode beitreten — Secret übernehmen und lokale Daten umschlüsseln.
#[tauri::command]
pub async fn sync_join_group(app: AppHandle, code: String) -> Result<SyncStatus, String> {
    let new_secret = Secret::from_pairing_code(&code).map_err(err)?;
    let new_keys = new_secret.derive_keys();

    let url = {
        let state = app.state::<AppState>();
        let url = state.settings.read().unwrap().sync.deployment_url.clone();
        if url.trim().is_empty() {
            return Err("Bitte zuerst die Convex-Deployment-URL eintragen".into());
        }
        url
    };

    // Gruppe muss auf dem Server erreichbar sein (createGroup ist idempotent).
    let mut client = SyncClient::connect(&url, &new_keys.group_id, &new_keys.auth)
        .await
        .map_err(err)?;
    client.create_group().await.map_err(err)?;

    // Loop stoppen, umschlüsseln, Schlüssel tauschen, neu starten.
    // Zwei-Phasen-Rotation wie in rotate_to_new_secret (s. dort).
    let state = app.state::<AppState>();
    let _rotation = state.rotation_lock.lock().unwrap();
    state.sync_gen.fetch_add(1, Ordering::SeqCst);
    new_secret.store_pending(&state.paths).map_err(err)?;
    if let Err(e) = reencrypt_all(&app, &new_keys) {
        Secret::remove_pending(&state.paths);
        return Err(err(e));
    }
    if let Err(e) = Secret::promote_pending(&state.paths) {
        tracing::warn!("key.bin.new nicht promotet ({e}) — Recovery beim nächsten Start");
    }
    *state.keys.write().unwrap() = new_keys.clone();

    SyncState {
        group_id: new_keys.group_id.clone(),
        watermark: 0,
    }
    .save(&state.paths)
    .map_err(err)?;
    {
        let db = state.db.lock().unwrap();
        db::mark_all_dirty(&db).map_err(err)?;
    }
    super::restart(&app);
    Ok(sync_status(app.state()))
}

/// Gruppe verlassen: neues Secret (alte Mitglieder können künftige Daten nicht lesen),
/// lokale Historie bleibt erhalten.
#[tauri::command]
pub async fn sync_leave_group(app: AppHandle) -> Result<SyncStatus, String> {
    rotate_to_new_secret(&app)?;
    Ok(sync_status(app.state()))
}

#[derive(Serialize)]
pub struct DeviceDto {
    pub device_id: String,
    pub name: String,
    pub platform: String,
    pub last_seen_at: i64,
    pub is_self: bool,
}

/// Geräte der aktiven Sync-Gruppe (Anzeige im Sync-Tab). Ad-hoc-Verbindung wie
/// bei den Pairing-Commands; ohne aktive Gruppe leere Liste.
#[tauri::command]
pub async fn sync_devices(app: AppHandle) -> Result<Vec<DeviceDto>, String> {
    let (url, group_id, auth, self_id) = {
        let state = app.state::<AppState>();
        if SyncState::load(&state.paths).is_none() {
            return Ok(Vec::new());
        }
        let url = state.settings.read().unwrap().sync.deployment_url.clone();
        let keys = state.keys.read().unwrap();
        (
            url,
            keys.group_id.clone(),
            keys.auth,
            state.device_id.clone(),
        )
    };
    let mut client = SyncClient::connect(&url, &group_id, &auth)
        .await
        .map_err(err)?;
    let mut devices: Vec<DeviceDto> = client
        .list_devices()
        .await
        .map_err(err)?
        .into_iter()
        .map(|d| DeviceDto {
            is_self: d.device_id == self_id,
            device_id: d.device_id,
            name: d.name,
            platform: d.platform,
            last_seen_at: d.last_seen_at,
        })
        .collect();
    // Eigenes Gerät zuerst, danach nach Aktivität.
    devices.sort_by(|a, b| {
        b.is_self
            .cmp(&a.is_self)
            .then(b.last_seen_at.cmp(&a.last_seen_at))
    });
    Ok(devices)
}

/// Alle Ciphertexte von den aktuellen auf neue Schlüssel umschlüsseln.
/// Läuft in EINER Transaktion: ist auch nur eine Zeile nicht entschlüsselbar,
/// bleibt die DB komplett auf dem alten Schlüssel. Überspringen würde nach dem
/// Key-Tausch eine gemischt verschlüsselte, dauerhaft teilweise unlesbare DB erzeugen.
fn reencrypt_all(app: &AppHandle, new_keys: &crypto::CryptoKeys) -> anyhow::Result<()> {
    let state = app.state::<AppState>();
    let old_keys = state.keys.read().unwrap().clone();
    let db = state.db.lock().unwrap();
    let tx = db.unchecked_transaction()?;
    let rows = db::list_all(&tx)?;
    tracing::info!(
        "Schlüsselrotation: {} Einträge werden umgeschlüsselt",
        rows.len()
    );
    for row in rows {
        let reencrypt = |blob: &Option<Vec<u8>>| -> anyhow::Result<Option<Vec<u8>>> {
            match blob {
                Some(data) => {
                    let plain = crypto::decrypt(&old_keys, &row.uuid, row.kind, data)?;
                    Ok(Some(crypto::encrypt(
                        new_keys, &row.uuid, row.kind, &plain,
                    )?))
                }
                None => Ok(None),
            }
        };
        let new_cipher = reencrypt(&row.cipher)
            .map_err(|e| anyhow::anyhow!("Eintrag {} nicht umschlüsselbar: {e}", row.uuid))?;
        let new_thumb = reencrypt(&row.thumb)
            .map_err(|e| anyhow::anyhow!("Thumbnail {} nicht umschlüsselbar: {e}", row.uuid))?;
        db::update_cipher(&tx, &row.uuid, new_cipher.as_deref(), new_thumb.as_deref())?;
    }
    tx.commit()?;
    Ok(())
}

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}
