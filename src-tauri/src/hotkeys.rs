use std::sync::atomic::Ordering;

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::state::AppState;
use crate::{typing, windows_util};

pub fn parse(s: &str) -> Option<Shortcut> {
    // Settings führen das getippte Zeichen; die Plattform-Schicht übersetzt es
    // in die physische Schreibweise des Plugins (macOS layoutbewusst via
    // UCKeyTranslate, Windows Identität — RegisterHotKey ist layoutbewusst).
    match crate::platform::resolve_hotkey(s).parse::<Shortcut>() {
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

/// Fester Abbruch fürs Tippen: immer ESC — aber nur WÄHREND eines Tipp-Vorgangs
/// global registriert (`typing::EscCancelGuard`); dauerhaft würde der Shortcut
/// systemweit jede ESC-Taste schlucken.
fn esc() -> Shortcut {
    "escape".parse().expect("escape ist parsebar")
}

pub fn register_typing_esc(app: &AppHandle) {
    register(app, esc(), "Abbruch (ESC)");
}

pub fn unregister_typing_esc(app: &AppHandle) {
    let _ = app.global_shortcut().unregister(esc());
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
    if *pressed == esc() {
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
