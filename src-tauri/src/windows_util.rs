//! Verwaltung der App-Fenster (Historie, Einstellungen, Update-Hinweis) —
//! plattformneutral; Sichtbarkeit/Aktivierung/Arbeitsbereich liefert `platform`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError, TryLockError};
use std::time::{Duration, Instant};

use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};

use crate::platform::{self, Frame};
use crate::state::AppState;
use crate::storage::settings::{HistoryScreen, WindowPosition};

/// Seitenverhältnis (und Größe ohne Monitordaten) sowie Mindestgröße der
/// Historie in logischen Pixeln.
const HISTORY_BASE: (f64, f64) = (940.0, 600.0);
const HISTORY_MIN: (f64, f64) = (720.0, 460.0);
const UPDATE_SIZE: (f64, f64) = (380.0, 176.0);
const SETTINGS_SIZE: (f64, f64) = (720.0, 560.0);

/// Rahmen der Historie im Arbeitsbereich `area`: Höhe als Anteil
/// `size_percent`, Breite im Seitenverhältnis, mindestens `HISTORY_MIN`,
/// höchstens der Arbeitsbereich. `anchor` ist die Lage im freien Raum (0..1),
/// ohne Angabe mittig.
fn history_frame(area: Frame, size_percent: u32, anchor: Option<(f64, f64)>) -> Frame {
    let h = area.h * f64::from(size_percent) / 100.0;
    let w = h * HISTORY_BASE.0 / HISTORY_BASE.1;
    let (w, h) = (
        w.max(HISTORY_MIN.0).min(area.w),
        h.max(HISTORY_MIN.1).min(area.h),
    );
    let (fx, fy) = anchor.unwrap_or((0.5, 0.5));
    Frame {
        x: area.x + fx * (area.w - w),
        y: area.y + fy * (area.h - h),
        w,
        h,
    }
}

/// Umkehrung von `history_frame`: Lage eines Rahmens im freien Raum von `area`,
/// auf vier Stellen gerundet.
fn anchor_of(area: Frame, frame: Frame) -> (f64, f64) {
    let axis = |pos: f64, start: f64, free: f64| {
        if free < 1.0 {
            return 0.5;
        }
        (((pos - start) / free).clamp(0.0, 1.0) * 10_000.0).round() / 10_000.0
    };
    (
        axis(frame.x, area.x, area.w - frame.w),
        axis(frame.y, area.y, area.h - frame.h),
    )
}

fn same_frame(a: Frame, b: Frame) -> bool {
    [(a.x, b.x), (a.y, b.y), (a.w, b.w), (a.h, b.h)]
        .iter()
        .all(|(p, q)| (p - q).abs() < 2.0)
}

fn find_monitor(app: &AppHandle, id: &str) -> Option<tauri::Monitor> {
    app.available_monitors()
        .ok()?
        .into_iter()
        .find(|m| platform::monitor_id(m).is_some_and(|m_id| m_id == id))
}

/// Monitor und Lage der Historie: die verschobene Position, solange ihr Monitor
/// angeschlossen ist, sonst die Regel aus `history.window_screen`. Ein nicht
/// angeschlossener gewählter Monitor fällt auf den unter dem Mauszeiger zurück.
fn history_target(
    app: &AppHandle,
    screen: HistoryScreen,
    position: Option<WindowPosition>,
) -> Option<(tauri::Monitor, Option<(f64, f64)>)> {
    if let Some(p) = position {
        if let Some(monitor) = find_monitor(app, &p.monitor) {
            return Some((monitor, Some((p.x, p.y))));
        }
    }
    let primary = || app.primary_monitor().ok().flatten();
    let cursor = || platform::monitor_under_cursor(app);
    let monitor = match screen {
        HistoryScreen::Cursor => cursor().or_else(primary),
        HistoryScreen::Primary => primary().or_else(cursor),
        HistoryScreen::Monitor(id) => find_monitor(app, &id).or_else(cursor).or_else(primary),
    }?;
    Some((monitor, None))
}

