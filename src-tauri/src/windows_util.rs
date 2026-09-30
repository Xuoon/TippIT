//! Verwaltung der App-Fenster (Historie, Einstellungen, Update-Hinweis) —
//! plattformneutral; Sichtbarkeit/Aktivierung/Arbeitsbereich liefert `platform`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError, TryLockError};
use std::time::Duration;

use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};

use crate::platform;
use crate::state::AppState;
use crate::storage::settings::HistoryScreen;
use crate::tray;

const HISTORY_SIZE: (f64, f64) = (940.0, 600.0);
const UPDATE_SIZE: (f64, f64) = (380.0, 176.0);
const SETTINGS_SIZE: (f64, f64) = (720.0, 560.0);

/// Historie-Basisgröße skaliert mit dem Setting `history.window_scale`
/// (Prozent, 100 = Standard; Grenzen setzt der Slider in den Einstellungen).
fn history_size(app: &AppHandle) -> (f64, f64) {
    let scale = {
        let state = app.state::<AppState>();
        let s = state.settings.read().unwrap();
        f64::from(s.history.window_scale) / 100.0
    };
    (HISTORY_SIZE.0 * scale, HISTORY_SIZE.1 * scale)
}

/// Monitor, auf dem die Historie laut Setting `history.window_screen` öffnet.
/// Ein nicht angeschlossener gewählter Monitor fällt auf den unter dem Mauszeiger zurück.
fn history_monitor(app: &AppHandle) -> Option<tauri::Monitor> {
    let screen = app
        .state::<AppState>()
        .settings
        .read()
        .unwrap()
        .history
        .window_screen
        .clone();
    let primary = || app.primary_monitor().ok().flatten();
    let cursor = || platform::monitor_under_cursor(app);
    match screen {
        HistoryScreen::Cursor => cursor().or_else(primary),
        HistoryScreen::Primary => primary().or_else(cursor),
        HistoryScreen::Monitor(id) => app
            .available_monitors()
            .ok()
            .and_then(|monitors| {
                monitors
                    .into_iter()
                    .find(|m| platform::monitor_id(m).is_some_and(|m_id| m_id == id))
            })
            .or_else(cursor)
            .or_else(primary),
    }
}

/// Fenster unten rechts im Arbeitsbereich platzieren (Größe in logischen
/// Pixeln, Rand in logischen Pixeln — beides wird mit dem Scale-Faktor skaliert).
fn position_bottom_right(window: &tauri::WebviewWindow, size: (f64, f64), margin: f64) {
    let (_, _, right, bottom) = platform::work_area(window);
    let sf = window.scale_factor().unwrap_or(1.0);
    let x = right - (size.0 + margin) * sf;
    let y = bottom - (size.1 + margin) * sf;
    let _ = window.set_position(PhysicalPosition::new(x as i32, y as i32));
}

fn show_and_focus(window: &tauri::WebviewWindow) {
    let _ = window.show();
    let _ = window.set_focus();
}

/// Sichtbarkeit des Historie-Fensters (Plattform-Wahrheit, nicht Tauris
/// interner Zustand — s. platform::window_visible).
pub fn history_visible(app: &AppHandle) -> bool {
    app.get_webview_window("history")
        .map(|w| platform::window_visible(&w))
        .unwrap_or(false)
}

pub fn toggle_history(app: &AppHandle) {
    if history_visible(app) {
        hide_history(app);
    } else {
        show_history(app);
    }
}

/// Zählt jedes Öffnen der Historie. Ein verzögerter Fokusverlust-Check eines
/// früheren Öffnens darf das neu geöffnete Fenster nicht verstecken.
static HISTORY_SHOWN: AtomicU64 = AtomicU64::new(0);

pub fn hide_history(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("history") {
        platform::hide_window(&w);
    }
    tray::set_history_checked(app, false);
}

/// Fokusverlust der Historie. Mit `close_on_blur` versteckt sie sich nur, wenn
/// eine fremde App vorn ist; ein eigenes Fenster oder ein eigener Dialog (z. B.
/// „Bild speichern") lässt sie offen. Ohne die Option holt sie den Fokus zurück.
fn on_history_blur(app: AppHandle) {
    let close = app
        .state::<AppState>()
        .settings
        .read()
        .unwrap()
        .history
        .close_on_blur;
    if !close {
        refocus_history_after_pointer_release(app);
        return;
    }
    let shown = HISTORY_SHOWN.load(Ordering::SeqCst);
    std::thread::spawn(move || {
        // Der Fokuswechsel braucht einen Moment, bis das neue Vordergrundfenster
        // feststeht.
        std::thread::sleep(Duration::from_millis(100));
        if HISTORY_SHOWN.load(Ordering::SeqCst) != shown || !history_visible(&app) {
            return;
        }
        // Ein Tippvorgang versteckt die Historie selbst und aktiviert das Ziel.
        let typing = matches!(
            app.state::<AppState>().typing_lock.try_lock(),
            Err(TryLockError::WouldBlock)
        );
        if typing {
            return;
        }
        // Unbekannte Vordergrund-App (None): offen lassen, ein Erkennungsfehler
        // soll das Fenster nicht wegklicken.
        if platform::foreground_app_info(|_| false).is_some_and(|a| !a.is_self) {
            hide_history(&app);
        }
    });
}

