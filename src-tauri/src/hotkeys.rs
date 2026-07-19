use std::sync::atomic::Ordering;

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::state::AppState;
use crate::{typing, windows_util};

pub fn parse(s: &str) -> Option<Shortcut> {
    match s.parse::<Shortcut>() {
        Ok(sc) => Some(sc),
        Err(e) => {
            tracing::error!("Hotkey '{s}' nicht parsebar: {e}");
            None
        }
    }
}

fn current(app: &AppHandle) -> (Option<Shortcut>, Option<Shortcut>) {
    let state = app.state::<AppState>();
    let s = state.settings.read().unwrap();
    (parse(&s.hotkeys.paste), parse(&s.hotkeys.history))
}

/// Konfigurierbarer Abbruch-Hotkey (Default `ctrl+alt+escape`): bricht einen
/// laufenden Tipp-Vorgang ab. NICHT `ctrl+shift+escape` — das ist von Windows für
/// den Task-Manager reserviert und `RegisterHotKey` scheitert dort mit Fehler 1409.
fn cancel_shortcut(app: &AppHandle) -> Option<Shortcut> {
    let state = app.state::<AppState>();
    let s = state.settings.read().unwrap();
    parse(&s.hotkeys.cancel)
}

/// Registriert alle Hotkeys gemäß Settings; der Einfügen-Hotkey nur, wenn nicht pausiert.
pub fn register_all(app: &AppHandle) {
    let state = app.state::<AppState>();
    let (paste, history) = current(app);
    if !state.paused.load(Ordering::SeqCst) {
        if let Some(sc) = paste {
            register(app, sc, "Einfügen");
        }
    }
    if let Some(sc) = history {
        register(app, sc, "Historie");
    }
    if let Some(sc) = cancel_shortcut(app) {
        register(app, sc, "Abbruch");
    }
}

pub fn register_paste(app: &AppHandle) {
    if let (Some(sc), _) = current(app) {
        register(app, sc, "Einfügen");
    }
}

pub fn unregister_paste(app: &AppHandle) {
    if let (Some(sc), _) = current(app) {
        let _ = app.global_shortcut().unregister(sc);
    }
}

/// Für Settings-Änderungen: alles neu registrieren.
pub fn reregister_all(app: &AppHandle) {
    let _ = app.global_shortcut().unregister_all();
    register_all(app);
}

fn register(app: &AppHandle, sc: Shortcut, label: &str) {
    if let Err(e) = app.global_shortcut().register(sc) {
        tracing::error!("Hotkey-Registrierung '{label}' fehlgeschlagen (Konflikt?): {e}");
    }
}

/// Wird vom global-shortcut-Handler bei Tastendruck aufgerufen.
pub fn handle(app: &AppHandle, pressed: &Shortcut) {
    let (paste, history) = current(app);
    if cancel_shortcut(app).as_ref() == Some(pressed) {
        typing::cancel(app);
    } else if paste.as_ref() == Some(pressed) {
        typing::paste_clipboard(app);
    } else if history.as_ref() == Some(pressed) {
        // Parität: während der Pause ist auch der Historie-Hotkey wirkungslos.
        let state = app.state::<AppState>();
        if state.paused.load(Ordering::SeqCst) {
            return;
        }
        windows_util::toggle_history(app);
    }
}