/// Zuletzt selbst gesetzter Rahmen der Historie. Daran erkennt `Moved`, ob der
/// Nutzer verschoben hat, und ScaleFactorChanged, ob tao die Größe verdorben hat.
struct Placement {
    monitor: tauri::Monitor,
    frame: Frame,
    at: Instant,
}

static PLACEMENT: Mutex<Option<Placement>> = Mutex::new(None);

fn placement() -> std::sync::MutexGuard<'static, Option<Placement>> {
    PLACEMENT.lock().unwrap_or_else(PoisonError::into_inner)
}

fn place_history(window: &tauri::WebviewWindow, monitor: tauri::Monitor, frame: Frame) {
    platform::place_window(window, &monitor, frame);
    *placement() = Some(Placement {
        monitor,
        frame,
        at: Instant::now(),
    });
}

/// Liegt die Historie nicht auf dem zuletzt gesetzten Rahmen, erneut setzen.
/// Nötig, wenn die DPI-Meldung des Zielmonitors erst nach dem Platzieren kommt
/// (verstecktes Fenster): tao skaliert die schon richtige Größe dann nochmals um,
/// und das Fenster öffnet riesig.
fn correct_placement(window: &tauri::WebviewWindow) {
    let Some((monitor, frame)) = placement().as_ref().map(|p| (p.monitor.clone(), p.frame)) else {
        return;
    };
    if platform::window_frame(window, &monitor).is_some_and(|f| !same_frame(f, frame)) {
        platform::place_window(window, &monitor, frame);
    }
}

/// Skalierungswechsel kurz nach dem Platzieren stammen vom Monitorwechsel beim
/// Öffnen; später zieht der Nutzer das Fenster selbst über Monitore.
const PLACEMENT_SETTLE: Duration = Duration::from_millis(1500);

fn on_history_scale_changed(app: &AppHandle, window: &tauri::WebviewWindow) {
    let placed = placement()
        .as_ref()
        .filter(|p| p.at.elapsed() < PLACEMENT_SETTLE)
        .and_then(|p| platform::monitor_id(&p.monitor));
    let Some(placed) = placed else {
        return;
    };
    // Liegt das Fenster schon auf einem anderen Monitor, hat der Nutzer es
    // gezogen; zurückzusetzen würde seine Verschiebung verwerfen.
    let on_placed = window
        .current_monitor()
        .ok()
        .flatten()
        .and_then(|m| platform::monitor_id(&m))
        .is_some_and(|id| id == placed);
    if !on_placed {
        return;
    }
    // Erst nach tao: das setzt die umgerechnete Größe nach dem Event-Callback.
    let window = window.clone();
    let _ = app.run_on_main_thread(move || correct_placement(&window));
}

/// Ende der letzten Verschiebung; ein laufender Wartethread verlängert nur die Frist.
static MOVE_DEADLINE: Mutex<Option<Instant>> = Mutex::new(None);
const MOVE_SETTLE: Duration = Duration::from_millis(400);

fn schedule_position_save(app: AppHandle) {
    {
        let mut deadline = MOVE_DEADLINE.lock().unwrap_or_else(PoisonError::into_inner);
        let running = deadline.is_some();
        *deadline = Some(Instant::now() + MOVE_SETTLE);
        if running {
            return;
        }
    }
    std::thread::spawn(move || {
        loop {
            let wait = {
                let mut deadline = MOVE_DEADLINE.lock().unwrap_or_else(PoisonError::into_inner);
                let now = Instant::now();
                match *deadline {
                    Some(t) if t > now => t - now,
                    _ => {
                        *deadline = None;
                        break;
                    }
                }
            };
            std::thread::sleep(wait);
        }
        remember_position(&app);
    });
}

/// Hat der Nutzer die Historie verschoben? Nur gegenüber einem selbst gesetzten
/// Rahmen erkennbar: ohne Platzierung (mittig ohne Monitordaten) nie, sonst würde
/// aus „keine Position" still eine.
fn moved_by_user(placed: Option<Frame>, current: Frame) -> bool {
    placed.is_some_and(|p| !same_frame(p, current))
}

