//! Mini-Palette: Modifier + Linksklick öffnet am Mauszeiger die neuesten
//! Einträge. Ausgelöst wird sie von einem globalen Maus-Hook in `platform`
//! (Experiment, s. AGENTS.md); Fenster und Auswahl sind plattformneutral.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use crate::platform::{self, ClickTarget, Frame, ScreenPoint};
use crate::state::AppState;
use crate::storage::db::KIND_IMAGE;
use crate::{history, typing, windows_util};

const LABEL: &str = "palette";
/// Größe vor dem ersten Messen; danach bestimmt der Inhalt (`palette_ready`).
const INITIAL_SIZE: (f64, f64) = (340.0, 220.0);
/// Abstand der linken oberen Ecke zum Mauszeiger.
const CURSOR_GAP: f64 = 4.0;

/// Mauszeiger beim Auslösen, bis das Frontend seine Größe meldet.
struct Anchor {
    monitor: tauri::Monitor,
    x: f64,
    y: f64,
}

static PENDING: Mutex<Option<Anchor>> = Mutex::new(None);
/// Zählt jedes Zeigen; ein verzögerter Blur-Check eines früheren Zeigens darf
/// die neu geöffnete Palette nicht verstecken.
static SHOWN: AtomicU64 = AtomicU64::new(0);
/// Zählt jedes Auslösen (unter `PENDING` erhöht); ein Blur-Check verwirft nur
/// eine Öffnung, die schon vor dem Fokusverlust vorgemerkt war.
static OPENED: AtomicU64 = AtomicU64::new(0);
static CLICK_TX: OnceLock<Sender<ClickTarget>> = OnceLock::new();
/// Tipp-Ziel der Palette (0 = keins) und die Klickstelle darin. Eigenes Ziel
/// statt `prev_target`, damit die offene Historie ihr gemerktes Ziel behält.
static TARGET: Mutex<(isize, Option<ScreenPoint>)> = Mutex::new((0, None));
static APPLY: Mutex<()> = Mutex::new(());
static WAITING_FOR_PERMISSION: AtomicBool = AtomicBool::new(false);

/// Maus-Hook nach `settings.palette` an- oder abschalten. Idempotent: beim
/// Start und nach jeder Änderung der Palette-Einstellungen.
pub fn apply(app: &AppHandle) {
    let _guard = APPLY.lock().unwrap_or_else(PoisonError::into_inner);
    let cfg = app
        .state::<AppState>()
        .settings
        .read()
        .unwrap()
        .palette
        .clone();
    let tx = click_sender(app);
    if !cfg.enabled {
        platform::set_click_trigger(None, tx);
        hide(app);
        return;
    }
    // macOS: ohne Bedienungshilfen kein Tap. Nicht nachfragen, nur warten, bis
    // der Nutzer die Freigabe über die Einstellungen erteilt hat.
    if !platform::input_permission_granted() {
        wait_for_permission(app);
        return;
    }
    if !platform::set_click_trigger(Some(cfg.modifier), tx) {
        tracing::warn!("Maus-Hook der Palette nicht aktiv");
    }
}

fn wait_for_permission(app: &AppHandle) {
    if WAITING_FOR_PERMISSION.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(3));
            let enabled = app
                .state::<AppState>()
                .settings
                .read()
                .unwrap()
                .palette
                .enabled;
            if !enabled || platform::input_permission_granted() {
                break;
            }
        }
        WAITING_FOR_PERMISSION.store(false, Ordering::SeqCst);
        // Auf dem Main-Thread: `apply` hält `APPLY` über Fensteraufrufe, die
        // von hier aus auf den Main-Thread warten; `set_settings` läuft dort
        // und wartet auf `APPLY`.
        let app2 = app.clone();
        let _ = app.run_on_main_thread(move || apply(&app2));
    });
}

/// Kanal vom Hook-Thread zur App: jeder Auslöser öffnet die Palette auf dem
/// Main-Thread.
fn click_sender(app: &AppHandle) -> &'static Sender<ClickTarget> {
    CLICK_TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<ClickTarget>();
        let app = app.clone();
        std::thread::spawn(move || {
            for target in rx {
                let app2 = app.clone();
                let _ = app.run_on_main_thread(move || open(&app2, target));
            }
        });
        tx
    })
}