/// Nach einem Hintergrundklick darf dessen Mouse-up noch im Ziel ankommen;
/// anschließend erhält die weiterhin sichtbare Historie den Tastaturfokus zurück.
fn refocus_history_after_pointer_release(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(15));
        while history_visible(&app) && platform::pointer_buttons_held() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        if history_visible(&app) {
            if let Some(window) = app.get_webview_window("history") {
                platform::show_window_activated(&window);
            }
        }
    });
}

pub fn show_history(app: &AppHandle) {
    // Das aktuell fokussierte Fenster merken — Ziel für „als Tastatur tippen".
    // Muss VOR dem Aktivieren der Historie passieren, sonst wäre die Historie
    // selbst das „vorherige" Fenster.
    let state = app.state::<AppState>();
    let prev = platform::current_foreground();
    // Name der Ziel-App für den Footer (vor dem Fokuswechsel).
    let foreground = platform::foreground_app_info(|_| false);
    // Liegt TippIT selbst vorn (z. B. die Einstellungen), gibt es kein Tipp-Ziel:
    // `spawn_type` verweigert dann mit Fehlerton, statt in die eigenen Fenster zu tippen.
    let is_self = foreground.as_ref().is_some_and(|a| a.is_self);
    state
        .prev_target
        .store(if is_self { 0 } else { prev }, Ordering::SeqCst);
    let target_app = foreground.filter(|a| !a.is_self).map(|a| (a.name, a.id));
    *state.prev_target_app.lock().unwrap() = target_app;

    let window = match app.get_webview_window("history") {
        Some(w) => w,
        None => match create_history_window(app) {
            Ok(w) => w,
            Err(e) => {
                tracing::error!("Historie-Fenster konnte nicht erstellt werden: {e}");
                return;
            }
        },
    };

    // Größe folgt dem Setting (kann sich seit dem letzten Öffnen geändert haben),
    // dann zentriert im Arbeitsbereich des gewählten Monitors (Spotlight-Stil).
    let size = history_size(app);
    match history_monitor(app) {
        Some(monitor) => platform::place_centered(&window, &monitor, size),
        None => {
            tracing::warn!("Kein Monitor ermittelbar, Historie wird mittig gesetzt");
            let _ = window.set_size(tauri::LogicalSize::new(size.0, size.1));
            let _ = window.center();
        }
    }
    HISTORY_SHOWN.fetch_add(1, Ordering::SeqCst);
    // MIT Aktivierung zeigen: Pfeiltasten/Sofort-Suche funktionieren direkt.
    // Das Tipp-Ziel ist davon unabhängig — prev_target wurde oben gemerkt und
    // type_entry aktiviert es vor dem Tippen wieder.
    platform::show_window_activated(&window);
    let _ = window.emit("history-shown", ());
    tray::set_history_checked(app, true);
}

/// Eigene Oberfläche: `tauri://localhost` (macOS), `http(s)://tauri.localhost`
/// (Windows) und im Debug-Build der Vite-Devserver.
fn is_app_url(url: &tauri::Url) -> bool {
    match url.scheme() {
        "tauri" => true,
        "http" | "https" => {
            let host = url.host_str();
            host == Some("tauri.localhost") || (cfg!(debug_assertions) && host == Some("localhost"))
        }
        _ => false,
    }
}

fn create_history_window(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    let size = history_size(app);
    // Fenster-Chrome je Plattform (`platform::TRANSPARENT_WINDOW`): macOS
    // transparent mit CSS-Radius (braucht Feature macos-private-api und
    // app.macOSPrivateApi), Windows opak mit nativen DWM-Ecken.
    let window = WebviewWindowBuilder::new(app, "history", WebviewUrl::App("history".into()))
        .title("TippIT")
        .inner_size(size.0, size.1)
        .decorations(false)
        .transparent(platform::TRANSPARENT_WINDOW)
        .shadow(true)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        // Die Vorschau rendert fremdes (sanitisiertes) HTML: ein Link darin darf
        // die WebView nicht auf eine fremde Seite navigieren.
        .on_navigation(is_app_url)
        .build()?;

    // macOS: Corner-Radius der Layer, damit die eckige OS-Hülle nicht neben
    // dem CSS-Radius „durchscheint". Windows 11: DWM rundet das Fenster.
    platform::round_window_corners(&window, 12.0);

    let app2 = app.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            hide_history(&app2);
        }
        WindowEvent::Focused(false) => on_history_blur(app2.clone()),
        _ => {}
    });
    Ok(window)
}

