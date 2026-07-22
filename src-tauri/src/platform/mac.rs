//! macOS-Backend der Plattform-Schicht (CoreGraphics/AppKit, nur Apple Silicon).

use std::sync::mpsc::Sender;
use std::time::Duration;

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::CFDictionary;
use core_foundation::string::CFString;
use core_graphics::event::{CGEvent, CGEventTapLocation, CGKeyCode};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use objc2::AnyThread;
use objc2_app_kit::{
    NSApplicationActivationOptions, NSPasteboard, NSRunningApplication, NSSound, NSWorkspace,
};
use objc2_foundation::{NSData, NSProcessInfo, NSString, NSURL};

use crate::storage::settings::SyncSettings;
use crate::sync::policy::BlockReason;

use super::SpecialKey;

// ---------------------------------------------------------------------------
// Eingabe-Injektion (CGEvent auf dem HID-Tap)
// ---------------------------------------------------------------------------

const KEY_RETURN: CGKeyCode = 36;
const KEY_TAB: CGKeyCode = 48;

/// Ein CGEvent transportiert offiziell nur ~20 UTF-16-Einheiten — hier wird
/// intern in 16er-Häppchen gechunkt (Surrogatpaare bleiben zusammen), egal wie
/// groß der übergebene Puffer ist.
const EVENT_UNITS: usize = 16;

pub fn send_text(units: &[u16]) {
    let Ok(source) = CGEventSource::new(CGEventSourceStateID::HIDSystemState) else {
        tracing::warn!("CGEventSource nicht erzeugbar — tippe nicht");
        return;
    };
    let mut i = 0;
    while i < units.len() {
        let mut end = (i + EVENT_UNITS).min(units.len());
        // Kein Schnitt hinter einem High-Surrogate (0xD800–0xDBFF).
        if end < units.len() && (0xD800..0xDC00).contains(&units[end - 1]) {
            end += 1;
        }
        let chunk = &units[i..end];
        // Der Unicode-String hängt am KeyDown; das KeyUp hält Apps zufrieden,
        // die auf ausgeglichene Down/Up-Paare bestehen.
        if let (Ok(down), Ok(up)) = (
            CGEvent::new_keyboard_event(source.clone(), 0, true),
            CGEvent::new_keyboard_event(source.clone(), 0, false),
        ) {
            down.set_string_from_utf16_unchecked(chunk);
            down.post(CGEventTapLocation::HID);
            up.post(CGEventTapLocation::HID);
        }
        i = end;
    }
}

pub fn send_key(key: SpecialKey) {
    let code = match key {
        SpecialKey::Return => KEY_RETURN,
        SpecialKey::Tab => KEY_TAB,
    };
    let Ok(source) = CGEventSource::new(CGEventSourceStateID::HIDSystemState) else {
        return;
    };
    if let (Ok(down), Ok(up)) = (
        CGEvent::new_keyboard_event(source.clone(), code, true),
        CGEvent::new_keyboard_event(source, code, false),
    ) {
        down.post(CGEventTapLocation::HID);
        up.post(CGEventTapLocation::HID);
    }
}

// CGEventSourceFlagsState ist in core-graphics nicht gewrappt.
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceFlagsState(state_id: i32) -> u64;
}

/// true, solange ⌘/⇧/⌃/⌥ physisch gehalten werden (Hardware-Zustand, HID-State).
pub fn modifiers_held() -> bool {
    const MASK: u64 = 0x0002_0000 // Shift
        | 0x0004_0000 // Control
        | 0x0008_0000 // Option
        | 0x0010_0000; // Command
    let flags = unsafe { CGEventSourceFlagsState(CGEventSourceStateID::HIDSystemState as i32) };
    flags & MASK != 0
}

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrustedWithOptions(options: core_foundation::dictionary::CFDictionaryRef)
        -> bool;
}

