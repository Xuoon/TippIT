//! Eingabe-Berechtigung (macOS-Bedienungshilfen) und Installationsdiagnose für
//! den Einstellungs-Tab „Berechtigungen". Den Systemdialog löst nur eine
//! Nutzeraktion aus, nie der Start oder ein Tippversuch. URLs und Pfade stehen
//! im Backend fest; das Frontend übergibt keine.

use std::sync::OnceLock;

use tauri::AppHandle;

use crate::platform;

static SIGNATURE: OnceLock<String> = OnceLock::new();

#[derive(serde::Serialize)]
pub struct PermissionStatus {
    /// false unter Windows: dort gibt es keine Freigabe und keinen Tab.
    supported: bool,
    input_trusted: bool,
    /// "applications" | "translocated" | "dmg" | "downloads" | "other"
    location: &'static str,
    bundle_path: String,
    signature: String,
    bundle_id: String,
    /// Zwischenablage-Zugriff ab macOS 15.4: "default" | "ask" | "allow" | "deny",
    /// sonst null.
    clipboard_access: Option<&'static str>,
}

/// Async, weil die Signatur-Diagnose beim ersten Aufruf `codesign` startet.
/// Danach kommt sie aus dem Cache: Der Tab fragt im Sekundentakt ab, solange
/// die Freigabe fehlt, und die Signatur ändert sich zur Laufzeit nicht.
#[tauri::command(async)]
pub fn permission_status(app: AppHandle) -> PermissionStatus {
    let supported = cfg!(target_os = "macos");
    let install = platform::install_info();
    PermissionStatus {
        supported,
        input_trusted: platform::input_permission_granted(),
        location: install.location.as_str(),
        signature: SIGNATURE
            .get_or_init(|| platform::signature_info(&install.bundle_path))
            .clone(),
        bundle_path: install.bundle_path,
        bundle_id: if supported {
            app.config().identifier.clone()
        } else {
            String::new()
        },
        clipboard_access: platform::clipboard_access(),
    }
}

/// Systemdialog auslösen; liefert den aktuellen Status. Fehlt die Freigabe,
/// wird vorher der Eintrag entfernt: Ein Eintrag aus einer anderen Signatur
/// (ad-hoc-signierte Versionen vor dem Wechsel auf die Developer ID, lokale
/// Builds) steht in den Systemeinstellungen auf „An“, greift aber nicht, und
/// macOS fragt erst ohne ihn neu. Der Dialog kommt auch, wenn das Entfernen
/// scheitert; der Fehler geht für den Terminal-Hinweis ans Frontend.
#[tauri::command(async)]
pub fn request_input_permission(app: AppHandle) -> Result<bool, String> {
    let reset = if platform::input_permission_granted() {
        Ok(())
    } else {
        platform::reset_input_permission(&app.config().identifier)
    };
    let trusted = platform::request_input_permission();
    reset.map(|()| trusted).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_permission_settings() -> Result<(), String> {
    platform::open_input_permission_settings().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn open_clipboard_settings() -> Result<(), String> {
    platform::open_clipboard_permission_settings().map_err(|e| e.to_string())
}
