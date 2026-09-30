use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuBuilder, MenuEvent, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;

use crate::platform::{self, TrayStyle};
use crate::state::AppState;
use crate::{hotkeys, updater, windows_util};

pub const TRAY_ID: &str = "main";
const UPDATE_ID: &str = "update";

/// Handles auf die Menüeinträge, um Häkchen und Hotkey-Hinweise nachzuführen.
pub struct TrayHandles {
    pub hint_paste: MenuItem<Wry>,
    pub history: MenuItem<Wry>,
    pub pause: CheckMenuItem<Wry>,
    pub sounds: CheckMenuItem<Wry>,
    pub autostart: CheckMenuItem<Wry>,
    menu: Menu<Wry>,
    /// Steht nur im Menü, solange ein Update bereitliegt (`refresh_update`).
    update: MenuItem<Wry>,
}

/// Zustand des einfarbigen Tray-Symbols (Quellen: `icons/tray/*.svg`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Normal = 0,
    Paused = 1,
    Typing = 2,
}

static GLYPH: AtomicU8 = AtomicU8::new(Glyph::Normal as u8);
/// Zuletzt gezeigte Farbvariante; der Theme-Wächter vergleicht dagegen.
static SHOWN_STYLE: Mutex<Option<TrayStyle>> = Mutex::new(None);

fn current_glyph() -> Glyph {
    match GLYPH.load(Ordering::SeqCst) {
        1 => Glyph::Paused,
        2 => Glyph::Typing,
        _ => Glyph::Normal,
    }
}

/// PNG zu Zustand und Farbe. macOS zeigt Tray-Symbole 18 pt hoch, also 36 px;
/// Windows 16 px bei 100 %, darüber die 32-px-Fassung.
fn icon_png(glyph: Glyph, style: TrayStyle, hi_dpi: bool) -> &'static [u8] {
    macro_rules! png {
        ($name:literal) => {
            include_bytes!(concat!("../icons/tray/", $name, ".png")).as_slice()
        };
    }
    match (glyph, style, hi_dpi) {
        (Glyph::Normal, TrayStyle::Template, _) => png!("normal-template-36"),
        (Glyph::Paused, TrayStyle::Template, _) => png!("paused-template-36"),
        (Glyph::Typing, TrayStyle::Template, _) => png!("typing-template-36"),
        (Glyph::Normal, TrayStyle::Light, false) => png!("normal-white-16"),
        (Glyph::Normal, TrayStyle::Light, true) => png!("normal-white-32"),
        (Glyph::Paused, TrayStyle::Light, false) => png!("paused-white-16"),
        (Glyph::Paused, TrayStyle::Light, true) => png!("paused-white-32"),
        (Glyph::Typing, TrayStyle::Light, false) => png!("typing-white-16"),
        (Glyph::Typing, TrayStyle::Light, true) => png!("typing-white-32"),
        (Glyph::Normal, TrayStyle::Dark, false) => png!("normal-black-16"),
        (Glyph::Normal, TrayStyle::Dark, true) => png!("normal-black-32"),
        (Glyph::Paused, TrayStyle::Dark, false) => png!("paused-black-16"),
        (Glyph::Paused, TrayStyle::Dark, true) => png!("paused-black-32"),
        (Glyph::Typing, TrayStyle::Dark, false) => png!("typing-black-16"),
        (Glyph::Typing, TrayStyle::Dark, true) => png!("typing-black-32"),
    }
}

fn icon_image(app: &AppHandle, glyph: Glyph, style: TrayStyle) -> Option<Image<'static>> {
    let hi_dpi = app
        .primary_monitor()
        .ok()
        .flatten()
        .is_some_and(|m| m.scale_factor() > 1.0);
    Image::from_bytes(icon_png(glyph, style, hi_dpi))
        .inspect_err(|e| tracing::error!("Tray-Symbol nicht dekodierbar: {e}"))
        .ok()
}

/// Tray-Symbol auf `glyph` setzen, in der Farbe des aktuellen Designs.
pub fn show_glyph(app: &AppHandle, glyph: Glyph) {
    GLYPH.store(glyph as u8, Ordering::SeqCst);
    apply_icon(app);
}

fn apply_icon(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let style = platform::tray_style();
    let Some(icon) = icon_image(app, current_glyph(), style) else {
        return;
    };
    // `set_icon` allein setzt auf macOS das Template-Flag zurück.
    let _ = tray.set_icon_with_as_template(Some(icon), style == TrayStyle::Template);
    *SHOWN_STYLE.lock().unwrap_or_else(PoisonError::into_inner) = Some(style);
}

