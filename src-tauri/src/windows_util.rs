//! Verwaltung der App-Fenster (Historie, Einstellungen, Update-Hinweis) —
//! plattformneutral; Sichtbarkeit/Aktivierung/Arbeitsbereich liefert `platform`.

use std::sync::atomic::Ordering;

use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};

use crate::platform;
use crate::state::AppState;
use crate::tray;

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
    let (left, top, right, bottom) = platform::work_area(window);
    let sf = window.scale_factor().unwrap_or(1.0);
    let w = right - left;
    let h = bottom - top;
    // Klemmung auf die linke/obere Kante des Arbeitsbereichs: bei hoher DPI-Skalierung
    // kann size * sf größer als der Arbeitsbereich werden. Ohne Klemmung liefe das
    // Fenster oben/links heraus und Suchzeile + Liste wären unerreichbar; mit Klemmung
    // wird im Extremfall nur die Statusleiste unten abgeschnitten.
    let x = (left + (w - size.0 * sf) / 2.0).max(left);
    let y = (top + (h - size.1 * sf) / 2.0).max(top);
    let _ = window.set_position(PhysicalPosition::new(x as i32, y as i32));
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

pub fn hide_history(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("history") {
        platform::hide_window(&w);
    }
    tray::set_history_checked(app, false);
}

pub fn show_history(app: &AppHandle) {
    // Das aktuell fokussierte Fenster merken — Ziel für „als Tastatur tippen".
    // Muss VOR dem Aktivieren der Historie passieren, sonst wäre die Historie
    // selbst das „vorherige" Fenster.
    let state = app.state::<AppState>();
    let prev = platform::current_foreground();
    state.prev_target.store(prev, Ordering::SeqCst);
    // Name der Ziel-App für den Footer (vor dem Fokuswechsel).
    let target_app = platform::foreground_app_info().and_then(|a| {
        if a.is_self {
            None
        } else {
            Some((a.name, a.id))
        }
    });
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
    // dann zentriert im Arbeitsbereich (Spotlight-/ClipBook-Stil).
    let size = history_size(app);
    let _ = window.set_size(tauri::LogicalSize::new(size.0, size.1));
    position_center(&window, size);
    // MIT Aktivierung zeigen: Pfeiltasten/Sofort-Suche funktionieren direkt.
    // Das Tipp-Ziel ist davon unabhängig — prev_target wurde oben gemerkt und
    // type_entry aktiviert es vor dem Tippen wieder.
    platform::show_window_activated(&window);
    let _ = window.emit("history-shown", ());
    tray::set_history_checked(app, true);
}

fn create_history_window(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    let size = history_size(app);
    // Transparent + CSS border-radius = ClipBook-artige Floating-Rundung.
    // macOS: tauri feature macos-private-api + app.macOSPrivateApi in conf.
    let window = WebviewWindowBuilder::new(app, "history", WebviewUrl::App("history".into()))
        .title("TippIT")
        .inner_size(size.0, size.1)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .build()?;

    // macOS: native Corner-Radius der Layer, damit die eckige OS-Hülle
    // nicht neben dem CSS-Radius „durchscheint".
    platform::round_window_corners(&window, 20.0);

    let app2 = app.clone();
    window.on_window_event(move |event| {
        // Bewusst KEIN Hide bei Fokusverlust: die Historie bleibt offen, bis
        // X oder der Hotkey sie schließt. Nie zerstören (WebView-Neuaufbau ist teuer).
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
    match WebviewWindowBuilder::new(app, "update", WebviewUrl::App("update".into()))
        .title("TippIT-Update")
        .inner_size(UPDATE_SIZE.0, UPDATE_SIZE.1)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .build()
    {
        Ok(window) => platform::round_window_corners(&window, 20.0),
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
