pub mod convex;
pub mod pairing;
pub mod policy;
pub mod protocol;

use std::sync::atomic::Ordering;
use std::time::Duration;

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;
use crate::storage::db::{self, EntryRow};
use crate::storage::{crypto, paths::AppPaths};

pub use self::convex::SyncClient;
use protocol::{is_newer, SyncEntry, SETTINGS_UUID};

/// Server-Limit für Inline-Ciphertexte (muss zu convex/sync.ts passen).
const MAX_INLINE_CIPHER: usize = 900 * 1024;
const MAX_PUSH_BATCH_BYTES: usize = 8 * 1024 * 1024;
const PUSH_BATCH: u32 = 20;

#[derive(Serialize, Deserialize)]
pub struct SyncState {
    pub group_id: String,
    #[serde(default)]
    pub watermark: i64,
}

impl SyncState {
    pub fn load(paths: &AppPaths) -> Option<Self> {
        let raw = std::fs::read_to_string(paths.sync_file()).ok()?;
        serde_json::from_str(&raw).ok()
    }

    pub fn save(&self, paths: &AppPaths) -> anyhow::Result<()> {
        let tmp = paths.sync_file().with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, paths.sync_file())?;
        Ok(())
    }

    pub fn remove(paths: &AppPaths) {
        let _ = std::fs::remove_file(paths.sync_file());
    }
}

pub fn is_configured(app: &AppHandle) -> bool {
    let state = app.state::<AppState>();
    let url_set = !state
        .settings
        .read()
        .unwrap()
        .sync
        .deployment_url
        .trim()
        .is_empty();
    url_set && SyncState::load(&state.paths).is_some()
}

/// (Neu-)Start der Sync-Loop. Ein Generation-Bump beendet die alte Loop.
pub fn restart(app: &AppHandle) {
    let state = app.state::<AppState>();
    let generation = state.sync_gen.fetch_add(1, Ordering::SeqCst) + 1;
    if !is_configured(app) {
        *state.push_notify.lock().unwrap() = None;
        return;
    }
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    *state.push_notify.lock().unwrap() = Some(tx);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        run_loop(app, generation, rx).await;
    });
}

/// Live-Auswertung der Windows-Richtlinien gegen den aktuellen Settings-Stand.
fn sync_allowed(app: &AppHandle) -> bool {
    policy::block_reason(&app.state::<AppState>().settings.read().unwrap().sync).is_none()
}

async fn run_loop(
    app: AppHandle,
    generation: u64,
    mut push_rx: tokio::sync::mpsc::UnboundedReceiver<()>,
) {
    let mut backoff = Duration::from_secs(1);
    loop {
        if stale(&app, generation) {
            return;
        }
        if !sync_allowed(&app) {
            tokio::time::sleep(Duration::from_secs(30)).await;
            continue;
        }
        match run_session(&app, generation, &mut push_rx).await {
            Ok(()) => return, // Generation gewechselt → sauber beendet
            Err(e) => {
                tracing::warn!("Sync-Session abgebrochen: {e} — Retry in {backoff:?}");
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(Duration::from_secs(60));
            }
        }
    }
}

fn stale(app: &AppHandle, generation: u64) -> bool {
    app.state::<AppState>().sync_gen.load(Ordering::SeqCst) != generation
}