/// Taskleisten-Design geändert? Dann das Symbol umfärben.
fn refresh_style(app: &AppHandle) {
    let shown = *SHOWN_STYLE.lock().unwrap_or_else(PoisonError::into_inner);
    if shown != Some(platform::tray_style()) {
        apply_icon(app);
    }
}

/// Windows meldet einen Wechsel des Taskleisten-Designs nur an Fenster, die
/// TippIT nicht dauerhaft hat; eine Registry-Abfrage alle paar Sekunden ist billig.
fn watch_style(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(3));
        refresh_style(&app);
    });
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let (sounds_on, paste_hotkey, history_hotkey) = {
        let state = app.state::<AppState>();
        let s = state.settings.read().unwrap();
        (
            s.sounds,
            display_hotkey(&s.hotkeys.paste),
            display_hotkey(&s.hotkeys.history),
        )
    };

    let info = app.package_info();
    let version = MenuItem::with_id(
        app,
        "version",
        format!("{} {}", info.name, info.version),
        false,
        None::<&str>,
    )?;
    let update = MenuItem::with_id(app, UPDATE_ID, update_label(""), true, None::<&str>)?;
    // Die Historie ist die Hauptaktion und steht oben; der Tipp-Hotkey hat
    // keinen Menü-Gegenpart und bleibt ein grauer Hinweis.
    let history = MenuItem::with_id(
        app,
        "history",
        history_label(&history_hotkey),
        true,
        None::<&str>,
    )?;
    let hint_paste = MenuItem::with_id(
        app,
        "hint_paste",
        paste_label(&paste_hotkey),
        false,
        None::<&str>,
    )?;
    // Pause hält Tipp-Hotkey und Erfassung an, nicht nur das Tippen.
    let pause = CheckMenuItem::with_id(app, "pause", "Pausieren", true, false, None::<&str>)?;
    let sounds = CheckMenuItem::with_id(app, "sounds", "Töne", true, sounds_on, None::<&str>)?;
    let autostart_on = app.autolaunch().is_enabled().unwrap_or(false);
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        "Beim Anmelden starten",
        true,
        autostart_on,
        None::<&str>,
    )?;
    let settings_item = MenuItem::with_id(app, "settings", "Einstellungen…", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "TippIT beenden", true, None::<&str>)?;

    let menu = MenuBuilder::new(app)
        .item(&version)
        .separator()
        .item(&history)
        .item(&hint_paste)
        .separator()
        .item(&pause)
        .item(&sounds)
        .item(&autostart)
        .separator()
        .item(&settings_item)
        .item(&quit)
        .build()?;

    app.manage(TrayHandles {
        hint_paste,
        history,
        pause,
        sounds,
        autostart,
        menu: menu.clone(),
        update,
    });

    let style = platform::tray_style();
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(TOOLTIP)
        .menu(&menu)
        // Links öffnet die Historie, rechts das Menü.
        .show_menu_on_left_click(false)
        .on_menu_event(on_menu_event)
        .on_tray_icon_event(on_tray_icon_event);
    if let Some(icon) = icon_image(app, Glyph::Normal, style) {
        builder = builder
            .icon(icon)
            .icon_as_template(style == TrayStyle::Template);
    }
    builder.build(app)?;
    *SHOWN_STYLE.lock().unwrap_or_else(PoisonError::into_inner) = Some(style);
    if platform::TRAY_FOLLOWS_THEME {
        watch_style(app.clone());
    }
    refresh_update(app);

    Ok(())
}

fn on_tray_icon_event(tray: &tauri::tray::TrayIcon, event: TrayIconEvent) {
    let TrayIconEvent::Click {
        button,
        button_state: MouseButtonState::Up,
        ..
    } = event
    else {
        return;
    };
    let app = tray.app_handle();
    refresh_style(app);
    if button == MouseButton::Left {
        windows_util::show_history(app);
    }
}

/// Eintrag „Update auf … installieren…" an `PendingUpdate` anpassen: einfügen,
/// sobald ein Update bereitliegt, entfernen, wenn keins (mehr) da ist.
pub fn refresh_update(app: &AppHandle) {
    let Some(handles) = app.try_state::<TrayHandles>() else {
        return;
    };
    let version = app
        .try_state::<updater::PendingUpdate>()
        .and_then(|p| p.version());
    let listed = handles.menu.get(UPDATE_ID).is_some();
    match version {
        Some(version) => {
            let _ = handles.update.set_text(update_label(&version));
            if !listed {
                // Direkt unter der Versionszeile.
                let _ = handles.menu.insert(&handles.update, 1);
            }
        }
        None if listed => {
            let _ = handles.menu.remove(&handles.update);
        }
        None => {}
    }
}

