use std::sync::atomic::Ordering;
use std::sync::OnceLock;
use std::time::Duration;

use tauri::image::Image;
use tauri::menu::{CheckMenuItem, MenuBuilder, MenuEvent, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_autostart::ManagerExt;

use crate::state::AppState;
use crate::{hotkeys, windows_util};

pub const TRAY_ID: &str = "main";

/// Handles auf die Menüeinträge, um Häkchen und Hotkey-Hinweise nachzuführen.
pub struct TrayHandles {
    pub hint_paste: MenuItem<Wry>,
    pub history: MenuItem<Wry>,
    pub pause: CheckMenuItem<Wry>,
    pub sounds: CheckMenuItem<Wry>,
    pub autostart: CheckMenuItem<Wry>,
}

static ICON_NORMAL: OnceLock<Image<'static>> = OnceLock::new();
static ICON_BLINK: OnceLock<Image<'static>> = OnceLock::new();
static ICON_TYPING: OnceLock<Image<'static>> = OnceLock::new();

pub fn icon_normal() -> &'static Image<'static> {
    ICON_NORMAL.get_or_init(|| {
        Image::from_bytes(include_bytes!("../icons/tray.ico")).expect("tray.ico dekodierbar")
    })
}

pub fn icon_blink() -> &'static Image<'static> {
    ICON_BLINK.get_or_init(|| {
        Image::from_bytes(include_bytes!("../icons/blinken.ico")).expect("blinken.ico dekodierbar")
    })
}

/// Tipp-Indikator: normales Icon mit grünem Punkt. Der Blink-Task (typing.rs)
/// wechselt zwischen diesem Icon und `icon_normal()`, wodurch der Punkt blinkt.
pub fn icon_typing() -> &'static Image<'static> {
    ICON_TYPING.get_or_init(|| {
        Image::from_bytes(include_bytes!("../icons/tray_typing.png"))
            .expect("tray_typing.png dekodierbar")
    })
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
    });

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon_normal().clone())
        .tooltip(TOOLTIP)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(on_menu_event)
        .build(app)?;

    Ok(())
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
                if let Some(tray) = app.tray_by_id(TRAY_ID) {
                    let icon = if blink { icon_blink() } else { icon_normal() };
                    let _ = tray.set_icon(Some(icon.clone()));
                }
                std::thread::sleep(Duration::from_millis(250));
            }
            if let Some(tray) = app.tray_by_id(TRAY_ID) {
                let _ = tray.set_icon(Some(icon_normal().clone()));
            }
        });
    } else {
        hotkeys::register_paste(app);
    }
}
