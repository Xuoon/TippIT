use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RETURN,
    VK_RWIN, VK_SHIFT, VK_TAB,
};

use crate::sound;
use crate::state::AppState;
use crate::storage::settings::{TypingMode, TypingSettings};
use crate::{tray, windows_util};

/// STRG+E: Zwischenablage als Tastatureingaben ins fokussierte Fenster tippen.
/// Ablauf (Parität zu AutoIt): Text lesen → trimmen → leer = no-op → 440-Hz-Beep →
/// pre_delay warten → auf Modifier-Release warten → injizieren.
pub fn paste_clipboard(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let Ok(_guard) = state.typing_lock.try_lock() else {
            tracing::debug!("Tipp-Vorgang läuft bereits, ignoriere STRG+E");
            return;
        };
        // Bei offener Historie nicht tippen (Schutz vor Vertippen ins eigene Fenster).
        if windows_util::history_visible(&app) {
            tracing::debug!("Historie offen — STRG+E ignoriert");
            return;
        }
        let generation = state.typing_gen.fetch_add(1, Ordering::SeqCst) + 1;
        let (cfg, sounds) = {
            let s = state.settings.read().unwrap();
            (s.typing.clone(), s.sounds)
        };

        let mut text = match arboard::Clipboard::new().and_then(|mut c| c.get_text()) {
            Ok(t) => t,
            Err(e) => {
                tracing::debug!("kein Text in der Zwischenablage: {e}");
                return;
            }
        };
        if cfg.trim {
            text = text.trim().to_string();
        }
        if text.is_empty() {
            return;
        }

        if sounds {
            sound::beep_blocking(440, 200);
        }
        // Pre-Delay in Scheiben schlafen, damit der Abbruch-Hotkey auch hier greift.
        let mut waited = 0;
        while waited < cfg.pre_delay_ms && alive(&app, generation) {
            let step = (cfg.pre_delay_ms - waited).min(50);
            std::thread::sleep(Duration::from_millis(step));
            waited += step;
        }
        // Abbruch während des Pre-Delays: gar nicht erst mit dem Tippen beginnen.
        if !alive(&app, generation) {
            return;
        }
        type_text(&app, &text, &cfg, generation);
    });
}

/// Laufenden Tipp-Vorgang abbrechen (Abbruch-Hotkey). Quittiert nur, wenn wirklich
/// etwas läuft: der belegte `typing_lock` ist das verlässliche „aktiv"-Signal.
/// Ein leerer Abbruch (kein Vorgang) tut bewusst nichts — kein Ton, kein Bump.
pub fn cancel(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.typing_lock.try_lock().is_ok() {
        return; // Lock frei → kein Vorgang aktiv, nichts abzubrechen.
    }
    state.typing_gen.fetch_add(1, Ordering::SeqCst);
    if state.settings.read().unwrap().sounds {
        sound::beep(300, 150);
    }
}

/// Blink-Task am Ende eines regulär abgeschlossenen Vorgangs stoppen (bumpt die
/// Generation, ohne zu quittieren). Läuft ausschließlich unter gehaltenem
/// `typing_lock`, weshalb ein hier gesetzter Bump keinen bereits neu gestarteten
/// Vorgang entwerten kann (der zieht seine Generation erst NACH Lock-Erwerb).
fn stop_blink(app: &AppHandle) {
    app.state::<AppState>()
        .typing_gen
        .fetch_add(1, Ordering::SeqCst);
}

/// true, solange die Generation noch aktuell ist (kein Abbruch, kein neuer Vorgang).
fn alive(app: &AppHandle, generation: u64) -> bool {
    app.state::<AppState>().typing_gen.load(Ordering::SeqCst) == generation
}

/// Grünen Punkt am Tray-Icon blinken lassen, bis die Generation endet/abbricht.
fn start_typing_blink(app: &AppHandle, generation: u64) {
    let app = app.clone();
    std::thread::spawn(move || {
        let mut on = true;
        while alive(&app, generation) {
            if let Some(t) = app.tray_by_id(tray::TRAY_ID) {
                // Wechsel zwischen „Icon mit grünem Punkt" und normalem Icon → der
                // Punkt blinkt.
                let icon = if on {
                    tray::icon_typing()
                } else {
                    tray::icon_normal()
                };
                let _ = t.set_icon(Some(icon.clone()));
            }
            on = !on;
            std::thread::sleep(Duration::from_millis(400));
        }
        // Nicht pausiert → zurück aufs normale Icon (bei Pause übernimmt deren Blink-Task).
        let paused = app.state::<AppState>().paused.load(Ordering::SeqCst);
        if let Some(t) = app.tray_by_id(tray::TRAY_ID) {
            if !paused {
                let _ = t.set_icon(Some(tray::icon_normal().clone()));
            }
        }
    });
}