fn update_label(version: &str) -> String {
    format!("Update auf {version} installieren…")
}

const TOOLTIP: &str = "TippIT";

fn display_hotkey(value: &str) -> String {
    crate::platform::display_hotkey(value)
}

fn history_label(hotkey: &str) -> String {
    format!("Historie öffnen ({hotkey})")
}

fn paste_label(hotkey: &str) -> String {
    format!("Zwischenablage tippen ({hotkey})")
}

fn on_menu_event(app: &AppHandle, event: MenuEvent) {
    let handles = app.state::<TrayHandles>();
    match event.id().as_ref() {
        "history" => windows_util::show_history(app),
        UPDATE_ID => updater::install_from_tray(app),
        "pause" => {
            let paused = handles.pause.is_checked().unwrap_or(false);
            set_paused(app, paused);
        }
        "sounds" => {
            // Über die zentrale Settings-Pipeline (Persistenz + Event),
            // sonst überschreibt das offene Settings-Fenster den Wert wieder.
            let on = handles.sounds.is_checked().unwrap_or(true);
            let mut settings = app.state::<AppState>().settings.read().unwrap().clone();
            settings.sounds = on;
            if let Err(e) = crate::history::set_settings(app.clone(), settings) {
                tracing::error!("Sounds umschalten fehlgeschlagen: {e}");
            }
        }
        "autostart" => {
            let manager = app.autolaunch();
            let enabled = manager.is_enabled().unwrap_or(false);
            // Translokiert oder vom DMG gestartet landete ein Wegwerfpfad im
            // Autostart-Eintrag; der Tab „Berechtigungen" erklärt das Verschieben.
            if !enabled && crate::platform::install_info().location.is_transient() {
                tracing::warn!("Autostart abgelehnt: TippIT läuft nicht aus dem Programme-Ordner");
                let _ = handles.autostart.set_checked(false);
                windows_util::open_settings_tab(app, "berechtigungen");
                return;
            }
            let result = if enabled {
                manager.disable()
            } else {
                manager.enable()
            };
            if let Err(e) = result {
                tracing::error!("Autostart umschalten fehlgeschlagen: {e}");
            }
            let _ = handles
                .autostart
                .set_checked(manager.is_enabled().unwrap_or(false));
        }
        "settings" => windows_util::open_settings(app),
        "quit" => app.exit(0),
        _ => {}
    }
}

/// Tray-Menü an geänderte Settings anpassen (Hotkey-Hinweise, Sounds-Häkchen) —
/// wird von set_settings aufgerufen.
pub fn refresh_from_settings(app: &AppHandle) {
    let Some(handles) = app.try_state::<TrayHandles>() else {
        return;
    };
    let state = app.state::<AppState>();
    let s = state.settings.read().unwrap();
    let _ = handles
        .hint_paste
        .set_text(paste_label(&display_hotkey(&s.hotkeys.paste)));
    let _ = handles
        .history
        .set_text(history_label(&display_hotkey(&s.hotkeys.history)));
    let _ = handles.sounds.set_checked(s.sounds);
}

/// Pause umschalten: Einfügen-Hotkey (de)registrieren + Tray-Icon blinken lassen.
pub fn set_paused(app: &AppHandle, paused: bool) {
    let state = app.state::<AppState>();
    state.paused.store(paused, Ordering::SeqCst);
    // Pause auch ohne Menü erkennbar (Hover), nicht nur am Blinken.
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(if paused { "TippIT (pausiert)" } else { TOOLTIP }));
    }
    if let Some(handles) = app.try_state::<TrayHandles>() {
        let _ = handles.pause.set_checked(paused);
    }
    // Generation-Bump beendet einen eventuell laufenden Blink-Task.
    let generation = state.blink_gen.fetch_add(1, Ordering::SeqCst) + 1;

    if paused {
        hotkeys::unregister_paste(app);
        let app = app.clone();
        std::thread::spawn(move || {
            let mut blink = false;
            loop {
                {
                    let state = app.state::<AppState>();
                    if state.blink_gen.load(Ordering::SeqCst) != generation
                        || !state.paused.load(Ordering::SeqCst)
                    {
                        break;
                    }
                }
                blink = !blink;
                show_glyph(&app, if blink { Glyph::Paused } else { Glyph::Normal });
                std::thread::sleep(Duration::from_millis(250));
            }
            show_glyph(&app, Glyph::Normal);
        });
    } else {
        hotkeys::register_paste(app);
    }
}