pub fn open_settings(app: &AppHandle) {
    // Solange die Einstellungen offen sind, regulärer App-Switcher-Eintrag
    // mit Icon (macOS-ActivationPolicy; Windows no-op).
    platform::set_app_switcher_visible(app, true);
    if let Some(w) = app.get_webview_window("settings") {
        show_and_focus(&w);
        return;
    }
    match WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings".into()))
        .title("TippIT – Einstellungen")
        .inner_size(SETTINGS_SIZE.0, SETTINGS_SIZE.1)
        // Feste Größe: der Inhalt scrollt, das Fenster nicht.
        .resizable(false)
        .maximizable(false)
        .visible(false)
        .build()
    {
        Ok(w) => {
            // Schließen versteckt nur (Fenster lebt weiter, s. o.) —
            // und macht die App wieder zur reinen Menüleisten-App.
            let w2 = w.clone();
            let app2 = app.clone();
            w.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = w2.hide();
                    platform::set_app_switcher_visible(&app2, false);
                }
            });
        }
        Err(e) => {
            tracing::error!("Einstellungs-Fenster konnte nicht erstellt werden: {e}");
            platform::set_app_switcher_visible(app, false);
        }
    }
}

/// Tab, den die Einstellungen beim nächsten Laden zeigen sollen.
static PENDING_SETTINGS_TAB: Mutex<Option<String>> = Mutex::new(None);

/// Einstellungen auf einem bestimmten Tab öffnen (z. B. „berechtigungen"). Ein
/// neues Fenster holt den Tab per `take_settings_tab`, ein schon geladenes
/// wechselt über das Event `settings-tab`.
pub fn open_settings_tab(app: &AppHandle, tab: &str) {
    *PENDING_SETTINGS_TAB
        .lock()
        .unwrap_or_else(PoisonError::into_inner) = Some(tab.to_owned());
    open_settings(app);
    let _ = app.emit_to("settings", "settings-tab", tab);
}

/// Liefert den vorgemerkten Tab genau einmal.
#[tauri::command]
pub fn take_settings_tab() -> Option<String> {
    PENDING_SETTINGS_TAB
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
}

#[derive(serde::Serialize)]
pub struct MonitorInfo {
    /// Kennung für das Setting (`platform::monitor_id`), nicht zum Anzeigen.
    id: String,
    label: String,
    primary: bool,
    /// Auflösung in physischen Pixeln.
    width: u32,
    height: u32,
}

/// Angeschlossene Monitore für die Auswahl in den Einstellungen. Monitore ohne
/// Kennung fehlen: ein Setting könnte sie nicht wiederfinden.
#[tauri::command]
pub fn list_monitors(app: AppHandle) -> Vec<MonitorInfo> {
    let primary = app
        .primary_monitor()
        .ok()
        .flatten()
        .and_then(|m| platform::monitor_id(&m));
    app.available_monitors()
        .unwrap_or_default()
        .iter()
        .filter_map(|m| {
            let id = platform::monitor_id(m)?;
            Some(MonitorInfo {
                primary: primary.as_ref() == Some(&id),
                label: platform::monitor_label(m),
                width: m.size().width,
                height: m.size().height,
                id,
            })
        })
        .collect()
}

#[tauri::command]
pub fn settings_window_ready(app: AppHandle) {
    if let Some(window) = app.get_webview_window("settings") {
        show_and_focus(&window);
    }
}

pub fn show_update_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("update") {
        // Fenster existiert bereits (z. B. Ready-Aufruf ging verloren):
        // erneut platzieren und zeigen statt unsichtbar hängen zu lassen.
        position_bottom_right(&window, UPDATE_SIZE, 14.0);
        let _ = window.show();
        return;
    }
    match WebviewWindowBuilder::new(app, "update", WebviewUrl::App("update".into()))
        .title("TippIT-Update")
        .inner_size(UPDATE_SIZE.0, UPDATE_SIZE.1)
        .decorations(false)
        .transparent(platform::TRANSPARENT_WINDOW)
        .shadow(true)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .build()
    {
        Ok(window) => platform::round_window_corners(&window, 12.0),
        Err(e) => tracing::warn!("Update-Hinweis konnte nicht erstellt werden: {e}"),
    }
}

#[tauri::command]
pub fn update_window_ready(app: AppHandle) {
    let Some(window) = app.get_webview_window("update") else {
        return;
    };
    position_bottom_right(&window, UPDATE_SIZE, 14.0);
    let _ = window.show();
}

#[tauri::command]
pub fn close_update_window(app: AppHandle) {
    if let Some(window) = app.get_webview_window("update") {
        let _ = window.destroy();
    }
}
