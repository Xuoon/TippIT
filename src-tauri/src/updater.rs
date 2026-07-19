use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_updater::{Update, UpdaterExt};

#[derive(Default)]
pub struct PendingUpdate(Mutex<Option<Update>>);

#[derive(Clone, Serialize)]
pub struct UpdateMetadata {
    pub version: String,
}

fn metadata(update: &Update) -> UpdateMetadata {
    UpdateMetadata {
        version: update.version.clone(),
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

#[tauri::command]
pub async fn install_update(state: State<'_, PendingUpdate>) -> Result<(), String> {
    // Clone statt take: schlägt der Download fehl, bleibt das Update für
    // einen erneuten Versuch verfügbar (auch aus dem jeweils anderen Fenster).
    let update = state
        .0
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| "Kein Update verfügbar".to_string())?;
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    *state.0.lock().unwrap() = None;
    Ok(())
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