/// Palette am Mauszeiger vorbereiten. Sichtbar wird sie erst, wenn das
/// Frontend die Einträge geladen und seine Größe gemeldet hat (`palette_ready`).
fn open(app: &AppHandle, click: ClickTarget) {
    // Der Klick wurde verschluckt, die angeklickte App ist also nicht vorn:
    // Ziel ist sie, nicht das Vordergrundfenster. Die Aktivierung allein
    // fokussiert nur deren Fenster, deshalb wird die Klickstelle vor dem
    // Einfügen erneut angeklickt. In der Historie gilt deren Ziel.
    let target = match click {
        ClickTarget::App(target, point) => (target, Some(point)),
        ClickTarget::Own if windows_util::history_visible(app) => (
            app.state::<AppState>().prev_target.load(Ordering::SeqCst),
            None,
        ),
        ClickTarget::Own | ClickTarget::Unknown => (0, None),
    };
    *TARGET.lock().unwrap_or_else(PoisonError::into_inner) = target;
    let Some((monitor, x, y)) = platform::cursor_point(app) else {
        tracing::warn!("Mauszeiger nicht ermittelbar, Palette bleibt zu");
        return;
    };
    {
        let mut pending = PENDING.lock().unwrap_or_else(PoisonError::into_inner);
        OPENED.fetch_add(1, Ordering::SeqCst);
        *pending = Some(Anchor { monitor, x, y });
    }
    if app.get_webview_window(LABEL).is_some() {
        let _ = app.emit_to(LABEL, "palette-open", ());
    } else if let Err(e) = create(app) {
        tracing::error!("Palette-Fenster konnte nicht erstellt werden: {e}");
    }
}

fn create(app: &AppHandle) -> tauri::Result<tauri::WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("palette".into()))
        .title("TippIT")
        .inner_size(INITIAL_SIZE.0, INITIAL_SIZE.1)
        .decorations(false)
        .transparent(platform::TRANSPARENT_WINDOW)
        .shadow(true)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .on_navigation(windows_util::is_app_url)
        .build()?;
    platform::round_window_corners(&window, 10.0);
    let app2 = app.clone();
    window.on_window_event(move |event| match event {
        // Nie zerstören, nur verstecken (wie die Historie).
        WindowEvent::CloseRequested { api, .. } => {
            api.prevent_close();
            hide(&app2);
        }
        WindowEvent::Focused(false) => on_blur(app2.clone()),
        _ => {}
    });
    Ok(window)
}

/// Klick außerhalb schließt, auch eine gerade ladende erneute Öffnung. Kurz
/// warten: beim Aktivieren kann ein Fokusverlust durchrutschen, der nicht vom
/// Nutzer kommt.
fn on_blur(app: AppHandle) {
    let shown = SHOWN.load(Ordering::SeqCst);
    let opened = OPENED.load(Ordering::SeqCst);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        if SHOWN.load(Ordering::SeqCst) != shown {
            return;
        }
        let focused = app
            .get_webview_window(LABEL)
            .is_some_and(|w| w.is_focused().unwrap_or(false));
        if focused {
            return;
        }
        {
            // Ein erst nach dem Fokusverlust ausgelöstes Öffnen bleibt stehen.
            let mut pending = PENDING.lock().unwrap_or_else(PoisonError::into_inner);
            if OPENED.load(Ordering::SeqCst) != opened {
                return;
            }
            pending.take();
        }
        hide_window(&app);
    });
}

pub fn hide(app: &AppHandle) {
    PENDING
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    hide_window(app);
}

fn hide_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        platform::set_click_exempt(&window, None);
        if platform::window_visible(&window) {
            platform::hide_window(&window);
        }
    }
}

/// Rahmen der Palette: linke obere Ecke am Mauszeiger; wo der Platz nicht
/// reicht, links bzw. oberhalb davon, und immer im Arbeitsbereich.
fn palette_frame(area: Frame, cursor: (f64, f64), size: (f64, f64)) -> Frame {
    let (w, h) = (size.0.min(area.w), size.1.min(area.h));
    let axis = |pos: f64, len: f64, start: f64, extent: f64| {
        let end = start + extent;
        let p = if pos + CURSOR_GAP + len > end {
            pos - CURSOR_GAP - len
        } else {
            pos + CURSOR_GAP
        };
        p.clamp(start, end - len)
    };
    Frame {
        x: axis(cursor.0, w, area.x, area.w),
        y: axis(cursor.1, h, area.y, area.h),
        w,
        h,
    }
}

#[derive(serde::Serialize)]
pub struct PaletteEntry {
    uuid: String,
    kind: u8,
    preview: String,
    has_thumb: bool,
    snippet: bool,
}

/// Die neuesten aktiven Einträge (`settings.palette.count`), ohne Vorrang für
/// Bausteine oder Angepinntes.
#[tauri::command]
pub fn palette_entries(state: State<'_, AppState>) -> Vec<PaletteEntry> {
    let count = usize::from(state.settings.read().unwrap().palette.count);
    state
        .index
        .read()
        .unwrap()
        .recent(count)
        .into_iter()
        .map(|e| PaletteEntry {
            uuid: e.uuid,
            kind: e.kind,
            preview: e.preview,
            has_thumb: e.has_thumb,
            snippet: e.snippet,
        })
        .collect()
}