async fn run_session(
    app: &AppHandle,
    generation: u64,
    push_rx: &mut tokio::sync::mpsc::UnboundedReceiver<()>,
) -> anyhow::Result<()> {
    let (url, group_id, auth_key) = {
        let state = app.state::<AppState>();
        let _rotation = state.rotation_lock.lock().unwrap();
        let url = state.settings.read().unwrap().sync.deployment_url.clone();
        let keys = state.keys.read().unwrap();
        (url, keys.group_id.clone(), keys.auth)
    };
    let mut client = SyncClient::connect(&url, &group_id, &auth_key).await?;
    tracing::info!("Sync verbunden ({group_id})");

    // Gerät für die Geräteliste melden — best-effort: ein Server ohne die
    // devices-Funktionen (älteres Deployment) darf die Session nicht stoppen.
    announce_device(app, &mut client).await;
    let mut last_announce = std::time::Instant::now();

    // Initial: erst pushen (lokale Änderungen), dann pullen.
    push_dirty(app, generation, &mut client).await?;
    pull_new(app, generation, &mut client).await?;

    let interval_minutes = app
        .state::<AppState>()
        .settings
        .read()
        .unwrap()
        .sync
        .interval_minutes;
    let realtime = interval_minutes == 0;
    // Im Intervall-Modus keine Subscription: ihr Änderungssignal würde ohnehin
    // verworfen (der Tick pullt deterministisch), kostet den Server aber
    // Query-Re-Läufe und Streaming bei jedem Gruppen-Write.
    let mut subscription = if realtime {
        Some(client.subscribe_latest().await?)
    } else {
        None
    };
    let period = if realtime {
        Duration::from_secs(60)
    } else {
        Duration::from_secs(interval_minutes.clamp(1, 1440) * 60)
    };
    let mut tick = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
    let policy_period = Duration::from_secs(30);
    let mut policy_tick =
        tokio::time::interval_at(tokio::time::Instant::now() + policy_period, policy_period);

    loop {
        if stale(app, generation) {
            return Ok(());
        }
        tokio::select! {
            update = next_update(&mut subscription) => {
                match update {
                    Some(result) => {
                        let latest = convex::latest_from_result(&result)?;
                        let watermark = current_watermark(app);
                        if latest > watermark {
                            if !sync_allowed(app) {
                                anyhow::bail!("Sync durch Systemrichtlinie pausiert");
                            }
                            // Subscription hält die Verbindung; Pull separat.
                            pull_new(app, generation, &mut client).await?;
                        }
                    }
                    None => anyhow::bail!("Subscription beendet"),
                }
            }
            notified = push_rx.recv() => {
                if notified.is_none() {
                    return Ok(()); // Kanal geschlossen → Loop neu gestartet
                }
                // Debounce: kurz sammeln, dann in einem Batch pushen.
                tokio::time::sleep(Duration::from_millis(500)).await;
                while push_rx.try_recv().is_ok() {}
                if realtime && sync_allowed(app) {
                    push_dirty(app, generation, &mut client).await?;
                }
            }
            _ = tick.tick() => {
                if sync_allowed(app) {
                    // Der Tick ist zugleich Intervallsteuerung und Sicherheitsnetz:
                    // auch im Realtime-Modus pullen, falls eine Subscription-Notification
                    // verloren ging oder ohne Stream-Abbruch fehlerhaft war.
                    push_dirty(app, generation, &mut client).await?;
                    pull_new(app, generation, &mut client).await?;
                    // „Zuletzt aktiv" der Geräteliste frisch halten — sparsam,
                    // damit lange Realtime-Sessions nicht minütlich schreiben.
                    if last_announce.elapsed() >= Duration::from_secs(30 * 60) {
                        announce_device(app, &mut client).await;
                        last_announce = std::time::Instant::now();
                    }
                }
            }
            _ = policy_tick.tick() => {
                if !sync_allowed(app) {
                    // Verbindung/Subscription schließen, sobald eine Geräte-Richtlinie
                    // greift; der äußere Loop verbindet nach Freigabe erneut.
                    anyhow::bail!("Sync durch Systemrichtlinie pausiert");
                }
            }
        }
    }
}

/// Nächstes Subscription-Ergebnis; ohne Subscription (Intervall-Modus) wartet
/// die Future endlos, sodass die anderen select!-Zweige den Takt vorgeben.
async fn next_update(
    subscription: &mut Option<::convex::QuerySubscription>,
) -> Option<::convex::FunctionResult> {
    match subscription {
        Some(sub) => sub.next().await,
        None => std::future::pending().await,
    }
}

fn current_watermark(app: &AppHandle) -> i64 {
    let state = app.state::<AppState>();
    SyncState::load(&state.paths)
        .map(|s| s.watermark)
        .unwrap_or(0)
}