/// Bedienungshilfen-Berechtigung prüfen und beim ersten Mal den System-Dialog
/// auslösen — ohne sie verwirft macOS gepostete Tastatur-Events stillschweigend.
pub fn ensure_input_permission() -> bool {
    let options = CFDictionary::from_CFType_pairs(&[(
        CFString::from_static_string("AXTrustedCheckOptionPrompt").as_CFType(),
        CFBoolean::true_value().as_CFType(),
    )]);
    let trusted = unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) };
    if !trusted {
        tracing::warn!(
            "Keine Bedienungshilfen-Berechtigung — Tippen wirkungslos, bis TippIT unter \
             Systemeinstellungen → Datenschutz & Sicherheit → Bedienungshilfen erlaubt ist"
        );
    }
    trusted
}

// ---------------------------------------------------------------------------
// Vordergrund-Ziel (Tipp-Ziel der Historie)
// ---------------------------------------------------------------------------

/// Aktuell aktive App als opakes Target (PID als isize). 0 = unbekannt.
pub fn current_foreground() -> isize {
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|app| app.processIdentifier() as isize)
        .unwrap_or(0)
}

pub fn activate_target(target: isize) {
    let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(target as i32)
    else {
        return;
    };
    #[allow(deprecated)] // ActivateIgnoringOtherApps: nötig, solange wir selbst aktiv sind
    app.activateWithOptions(NSApplicationActivationOptions::ActivateIgnoringOtherApps);
}