/// Frontend hat gerendert: an den Mauszeiger legen und mit Aktivierung zeigen
/// (Ziffern und Esc gehen direkt an die Palette).
#[tauri::command]
pub fn palette_ready(app: AppHandle, width: f64, height: f64) {
    let Some(anchor) = PENDING
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
    else {
        return;
    };
    let Some(window) = app.get_webview_window(LABEL) else {
        return;
    };
    let size = |v: f64, fallback: f64| {
        if v.is_finite() && v >= 1.0 {
            v
        } else {
            fallback
        }
    };
    let frame = palette_frame(
        Frame::work_area(&anchor.monitor),
        (anchor.x, anchor.y),
        (size(width, INITIAL_SIZE.0), size(height, INITIAL_SIZE.1)),
    );
    platform::place_window(&window, &anchor.monitor, frame);
    platform::set_click_exempt(&window, Some(frame));
    SHOWN.fetch_add(1, Ordering::SeqCst);
    platform::show_window_activated(&window);
    // Ein verstecktes Fenster bekommt die DPI des Zielmonitors erst beim
    // Zeigen; tao skaliert die Größe dann nochmals um.
    let drifted = platform::window_frame(&window, &anchor.monitor).is_some_and(|f| {
        [
            (f.x, frame.x),
            (f.y, frame.y),
            (f.w, frame.w),
            (f.h, frame.h),
        ]
        .iter()
        .any(|(a, b)| (a - b).abs() >= 2.0)
    });
    if drifted {
        platform::place_window(&window, &anchor.monitor, frame);
    }
}

#[tauri::command]
pub fn palette_hide(app: AppHandle) {
    hide(&app);
}

/// Eintrag ins zuvor fokussierte Feld bringen, wie Enter in der Historie:
/// Inhalt in die Zwischenablage, dann einfügen; `type_chars` tippt
/// zeichenweise. Bei TOTP kommt nur der im Frontend erzeugte `code`, nie das
/// Secret.
#[tauri::command(async)]
pub fn palette_pick(
    app: AppHandle,
    uuid: String,
    type_chars: bool,
    code: Option<String>,
) -> Result<(), String> {
    let inject = if type_chars {
        typing::Inject::PerChar
    } else {
        typing::Inject::Paste
    };
    let action = history::begin_action();
    hide(&app);
    // 0 lässt `spawn_type` mit Fehlerton abbrechen, statt irgendwohin zu tippen.
    let (target, click) = *TARGET.lock().unwrap_or_else(PoisonError::into_inner);
    let target = Some(target);
    if let Some(code) = code {
        let code = code.trim().to_owned();
        if code.is_empty() || code.len() > 10 || !code.bytes().all(|b| b.is_ascii_digit()) {
            return Err("Kein gültiger Code".into());
        }
        let text = code.clone();
        history::write_own_clipboard(&app, action, move || {
            arboard::Clipboard::new()
                .and_then(|mut c| c.set_text(text))
                .map_err(Into::into)
        })?;
        history::spawn_type(&app, code, Some(inject), target, click);
        return Ok(());
    }
    let kind = history::copy_to_clipboard(&app, &uuid, action)?;
    // Bilder lassen sich nur einfügen.
    let inject = if kind == KIND_IMAGE {
        typing::Inject::Paste
    } else {
        inject
    };
    history::type_entry_to(&app, &uuid, Some(inject), target, click, action)
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Frame = Frame {
        x: 0.0,
        y: 25.0,
        w: 1440.0,
        h: 875.0,
    };

    #[test]
    fn opens_at_cursor() {
        let f = palette_frame(AREA, (100.0, 200.0), (340.0, 180.0));
        assert_eq!((f.x, f.y, f.w, f.h), (104.0, 204.0, 340.0, 180.0));
    }

    #[test]
    fn flips_left_and_up_at_the_edges() {
        let f = palette_frame(AREA, (1400.0, 880.0), (340.0, 180.0));
        assert_eq!((f.x, f.y), (1400.0 - 4.0 - 340.0, 880.0 - 4.0 - 180.0));
    }

    #[test]
    fn stays_inside_the_work_area() {
        // Zeiger in der Menüleiste: unter ihr, nie darüber.
        let f = palette_frame(AREA, (10.0, 5.0), (340.0, 180.0));
        assert!(f.y >= AREA.y && f.x >= AREA.x);
        // Größer als der Arbeitsbereich: auf ihn begrenzt.
        let tiny = Frame {
            x: 50.0,
            y: 0.0,
            w: 300.0,
            h: 100.0,
        };
        let f = palette_frame(tiny, (60.0, 50.0), (340.0, 180.0));
        assert_eq!((f.x, f.y, f.w, f.h), (50.0, 0.0, 300.0, 100.0));
    }
}