async fn push_dirty(
    app: &AppHandle,
    generation: u64,
    client: &mut SyncClient,
) -> anyhow::Result<()> {
    loop {
        let state = app.state::<AppState>();
        let rotation = state.rotation_lock.lock().unwrap();
        // Rotation kann zwischen dem äußeren Check und dem Lock begonnen haben.
        if stale(app, generation) {
            return Ok(());
        }
        let batch = {
            let db = state.db.lock().unwrap();
            db::list_dirty(&db, PUSH_BATCH)?
        };

        let mut entries: Vec<SyncEntry> = Vec::new();
        let mut push_bytes = 0usize;
        let mut synced_local_only: Vec<(String, i64)> = Vec::new();
        let scope = state.settings.read().unwrap().sync.clone();
        for row in &batch {
            if in_scope(row, &scope) {
                let row_bytes = row.cipher.as_ref().map_or(0, Vec::len)
                    + row.thumb.as_ref().map_or(0, Vec::len);
                if !entries.is_empty()
                    && push_bytes.saturating_add(row_bytes) > MAX_PUSH_BATCH_BYTES
                {
                    break;
                }
                push_bytes = push_bytes.saturating_add(row_bytes);
                entries.push(SyncEntry::from_row(row));
            } else {
                synced_local_only.push((row.uuid.clone(), row.lamport));
            }
        }

        // Out-of-scope-Zeilen als erledigt markieren, damit sie den Batch nicht blockieren.
        if !synced_local_only.is_empty() {
            let db = state.db.lock().unwrap();
            for (uuid, lamport) in &synced_local_only {
                db::mark_synced(&db, uuid, *lamport)?;
            }
        }

        // Netzwerk-I/O hält die Rotationsbarriere nicht; der Snapshot ist aber
        // vollständig mit Client-Gruppe und altem Key konsistent.
        drop(rotation);

        if entries.is_empty() {
            if batch.is_empty() {
                return Ok(());
            }
            continue;
        }

        let max_lamport = client.push(&entries).await?;
        // Nach einer Schlüsselrotation dürfen keine Alt-Zustände mehr markiert werden.
        if stale(app, generation) {
            return Ok(());
        }
        {
            let db = state.db.lock().unwrap();
            for e in &entries {
                db::mark_synced(&db, &e.uuid, e.lamport)?;
            }
            db::bump_lamport_to(&db, max_lamport)?;
        }
    }
}

/// Plattform-Kennung für die Geräteliste.
#[cfg(target_os = "macos")]
const PLATFORM: &str = "macos";
#[cfg(target_os = "windows")]
const PLATFORM: &str = "windows";

/// Gerätename fürs UI (Hostname, gekürzt) — rein kosmetisch, nie Auth-relevant.
fn device_display_name() -> String {
    let mut name = hostname::get()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_default();
    if name.trim().is_empty() {
        name = "Unbekanntes Gerät".into();
    }
    name.chars().take(64).collect()
}

async fn announce_device(app: &AppHandle, client: &mut SyncClient) {
    let device_id = app.state::<AppState>().device_id.clone();
    if let Err(e) = client
        .announce_device(&device_id, &device_display_name(), PLATFORM)
        .await
    {
        tracing::warn!("Geräte-Anmeldung übersprungen (Server ohne devices?): {e}");
    }
}

fn in_scope(row: &EntryRow, scope: &crate::storage::settings::SyncSettings) -> bool {
    use crate::storage::db::{KIND_FILES, KIND_IMAGE, KIND_TEXT};
    if row.deleted {
        return true; // Tombstones immer propagieren
    }
    let cipher_len = row.cipher.as_ref().map(|c| c.len()).unwrap_or(0)
        + row.thumb.as_ref().map(|t| t.len()).unwrap_or(0);
    if cipher_len > MAX_INLINE_CIPHER {
        return false;
    }
    match row.kind {
        KIND_TEXT | KIND_FILES => scope.sync_text,
        KIND_IMAGE => scope.sync_images && row.size_bytes as u64 <= scope.image_max_bytes,
        _ => false,
    }
}

