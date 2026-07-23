use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Manager};

use crate::platform::{self, SpecialKey};
use crate::sound;
use crate::state::AppState;
use crate::storage::settings::{TypingMode, TypingSettings};
use crate::{tray, windows_util};

/// Bulk-Modus: UTF-16-Einheiten pro Injektions-Aufruf. Geschnitten wird nur an
/// Zeichen-Grenzen — die Injektion ist pro Aufruf atomar; würde ein Surrogatpaar
/// über zwei Aufrufe gesplittet, könnte fremde Eingabe dazwischenrutschen und
/// das Zeichen zerstören.
const CHUNK_UNITS: usize = 32;

/// ESC als globalen Abbruch für die Dauer EINES Tipp-Vorgangs registrieren.
/// RAII: Drop deregistriert auf jedem Ausstiegspfad (Abbruch, Fehler, fertig) —
/// außerhalb eines Vorgangs bleibt ESC frei für andere Anwendungen.
pub struct EscCancelGuard(AppHandle);

impl EscCancelGuard {
    pub fn new(app: &AppHandle) -> Self {
        crate::hotkeys::register_typing_esc(app);
        Self(app.clone())
    }
}

impl Drop for EscCancelGuard {
    fn drop(&mut self) {
        crate::hotkeys::unregister_typing_esc(&self.0);
    }
}

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

        // Ab hier läuft ein echter Vorgang → ESC bricht ab (auch im Pre-Delay).
        let _esc = EscCancelGuard::new(&app);

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
    // macOS verwirft Events ohne Bedienungshilfen-Berechtigung STILL — vor jedem
    // Versuch prüfen statt ins Leere zu tippen; der Aufruf löst zugleich den
    // System-Prompt erneut aus. (Windows: immer true.)
    if !platform::ensure_input_permission() {
        tracing::warn!("Keine Eingabe-Berechtigung — Tippvorgang abgebrochen");
        if app.state::<AppState>().settings.read().unwrap().sounds {
            sound::beep_blocking(220, 300);
        }
        stop_blink(app);
        return;
    }
    // CR-only-Umbrüche (Alt-Mac, manche Excel-/Terminal-Exporte) als Enter
    // behandeln — sonst würden alle Umbrüche verschluckt.
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let text = text.as_str();
    // Physisch gehaltene Modifier (STRG des Hotkeys!) müssen los sein, sonst
    // interpretiert das Zielfenster die Zeichen als Shortcuts (STRG+e, STRG+v, …).
    // Der Abbruch-Hotkey greift auch in dieser Wartephase (alive-Check innen).
    if !wait_modifiers_released(app, generation, Duration::from_secs(3)) {
        if alive(app, generation) {
            tracing::warn!("Modifier nach 3 s nicht losgelassen — Tippvorgang abgebrochen");
            if app.state::<AppState>().settings.read().unwrap().sounds {
                sound::beep_blocking(220, 300);
            }
            stop_blink(app);
        }
        return;
    }
    start_typing_blink(app, generation);

    match cfg.mode {
        TypingMode::Bulk => {
            let mut chunk: Vec<u16> = Vec::with_capacity(CHUNK_UNITS + 2);
            for ch in text.chars() {
                if !alive(app, generation) {
                    return;
                }
                if let Some(key) = special_key(ch) {
                    flush(&mut chunk);
                    platform::send_key(key);
                    continue;
                }
                let mut units = [0u16; 2];
                let encoded = ch.encode_utf16(&mut units);
                if chunk.len() + encoded.len() > CHUNK_UNITS && !chunk.is_empty() {
                    flush(&mut chunk);
                }
                chunk.extend_from_slice(encoded);
            }
            if alive(app, generation) {
                flush(&mut chunk);
            }
        }
        TypingMode::PerChar => {
            for ch in text.chars() {
                if !alive(app, generation) {
                    return;
                }
                match special_key(ch) {
                    Some(key) => platform::send_key(key),
                    None => {
                        let mut units = [0u16; 2];
                        platform::send_text(ch.encode_utf16(&mut units));
                    }
                }
                std::thread::sleep(Duration::from_millis(cfg.char_delay_ms));
            }
        }
    }
    // Regulär fertig: Generation bumpen, damit der Blink-Task sicher stoppt.
    // (Bei Abbruch kehren die Schleifen oben schon per `return` zurück; der
    // Blink-Task endet dann durch den Bump des Abbruchs.)
    stop_blink(app);
}

/// Viele Anwendungen ignorieren Unicode-LF/-Tab — echte Taste senden.
fn special_key(ch: char) -> Option<SpecialKey> {
    match ch {
        '\n' => Some(SpecialKey::Return),
        '\t' => Some(SpecialKey::Tab),
        _ => None,
    }
}

fn flush(chunk: &mut Vec<u16>) {
    if !chunk.is_empty() {
        platform::send_text(chunk);
        chunk.clear();
    }
}

fn wait_modifiers_released(app: &AppHandle, generation: u64, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        // Abbruch mitten in der Warteschleife sofort respektieren — sonst hinge
        // ein neu preemptender Vorgang bis zu 3 s an einem alten Modifier-Halt.
        if !alive(app, generation) {
            return false;
        }
        if !platform::modifiers_held() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}
