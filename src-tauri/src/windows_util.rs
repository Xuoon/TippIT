use std::sync::atomic::Ordering;

use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetForegroundWindow, IsWindowVisible, SetForegroundWindow, ShowWindow,
    SystemParametersInfoW, GA_ROOTOWNER, SPI_GETWORKAREA, SW_HIDE, SW_SHOW,
    SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

use crate::state::AppState;
use crate::tray;

/// Arbeitsbereich des primären Monitors (ohne Taskbar), in physischen Pixeln.
pub fn work_area() -> RECT {
    let mut rect = RECT::default();
    let _ = unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut rect as *mut _ as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    rect
}

/// Warten, bis das Zielfenster wirklich im Vordergrund ist (Poll-Muster wie
/// `typing::wait_modifiers_released`). Der GA_ROOTOWNER-Vergleich lässt legitime
/// Fälle durch, in denen das Foreground-HWND ein Dialog/Frame desselben Ziels ist
/// (UWP-ApplicationFrame, owned Dialoge).
pub fn wait_foreground(target: HWND, timeout: std::time::Duration) -> bool {
    let start = std::time::Instant::now();
    loop {
        let fg = unsafe { GetForegroundWindow() };
        if fg == target
            || unsafe { GetAncestor(fg, GA_ROOTOWNER) == GetAncestor(target, GA_ROOTOWNER) }
        {
            return true;
        }
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
}

const HISTORY_SIZE: (f64, f64) = (940.0, 600.0);
const UPDATE_SIZE: (f64, f64) = (360.0, 138.0);

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

/// Fenster mittig im Arbeitsbereich platzieren (Größe in logischen Pixeln).
fn position_center(window: &tauri::WebviewWindow, size: (f64, f64)) {
    let wa = work_area();
    let sf = window.scale_factor().unwrap_or(1.0);
    let w = (wa.right - wa.left) as f64;
    let h = (wa.bottom - wa.top) as f64;
    // Klemmung auf die linke/obere Kante des Arbeitsbereichs: bei hoher DPI-Skalierung
    // kann size * sf größer als der Arbeitsbereich werden. Ohne Klemmung liefe das
    // Fenster oben/links heraus und Suchzeile + Liste wären unerreichbar; mit Klemmung
    // wird im Extremfall nur die Statusleiste unten abgeschnitten.
    let x = (wa.left as f64 + (w - size.0 * sf) / 2.0).max(wa.left as f64);
    let y = (wa.top as f64 + (h - size.1 * sf) / 2.0).max(wa.top as f64);
    let _ = window.set_position(PhysicalPosition::new(x as i32, y as i32));
}

/// Fenster unten rechts im Arbeitsbereich platzieren (Größe in logischen
/// Pixeln, Rand in logischen Pixeln — beides wird mit dem Scale-Faktor skaliert).
fn position_bottom_right(window: &tauri::WebviewWindow, size: (f64, f64), margin: f64) {
    let wa = work_area();
    let sf = window.scale_factor().unwrap_or(1.0);
    let x = wa.right as f64 - (size.0 + margin) * sf;
    let y = wa.bottom as f64 - (size.1 + margin) * sf;
    let _ = window.set_position(PhysicalPosition::new(x as i32, y as i32));
}

fn show_and_focus(window: &tauri::WebviewWindow) {
    let _ = window.show();
    let _ = window.set_focus();
}

// WICHTIG: Sichtbarkeit läuft komplett über Win32 (ShowWindow/IsWindowVisible),
// nicht über Tauris show()/hide()/is_visible(): das Fenster wird ohne Aktivierung
// per SW_SHOWNOACTIVATE gezeigt, wovon Tauris interner Visible-Zustand nichts
// mitbekommt — dessen hide() würde als No-op verpuffen (Fenster wäre "stuck").

fn history_hwnd(app: &AppHandle) -> Option<HWND> {
    let w = app.get_webview_window("history")?;
    w.hwnd().ok().map(|h| HWND(h.0))
}

/// Sichtbarkeit des Historie-Fensters (Win32-Stand, s. Kommentar unten).
pub fn history_visible(app: &AppHandle) -> bool {
    history_hwnd(app)
        .map(|hwnd| unsafe { IsWindowVisible(hwnd) }.as_bool())
        .unwrap_or(false)
}

pub fn toggle_history(app: &AppHandle) {
    if history_visible(app) {
        hide_history(app);
    } else {
        show_history(app);
    }
}

pub fn hide_history(app: &AppHandle) {
    if let Some(hwnd) = history_hwnd(app) {
        let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
    } else if let Some(w) = app.get_webview_window("history") {
        let _ = w.hide();
    }
    tray::set_history_checked(app, false);
}

pub fn show_history(app: &AppHandle) {
    // Das aktuell fokussierte Fenster merken — Ziel für „als Tastatur tippen".
    // Muss VOR dem Aktivieren der Historie passieren, sonst wäre die Historie
    // selbst das „vorherige" Fenster.
    let prev = unsafe { GetForegroundWindow() };
    app.state::<AppState>()
        .prev_hwnd
        .store(prev.0 as isize, Ordering::SeqCst);

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
    // dann zentriert im Arbeitsbereich (Spotlight-/ClipBook-Stil).
    let size = history_size(app);
    let _ = window.set_size(tauri::LogicalSize::new(size.0, size.1));
    position_center(&window, size);
    // MIT Aktivierung zeigen: Pfeiltasten/Sofort-Suche funktionieren direkt.
    // Das Tipp-Ziel ist davon unabhängig — prev_hwnd wurde oben gemerkt und
    // type_entry holt es per SetForegroundWindow zurück.
    match window.hwnd() {
        Ok(hwnd) => {
            let hwnd = HWND(hwnd.0);
            let _ = unsafe { ShowWindow(hwnd, SW_SHOW) };
            // Aus dem Hotkey-Kontext heraus haben wir Foreground-Rechte.
            let _ = unsafe { SetForegroundWindow(hwnd) };
        }
        Err(e) => {
            tracing::warn!("HWND nicht ermittelbar ({e}), zeige über Tauri");
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
    let _ = window.emit("history-shown", ());
    tray::set_history_checked(app, true);
}

fn create_history_window(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    let size = history_size(app);
    let window = WebviewWindowBuilder::new(app, "history", WebviewUrl::App("history".into()))
        .title("TippIT")
        .inner_size(size.0, size.1)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .build()?;

    let app2 = app.clone();
    window.on_window_event(move |event| {
        // Bewusst KEIN Hide bei Fokusverlust: die Historie bleibt offen, bis
        // X oder der Hotkey sie schließt. Nie zerstören (WebView2-Neuaufbau ist teuer).
        if let WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            hide_history(&app2);
        }
    });
    Ok(window)
}

pub fn open_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        show_and_focus(&w);
        return;
    }
    match WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings".into()))
        .title("TippIT – Einstellungen")
        .inner_size(860.0, 720.0)
        // Feste Größe: der Inhalt scrollt, das Fenster nicht.
        .resizable(false)
        .maximizable(false)
        .visible(false)
        .build()
    {
        Ok(w) => {
            // Schließen versteckt nur (Fenster lebt weiter, s. o.).
            let w2 = w.clone();
            w.on_window_event(move |event| {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = w2.hide();
                }
            });
        }
        Err(e) => tracing::error!("Einstellungs-Fenster konnte nicht erstellt werden: {e}"),
    }
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
    if let Err(e) = WebviewWindowBuilder::new(app, "update", WebviewUrl::App("update".into()))
        .title("TippIT-Update")
        .inner_size(UPDATE_SIZE.0, UPDATE_SIZE.1)
        .decorations(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .build()
    {
        tracing::warn!("Update-Hinweis konnte nicht erstellt werden: {e}");
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