async fn pull_new(app: &AppHandle, generation: u64, client: &mut SyncClient) -> anyhow::Result<()> {
    let device_id = app.state::<AppState>().device_id.clone();
    loop {
        if stale(app, generation) {
            return Ok(());
        }
        let since = current_watermark(app);
        let page = client.pull_since(since, &device_id).await?;
        if page.max_seq <= since {
            return Ok(());
        }
        // Page-Merge, Lamport und Cursor bilden zusammen einen Snapshot unter
        // derselben Rotationsbarriere. Sonst könnte ein alter Pull seinen Cursor
        // nach einer Gruppen-/Schlüsselrotation in den neuen SyncState schreiben.
        let state = app.state::<AppState>();
        let rotation = state.rotation_lock.lock().unwrap();
        if stale(app, generation) {
            return Ok(());
        }
        let mut max_lamport = 0i64;
        let mut changed = false;
        for remote in &page.entries {
            max_lamport = max_lamport.max(remote.lamport);
            if merge_remote(app, remote)? {
                changed = true;
            }
        }
        let mut sync_state = SyncState::load(&state.paths).unwrap_or(SyncState {
            group_id: state.keys.read().unwrap().group_id.clone(),
            watermark: 0,
        });
        sync_state.watermark = page.max_seq;
        sync_state.save(&state.paths)?;
        // Eigene Lamport-Uhr über alles Gesehene heben (LWW-Kausalität) und
        // das geräte-lokale History-Limit auch nach Remote-Materialisierung anwenden.
        let max_entries = state.settings.read().unwrap().history.max_entries;
        let db = state.db.lock().unwrap();
        db::bump_lamport_to(&db, max_lamport)?;
        let pruned = db::prune(&db, max_entries)?;
        drop(db);
        if !pruned.is_empty() {
            let mut index = state.index.write().unwrap();
            for uuid in pruned {
                index.remove(&uuid);
            }
            changed = true;
        }
        drop(rotation);
        if changed {
            let _ = app.emit("history-changed", ());
        }
        if !page.has_more {
            return Ok(());
        }
    }
}

/// Remote-Eintrag lokal übernehmen, wenn er nach LWW gewinnt.
/// Aufrufer hält `rotation_lock` über die komplette Pull-Page.
fn merge_remote(app: &AppHandle, remote: &SyncEntry) -> anyhow::Result<bool> {
    let state = app.state::<AppState>();

    // Settings werden nicht mehr synchronisiert — Settings-Dokumente älterer
    // Clients (SETTINGS_UUID) trotzdem überspringen, statt sie als Historie-
    // Eintrag zu materialisieren.
    if remote.uuid == SETTINGS_UUID {
        return Ok(false);
    }
    // Eigene Push-Echos filtert der LWW-Vergleich (gleiches lamport+device = nicht neuer).

    let db = state.db.lock().unwrap();
    if let Some(local) = db::get(&db, &remote.uuid)? {
        if !is_newer(
            remote.lamport,
            &remote.device_id,
            local.lamport,
            &local.device_id,
        ) {
            return Ok(false);
        }
    } else if remote.deleted {
        return Ok(false);
    }

    let plain_len = remote
        .cipher
        .as_ref()
        .map(|c| c.len().saturating_sub(28))
        .unwrap_or(0);
    let keys = state.keys.read().unwrap();
    // Hash lokal aus dem entschlüsselten Inhalt rekonstruieren (für Dedupe).
    let hash = match &remote.cipher {
        Some(c) => {
            let plain = crypto::decrypt(&keys, &remote.uuid, remote.kind, c).map_err(|e| {
                anyhow::anyhow!("Remote-Eintrag {} nicht entschlüsselbar: {e}", remote.uuid)
            })?;
            crypto::sha256(&plain).to_vec()
        }
        None => vec![0u8; 32],
    };
    let row = EntryRow {
        uuid: remote.uuid.clone(),
        kind: remote.kind,
        cipher: remote.cipher.clone(),
        thumb: remote.thumb.clone(),
        size_bytes: plain_len as i64,
        hash,
        created_at: remote.created_at,
        pinned: remote.pinned,
        deleted: remote.deleted,
        device_id: remote.device_id.clone(),
        lamport: remote.lamport,
        // Source-App ist geräte-lokal — Remote-Materialisierung lässt Spalten leer.
        // Normale Updates bewahren lokale Source, Tombstones entfernen sie.
        source_app_id: None,
        source_app_name: None,
        first_created_at: remote.created_at,
        copy_count: 1,
    };
    db::upsert_remote(&db, &row)?;
    // UPDATE bewahrt geräte-lokale Source-/Sortiermetadaten in SQLite; deshalb
    // genau die materialisierte Zeile in den In-Memory-Index übernehmen.
    let merged = db::get(&db, &row.uuid)?.ok_or_else(|| anyhow::anyhow!("Remote-Upsert fehlt"))?;
    state.index.write().unwrap().upsert(&merged, &keys);
    Ok(true)
}