/// Text mit gegebener Konfiguration tippen (wird auch von der Historie-Aktion genutzt).
/// Läuft unter der übergebenen Generation: ein Bump von `typing_gen` (Abbruch-Hotkey)
/// stoppt die Injektion und den Tray-Blinker.
pub fn type_text(app: &AppHandle, text: &str, cfg: &TypingSettings, generation: u64) {
    // CR-only-Umbrüche (Alt-Mac, manche Excel-/Terminal-Exporte) als Enter
    // behandeln — push_char droppt \r, was sonst alle Umbrüche verschluckt.
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let text = text.as_str();
    // Physisch gehaltene Modifier (STRG des Hotkeys!) müssen los sein, sonst
    // interpretiert das Zielfenster die Zeichen als Shortcuts (STRG+e, STRG+v, …).
    // Der Abbruch-Hotkey greift auch in dieser Wartephase (alive-Check innen).
    if !wait_modifiers_released(app, generation, Duration::from_secs(3)) {
        tracing::warn!("Modifier nach 3 s nicht losgelassen — tippe trotzdem");
    }
    if !alive(app, generation) {
        return;
    }
    start_typing_blink(app, generation);

    match cfg.mode {
        TypingMode::Bulk => {
            // Chunks nur an Zeichen-Grenzen schneiden: SendInput ist pro Aufruf
            // atomar — würde ein Surrogatpaar über zwei Aufrufe gesplittet,
            // könnte fremde Eingabe dazwischenrutschen und das Zeichen zerstören.
            let mut chunk: Vec<INPUT> = Vec::with_capacity(72);
            for ch in text.chars() {
                if !alive(app, generation) {
                    return;
                }
                let mut group = Vec::with_capacity(4);
                push_char(ch, &mut group);
                if chunk.len() + group.len() > 64 && !chunk.is_empty() {
                    send(&chunk);
                    chunk.clear();
                }
                chunk.extend(group);
            }
            if !chunk.is_empty() && alive(app, generation) {
                send(&chunk);
            }
        }
        TypingMode::PerChar => {
            for ch in text.chars() {
                if !alive(app, generation) {
                    return;
                }
                let mut buf = Vec::with_capacity(4);
                push_char(ch, &mut buf);
                if !buf.is_empty() {
                    send(&buf);
                    std::thread::sleep(Duration::from_millis(cfg.char_delay_ms));
                }
            }
        }
    }
    // Regulär fertig: Generation bumpen, damit der Blink-Task sicher stoppt.
    // (Bei Abbruch kehren die Schleifen oben schon per `return` zurück; der
    // Blink-Task endet dann durch den Bump des Abbruchs.)
    stop_blink(app);
}

fn wait_modifiers_released(app: &AppHandle, generation: u64, timeout: Duration) -> bool {
    const MODS: [VIRTUAL_KEY; 5] = [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN];
    let start = Instant::now();
    while start.elapsed() < timeout {
        // Abbruch mitten in der Warteschleife sofort respektieren — sonst hinge
        // ein neu preemptender Vorgang bis zu 3 s an einem alten Modifier-Halt.
        if !alive(app, generation) {
            return false;
        }
        let held = MODS
            .iter()
            .any(|vk| (unsafe { GetAsyncKeyState(vk.0 as i32) } as u16) & 0x8000 != 0);
        if !held {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

fn push_char(ch: char, buf: &mut Vec<INPUT>) {
    match ch {
        // Viele Anwendungen ignorieren ein Unicode-LF — echte Enter-Taste senden.
        '\n' => push_vk(VK_RETURN, buf),
        '\r' => {} // nach der Normalisierung in type_text nicht mehr erreichbar
        '\t' => push_vk(VK_TAB, buf),
        _ => {
            let mut units = [0u16; 2];
            for &unit in ch.encode_utf16(&mut units).iter() {
                buf.push(keyboard_input(VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE));
                buf.push(keyboard_input(
                    VIRTUAL_KEY(0),
                    unit,
                    KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
                ));
            }
        }
    }
}

fn push_vk(vk: VIRTUAL_KEY, buf: &mut Vec<INPUT>) {
    buf.push(keyboard_input(vk, 0, KEYBD_EVENT_FLAGS(0)));
    buf.push(keyboard_input(vk, 0, KEYEVENTF_KEYUP));
}

fn keyboard_input(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) {
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        // UIPI: Injektion in elevated Fenster wird ohne eigene Elevation still verworfen.
        tracing::warn!(
            "SendInput: nur {sent}/{} Events injiziert (Ziel elevated?)",
            inputs.len()
        );
    }
}
