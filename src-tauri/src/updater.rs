use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

/// Fortschritts-Event an Update-Fenster UND Einstellungen (beide zeigen den
/// Verlauf). Name muss mit `UPDATE_PROGRESS` in `src/lib/api.ts` übereinstimmen.
const PROGRESS_EVENT: &str = "update://progress";
/// Melde-Abstand, solange keine Gesamtgröße bekannt ist (siehe `install_update`).
const PROGRESS_STEP_BYTES: u64 = 256 * 1024;

#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<Update>>);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateMetadata {
    pub version: String,
    pub current_version: String,
}

/// Phase des laufenden Vorgangs. `installing` heißt unter Windows: der Installer
/// übernimmt und beendet TippIT gleich selbst — ein `done` kommt dort nie an.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    pub phase: &'static str,
    pub downloaded: u64,
    /// `None`, solange der Server keine Content-Length geliefert hat.
    pub total: Option<u64>,
}

fn metadata(update: &Update) -> UpdateMetadata {
    UpdateMetadata {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
    }
}

async fn fetch(app: &AppHandle) -> Result<Option<UpdateMetadata>, String> {
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    let result = update.as_ref().map(metadata);
    *app.state::<PendingUpdate>().0.lock().unwrap() = update;
    Ok(result)
}

#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<Option<UpdateMetadata>, String> {
    fetch(&app).await
}

#[tauri::command]
pub fn pending_update(state: State<'_, PendingUpdate>) -> Option<UpdateMetadata> {
    state.0.lock().unwrap().as_ref().map(metadata)
}

fn emit(app: &AppHandle, phase: &'static str, downloaded: u64, total: Option<u64>) {
    let _ = app.emit(
        PROGRESS_EVENT,
        UpdateProgress {
            phase,
            downloaded,
            total,
        },
    );
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    // Clone statt take: schlägt der Download fehl, bleibt das Update für
    // einen erneuten Versuch verfügbar (auch aus dem jeweils anderen Fenster).
    let update = app
        .state::<PendingUpdate>()
        .0
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "Kein Update verfügbar".to_string())?;

    emit(&app, "downloading", 0, None);
    let chunk_app = app.clone();
    let done_app = app.clone();
    let mut downloaded: u64 = 0;
    // Der Chunk-Callback feuert im KB-Takt — ohne Drosselung überschwemmen die
    // Events das Frontend. Ein Event pro angefangenem Prozent (und immer das
    // erste) reicht für einen flüssigen Balken.
    let mut last_percent = u64::MAX;
    // Ohne Content-Length gibt es keinen Prozentwert, an dem sich drosseln
    // ließe — dann zählt die übertragene Menge, sonst bliebe es bei genau
    // einem Event für den ganzen Download.
    let mut last_bytes = 0u64;

    let result = update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;
                let report = match total.filter(|t| *t > 0) {
                    Some(t) => {
                        let percent = downloaded.saturating_mul(100) / t;
                        let changed = percent != last_percent;
                        last_percent = percent;
                        changed
                    }
                    None => {
                        let changed = downloaded - last_bytes >= PROGRESS_STEP_BYTES;
                        if changed {
                            last_bytes = downloaded;
                        }
                        changed
                    }
                };
                if report {
                    emit(&chunk_app, "downloading", downloaded, total);
                }
            },
            move || emit(&done_app, "installing", 0, None),
        )
        .await;

    if let Err(e) = result {
        let message = e.to_string();
        emit(&app, "error", 0, None);
        return Err(message);
    }
    *app.state::<PendingUpdate>().0.lock().unwrap() = None;
    emit(&app, "done", 0, None);
    Ok(())
}

/// Nach einem Update auf macOS: die ausgetauschte App neu starten. Unter
/// Windows übernimmt das der Installer, dort wird der Befehl nie erreicht.
#[tauri::command]
pub fn restart_app(app: AppHandle) {
    app.restart();
}

pub fn check_on_start(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        match fetch(&app).await {
            Ok(Some(update)) => {
                tracing::info!("Update {} verfügbar", update.version);
                crate::windows_util::show_update_window(&app);
            }
            Ok(None) => tracing::info!("Kein Update verfügbar"),
            Err(e) => tracing::warn!("Update-Prüfung fehlgeschlagen: {e}"),
        }
    });
}
