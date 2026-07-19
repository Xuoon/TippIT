use std::sync::atomic::Ordering;

use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};
use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetForegroundWindow, IsWindowVisible, ShowWindow, SystemParametersInfoW,
    GA_ROOTOWNER, SPI_GETWORKAREA, SW_HIDE, SW_SHOWNOACTIVATE, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
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

const HISTORY_SIZE: (f64, f64) = (440.0, 660.0);

// WICHTIG: Sichtbarkeit läuft komplett über Win32 (ShowWindow/IsWindowVisible),
// nicht über Tauris show()/hide()/is_visible(): das Fenster wird ohne Aktivierung
// per SW_SHOWNOACTIVATE gezeigt, wovon Tauris interner Visible-Zustand nichts
// mitbekommt — dessen hide() würde als No-op verpuffen (Fenster wäre "stuck").

fn history_hwnd(app: &AppHandle) -> Option<HWND> {
    let w = app.get_webview_window("history")?;
    w.hwnd().ok().map(|h| HWND(h.0))
}

pub fn toggle_history(app: &AppHandle) {
    let visible = history_hwnd(app)
        .map(|hwnd| unsafe { IsWindowVisible(hwnd) }.as_bool())
        .unwrap_or(false);
    if visible {
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

    // Unten rechts im Arbeitsbereich (Parität zur AutoIt-Version).
    let wa = work_area();
    let sf = window.scale_factor().unwrap_or(1.0);
    let (w_px, h_px) = (HISTORY_SIZE.0 * sf, HISTORY_SIZE.1 * sf);
    let x = wa.right as f64 - w_px - 10.0 * sf;
    let y = wa.bottom as f64 - h_px - 10.0 * sf;
    let _ = window.set_position(PhysicalPosition::new(x as i32, y as i32));
    // Ohne Aktivierung zeigen: der Fokus bleibt beim bisherigen Fenster
    // (z. B. Windows-Suche oder das Textfeld, in das getippt werden soll).
    match window.hwnd() {
        Ok(hwnd) => {
            let _ = unsafe { ShowWindow(HWND(hwnd.0), SW_SHOWNOACTIVATE) };
        }
        Err(e) => {
            tracing::warn!("HWND nicht ermittelbar ({e}), zeige mit Aktivierung");
            let _ = window.show();
        }
    }
    let _ = window.emit("history-shown", ());
    tray::set_history_checked(app, true);
}

fn create_history_window(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, "history", WebviewUrl::App("history".into()))
        .title("TippIT")
        .inner_size(HISTORY_SIZE.0, HISTORY_SIZE.1)
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
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    match WebviewWindowBuilder::new(app, "settings", WebviewUrl::App("settings".into()))
        .title("TippIT – Einstellungen")
        .inner_size(720.0, 560.0)
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