/// Vom Nutzer verschobene Historie als Position merken (Setting
/// `history.window_position`). Läuft nach dem Verschieben und vor jedem
/// Verstecken: ein Schließen kurz nach dem Loslassen käme der Entprellung sonst
/// zuvor, versteckt verwirft `remember_position` den Stand.
fn remember_position(app: &AppHandle) {
    let Some(window) = app.get_webview_window("history") else {
        return;
    };
    if !platform::window_visible(&window) {
        return;
    }
    // Eigenes Platzieren (Öffnen, DPI-Korrektur) ist kein Verschieben. Den Guard
    // vor den Fenster-Gettern fallen lassen: die warten hier auf den Main-Thread,
    // der PLACEMENT selbst nimmt.
    let Some((placed_monitor, placed_frame)) =
        placement().as_ref().map(|p| (p.monitor.clone(), p.frame))
    else {
        return;
    };
    let Some(current) = platform::window_frame(&window, &placed_monitor) else {
        return;
    };
    if !moved_by_user(Some(placed_frame), current) {
        return;
    }
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let (Some(id), Some(frame)) = (
        platform::monitor_id(&monitor),
        platform::window_frame(&window, &monitor),
    ) else {
        tracing::warn!("Position der Historie nicht ermittelbar, nicht gespeichert");
        return;
    };
    let (x, y) = anchor_of(Frame::work_area(&monitor), frame);
    let position = WindowPosition { monitor: id, x, y };
    if let Err(e) = crate::history::set_history_position(app, Some(position)) {
        tracing::warn!("Position der Historie nicht gespeichert: {e}");
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
        remember_position(app);
        platform::hide_window(&w);
        // Ob die WebView ein per `hide_window` verstecktes Fenster als
        // `document.hidden` sieht, ist je Plattform offen; das Event ist eindeutig.
        let _ = w.emit("history-hidden", ());
    }
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
    // Ohne Ziel (`prev == 0`, z. B. die Taskleiste nach einem Tray-Klick unter
    // Windows) zeigt der Footer auch keinen App-Namen.
    let is_self = foreground.as_ref().is_some_and(|a| a.is_self);
    let no_target = is_self || prev == 0;
    state
        .prev_target
        .store(if no_target { 0 } else { prev }, Ordering::SeqCst);
    let target_app = foreground.filter(|_| !no_target).map(|a| (a.name, a.id));
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

    // Größe und Lage folgen den Settings (können sich seit dem letzten Öffnen
    // geändert haben) und dem Arbeitsbereich des Zielmonitors: ohne verschobene
    // Position mittig (Spotlight-Stil).
    let (screen, position, size) = {
        let s = state.settings.read().unwrap();
        (
            s.history.window_screen.clone(),
            s.history.window_position.clone(),
            s.history.window_size,
        )
    };
    match history_target(app, screen, position) {
        Some((monitor, anchor)) => {
            let frame = history_frame(Frame::work_area(&monitor), size, anchor);
            place_history(&window, monitor, frame);
        }
        None => {
            tracing::warn!("Kein Monitor ermittelbar, Historie wird mittig gesetzt");
            *placement() = None;
            let _ = window.set_size(tauri::LogicalSize::new(HISTORY_BASE.0, HISTORY_BASE.1));
            let _ = window.center();
        }
    }
    HISTORY_SHOWN.fetch_add(1, Ordering::SeqCst);
    // MIT Aktivierung zeigen: Pfeiltasten/Sofort-Suche funktionieren direkt.
    // Das Tipp-Ziel ist davon unabhängig — prev_target wurde oben gemerkt und
    // type_entry aktiviert es vor dem Tippen wieder.
    platform::show_window_activated(&window);
    correct_placement(&window);
    let _ = window.emit("history-shown", ());
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
    // Fenster-Chrome je Plattform (`platform::TRANSPARENT_WINDOW`): macOS
    // transparent mit CSS-Radius (braucht Feature macos-private-api und
    // app.macOSPrivateApi), Windows opak mit nativen DWM-Ecken.
    let window = WebviewWindowBuilder::new(app, "history", WebviewUrl::App("history".into()))
        .title("TippIT")
        .inner_size(HISTORY_BASE.0, HISTORY_BASE.1)
        .decorations(false)
        .transparent(platform::TRANSPARENT_WINDOW)
        .shadow(true)
        .resizable(false)
        // Doppelklick auf die Zieh-Fläche soll nichts maximieren.
        .maximizable(false)
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
    let window2 = window.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            hide_history(&app2);
        }
        WindowEvent::Focused(false) => on_history_blur(app2.clone()),
        WindowEvent::Moved(_) => schedule_position_save(app2.clone()),
        WindowEvent::ScaleFactorChanged { .. } => on_history_scale_changed(&app2, &window2),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn area(w: f64, h: f64) -> Frame {
        Frame {
            x: 0.0,
            y: 0.0,
            w,
            h,
        }
    }

    #[test]
    fn size_is_relative_to_work_area() {
        // 1080p bei 100 %: etwa die frühere feste Größe 940 × 600.
        let f = history_frame(area(1920.0, 1040.0), 58, None);
        assert!((f.h - 603.2).abs() < 0.1 && (f.w - 945.0).abs() < 1.0);
        // 4K bei 200 % hat denselben logischen Arbeitsbereich: gleiche Wirkung.
        assert_eq!(history_frame(area(1920.0, 1040.0), 58, None), f);
        // Mittig ohne Anker.
        assert!((f.x - (1920.0 - f.w) / 2.0).abs() < 1e-9);
    }

    #[test]
    fn size_respects_minimum_and_work_area() {
        // 1366 × 768 bei 125 %: Mindestgröße greift.
        let f = history_frame(area(1092.8, 574.4), 40, None);
        assert_eq!((f.w, f.h), (HISTORY_MIN.0, HISTORY_MIN.1));
        // Winziger Arbeitsbereich gewinnt gegen die Mindestgröße.
        let f = history_frame(area(640.0, 400.0), 90, None);
        assert_eq!((f.w, f.h, f.x, f.y), (640.0, 400.0, 0.0, 0.0));
        // Hochkant: Breite auf den Arbeitsbereich begrenzt.
        let f = history_frame(area(1080.0, 1880.0), 90, None);
        assert_eq!(f.w, 1080.0);
    }

    #[test]
    fn anchor_round_trips_across_resolutions() {
        let small = Frame {
            x: 100.0,
            y: 25.0,
            w: 1920.0,
            h: 1040.0,
        };
        let placed = history_frame(small, 58, Some((0.25, 1.0)));
        let anchor = anchor_of(small, placed);
        assert_eq!(anchor, (0.25, 1.0));
        // Anderer Monitor: gleiche relative Lage, vollständig im Arbeitsbereich.
        let big = area(2560.0, 1415.0);
        let moved = history_frame(big, 58, Some(anchor));
        assert!((moved.y + moved.h - 1415.0).abs() < 1e-9);
        assert!(moved.x >= 0.0 && moved.x + moved.w <= 2560.0);
    }

    #[test]
    fn anchor_is_clamped_and_centered_without_room() {
        let a = area(1000.0, 600.0);
        let off = Frame {
            x: -300.0,
            y: 900.0,
            w: 800.0,
            h: 600.0,
        };
        assert_eq!(anchor_of(a, off), (0.0, 0.5));
    }

    #[test]
    fn move_is_detected_against_own_placement() {
        let area = area(1920.0, 1040.0);
        let placed = history_frame(area, 58, None);
        // Eigenes Platzieren, Rundung von tao: kein Verschieben.
        let rounded = Frame {
            x: placed.x.round(),
            y: placed.y.round() + 1.0,
            ..placed
        };
        assert!(!moved_by_user(Some(placed), rounded));
        // Ohne eigene Platzierung wird nie gespeichert.
        assert!(!moved_by_user(None, rounded));
        // Gezogen: gespeichert, und beim nächsten Öffnen liegt sie wieder dort.
        let dragged = Frame {
            x: 120.0,
            y: 40.0,
            ..placed
        };
        assert!(moved_by_user(Some(placed), dragged));
        let reopened = history_frame(area, 58, Some(anchor_of(area, dragged)));
        assert!(same_frame(reopened, dragged));
    }
}