/// Warten, bis die Ziel-App wirklich im Vordergrund ist. Vergleich auf
/// PID-Ebene — Dialoge/Panels derselben App zählen damit automatisch als Treffer
/// (Pendant zum GA_ROOTOWNER-Vergleich unter Windows).
pub fn wait_foreground(target: isize, timeout: Duration) -> bool {
    let start = std::time::Instant::now();
    loop {
        if current_foreground() == target {
            return true;
        }
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
}

// ---------------------------------------------------------------------------
// Fenster-Sichtbarkeit & Arbeitsbereich
// ---------------------------------------------------------------------------

// Anders als unter Windows (SW_SHOWNOACTIVATE-Altlast) stimmt Tauris interner
// Visible-Zustand auf macOS — show/hide/is_visible reichen hier.

pub fn window_visible(window: &tauri::WebviewWindow) -> bool {
    window.is_visible().unwrap_or(false)
}

pub fn hide_window(window: &tauri::WebviewWindow) {
    let _ = window.hide();
}

/// Fenster MIT Aktivierung zeigen — set_focus aktiviert auch die App selbst
/// (wichtig als Accessory-App ohne Dock-Icon).
pub fn show_window_activated(window: &tauri::WebviewWindow) {
    let _ = window.show();
    let _ = window.set_focus();
}

/// Arbeitsbereich des primären Monitors (ohne Menüleiste/Dock) als
/// (links, oben, rechts, unten) in physischen Pixeln.
pub fn work_area(window: &tauri::WebviewWindow) -> (f64, f64, f64, f64) {
    match window.primary_monitor() {
        Ok(Some(monitor)) => {
            let area = monitor.work_area();
            (
                f64::from(area.position.x),
                f64::from(area.position.y),
                f64::from(area.position.x + area.size.width as i32),
                f64::from(area.position.y + area.size.height as i32),
            )
        }
        _ => {
            tracing::warn!("Kein primärer Monitor ermittelbar — nutze 1440×900");
            (0.0, 0.0, 1440.0, 900.0)
        }
    }
}

// ---------------------------------------------------------------------------
// Zwischenablage
// ---------------------------------------------------------------------------

/// changeCount des General-Pasteboards (Pendant zur Win32-Sequenznummer).
pub fn clipboard_seq() -> i64 {
    NSPasteboard::generalPasteboard().changeCount() as i64
}

/// Kopierte Dateien (public.file-url je Pasteboard-Item).
pub fn clipboard_file_list() -> Option<Vec<String>> {
    let file_url_type = NSString::from_str("public.file-url");
    let pb = NSPasteboard::generalPasteboard();
    let items = pb.pasteboardItems()?;
    let files: Vec<String> = items
        .iter()
        .filter_map(|item| {
            let raw = item.stringForType(&file_url_type)?;
            let url = NSURL::URLWithString(&raw)?;
            url.path().map(|p| p.to_string())
        })
        .collect();
    (!files.is_empty()).then_some(files)
}

/// macOS hat keine Clipboard-Change-Notification — changeCount-Polling ist der
/// offizielle Weg (machen alle Clipboard-Manager so). 400 ms halten die Latenz
/// unauffällig und die Last bei null.
pub fn watch_clipboard(tx: Sender<()>) {
    std::thread::spawn(move || {
        let mut last = clipboard_seq();
        loop {
            std::thread::sleep(Duration::from_millis(400));
            let current = clipboard_seq();
            if current != last {
                last = current;
                if tx.send(()).is_err() {
                    return; // Capture-Worker weg → Thread beenden
                }
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Sound
// ---------------------------------------------------------------------------

/// macOS kann keine Frequenz-Beeps wie kernel32 — deshalb wird ein Sinuston
/// als WAV in-memory erzeugt und über NSSound abgespielt (gleiche Semantik:
/// blockiert für die volle Dauer).
pub fn beep_blocking(freq: u32, duration_ms: u32) {
    let wav = tone_wav(freq, duration_ms);
    let data = NSData::with_bytes(&wav);
    let Some(sound) = NSSound::initWithData(NSSound::alloc(), &data) else {
        tracing::warn!("NSSound konnte Ton nicht laden");
        return;
    };
    sound.play();
    // NSSound spielt asynchron — schlafen hält zugleich das Objekt am Leben.
    std::thread::sleep(Duration::from_millis(u64::from(duration_ms) + 30));
}

/// 44,1-kHz-Mono-16-bit-Sinus mit 5-ms-Fades (kein Knacken), Amplitude gedeckelt.
fn tone_wav(freq: u32, duration_ms: u32) -> Vec<u8> {
    const RATE: u32 = 44_100;
    const AMPLITUDE: f32 = 0.25;
    let samples = (RATE * duration_ms / 1000) as usize;
    let fade = (RATE / 200) as usize; // 5 ms
    let mut data = Vec::with_capacity(44 + samples * 2);

    let byte_len = (samples * 2) as u32;
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&(36 + byte_len).to_le_bytes());
    data.extend_from_slice(b"WAVEfmt ");
    data.extend_from_slice(&16u32.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes()); // PCM
    data.extend_from_slice(&1u16.to_le_bytes()); // mono
    data.extend_from_slice(&RATE.to_le_bytes());
    data.extend_from_slice(&(RATE * 2).to_le_bytes());
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&16u16.to_le_bytes());
    data.extend_from_slice(b"data");
    data.extend_from_slice(&byte_len.to_le_bytes());

    for i in 0..samples {
        let envelope = ((i + 1).min(samples - i).min(fade) as f32) / fade as f32;
        let t = i as f32 / RATE as f32;
        let value = (t * freq as f32 * std::f32::consts::TAU).sin() * AMPLITUDE * envelope;
        data.extend_from_slice(&((value * f32::from(i16::MAX)) as i16).to_le_bytes());
    }
    data
}

// ---------------------------------------------------------------------------
// Schlüsselschutz at-rest
// ---------------------------------------------------------------------------

// Bewusst KEIN Keychain: bei ad-hoc-signierten Builds bindet die Keychain-ACL
// an den Binary-Hash — nach jedem Update käme ein Passwort-Prompt. Schutzniveau
// entspricht DPAPI im User-Scope (gleicher User liest mit): 0600-Rechte
// (storage::crypto::write_wrapped) + FileVault decken denselben Angriffsvektor
// (fremde User, Offline-Zugriff) ab.

pub fn protect(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    Ok(data.to_vec())
}

pub fn unprotect(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    Ok(data.to_vec())
}

// ---------------------------------------------------------------------------
// Dateisystem & Sync-Richtlinien
// ---------------------------------------------------------------------------

/// Der Punkt-Präfix versteckt unter macOS bereits — nichts zu tun.
pub fn hide_directory(_path: &std::path::Path) -> anyhow::Result<()> {
    Ok(())
}

/// Stromsparmodus blockt wie der Windows-Energiesparmodus; Mobilfunk-/
/// Datensparmodus-Erkennung gibt es unter macOS nicht (kein WWAN-Profil-API).
pub fn block_reason(settings: &SyncSettings) -> Option<BlockReason> {
    if !settings.allow_energy_saver && NSProcessInfo::processInfo().isLowPowerModeEnabled() {
        return Some(BlockReason::EnergySaver);
    }
    None
}
