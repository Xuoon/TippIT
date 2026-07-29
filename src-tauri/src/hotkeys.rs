use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::state::AppState;
use crate::{typing, windows_util};

const DUPLICATE_WINDOW: Duration = Duration::from_millis(150);
static LAST_HANDLED: OnceLock<Mutex<HashMap<u32, Instant>>> = OnceLock::new();

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

/// Windows-Fallback für Programme wie TeamViewer, die Tastenkombinationen vor
/// `WM_HOTKEY` für eine Remote-Sitzung abfangen. Der physische Tastenzustand
/// bleibt trotzdem global lesbar. Flankenerkennung + `handle`-Dedupe verhindern
/// Wiederholungen und Doppel-Auslösungen mit dem normalen Plugin-Handler.
#[cfg(target_os = "windows")]
pub fn start_focus_independent_listener(app: AppHandle) {
    std::thread::spawn(move || {
        let mut was_down = HashMap::<u32, bool>::new();
        loop {
            let state = app.state::<AppState>();
            let paused = state.paused.load(Ordering::SeqCst);
            let (paste, history) = current(&app);
            let mut shortcuts = Vec::with_capacity(3);
            if !paused {
                if let Some(sc) = paste {
                    shortcuts.push(sc);
                }
                if let Some(sc) = history {
                    if !shortcuts.contains(&sc) {
                        shortcuts.push(sc);
                    }
                }
            }
            shortcuts.push(esc());

            for shortcut in &shortcuts {
                let down = crate::platform::shortcut_pressed(shortcut);
                let previous = was_down.insert(shortcut.id(), down).unwrap_or(false);
                if down && !previous {
                    handle(&app, shortcut);
                }
            }
            was_down.retain(|id, _| shortcuts.iter().any(|shortcut| shortcut.id() == *id));
            std::thread::sleep(Duration::from_millis(12));
        }
    });
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
    let now = Instant::now();
    let mut last = LAST_HANDLED
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap();
    if last
        .get(&pressed.id())
        .is_some_and(|previous| now.duration_since(*previous) < DUPLICATE_WINDOW)
    {
        return;
    }
    last.insert(pressed.id(), now);
    drop(last);

    let (paste, history) = current(app);
    if *pressed == esc() {
        typing::cancel(app);
    } else if paste.as_ref() == Some(pressed) {
        if app.state::<AppState>().paused.load(Ordering::SeqCst) {
            return;
        }
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
