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

/// Reine Menüleisten-App: kein Dock-Icon und kein App-Switcher-Eintrag.
pub fn configure_app(app: &mut tauri::App) {
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
}

pub fn default_hotkeys() -> (&'static str, &'static str) {
    // ⌘ statt ⌃: ctrl+e wäre das systemweite Cocoa-„Zeilenende" (Emacs-Bindings).
    ("cmd+e", "cmd+shift+e")
}

/// PARITÄT: Symbole und Token müssen deckungsgleich mit `formatHotkey` in
/// `src/lib/platform.ts` bleiben.
pub fn display_hotkey(value: &str) -> String {
    value
        .to_uppercase()
        .replace("SUPER", "⌘")
        .replace("CMD", "⌘")
        .replace("CTRL", "⌃")
        .replace("SHIFT", "⇧")
        .replace("ALT", "⌥")
        .replace('+', " + ")
}

/// Auf macOS ist 0600 Teil des Schutzkonzepts für die ungewrappten Key-Dateien.
pub fn secure_key_file(path: &std::path::Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(())
}

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

const SELF_BUNDLE_ID: &str = "de.labit.tippit";

/// Best-effort Vordergrund-App für Source-Meta. Nie panic.
/// `None` = transient/unbekannt; Self wird mit `is_self: true` geliefert.
pub fn foreground_app_info() -> Option<super::ForegroundApp> {
    let app = NSWorkspace::sharedWorkspace().frontmostApplication()?;
    let bundle_id = app
        .bundleIdentifier()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());
    let name = app
        .localizedName()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| bundle_id.clone())
        .unwrap_or_else(|| "Unbekannt".into());
    let is_self = match &bundle_id {
        Some(id) => id == SELF_BUNDLE_ID,
        None => name.eq_ignore_ascii_case("TippIT"),
    };
    let id = bundle_id.unwrap_or_else(|| {
        if is_self {
            SELF_BUNDLE_ID.into()
        } else {
            format!("pid:{}", app.processIdentifier())
        }
    });
    let icon_png = if is_self {
        None
    } else {
        app.icon().and_then(|img| ns_image_to_png32(&img))
    };
    Some(super::ForegroundApp {
        id,
        name,
        icon_png,
        is_self,
    })
}

/// NSImage → 32×32 PNG (best-effort via TIFF + image-crate).
fn ns_image_to_png32(image: &objc2_app_kit::NSImage) -> Option<Vec<u8>> {
    let tiff = image.TIFFRepresentation()?;
    let bytes = tiff.to_vec();
    if bytes.is_empty() {
        return None;
    }
    let dyn_img = image::load_from_memory(&bytes).ok()?;
    let resized = dyn_img.resize_exact(32, 32, image::imageops::FilterType::Triangle);
    let mut out = std::io::Cursor::new(Vec::new());
    resized.write_to(&mut out, image::ImageFormat::Png).ok()?;
    Some(out.into_inner())
}

/// OCR über macOS Vision (VNRecognizeTextRequest). Best-effort.
/// Liefert pro erkannter Zeile Text + normalisierte Position (oben-links).
pub fn ocr_png(png: &[u8]) -> anyhow::Result<Vec<super::OcrLine>> {
    ocr_png_vision(png)
}

fn ocr_png_vision(png: &[u8]) -> anyhow::Result<Vec<super::OcrLine>> {
    // Vision Framework via objc runtime — synchron performRequests.
    use objc2::msg_send;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject};
    use objc2_foundation::{NSArray, NSData, NSDictionary, NSRect, NSString};

    // Vision.framework dynamisch laden (nicht in Link-Liste nötig mit dyld lazy).
    #[link(name = "Vision", kind = "framework")]
    extern "C" {}

    let data = NSData::with_bytes(png);
    // VNImageRequestHandler alloc initWithData:options:
    let handler_cls = AnyClass::get(c"VNImageRequestHandler")
        .ok_or_else(|| anyhow::anyhow!("Vision framework nicht geladen"))?;
    let handler: *mut AnyObject = unsafe { msg_send![handler_cls, alloc] };
    let empty = NSDictionary::<objc2_foundation::NSObject, objc2_foundation::NSObject>::new();
    let handler: *mut AnyObject =
        unsafe { msg_send![handler, initWithData: &*data, options: &*empty] };
    let handler = unsafe { Retained::from_raw(handler) }
        .ok_or_else(|| anyhow::anyhow!("VNImageRequestHandler init fehlgeschlagen"))?;

    let req_cls = AnyClass::get(c"VNRecognizeTextRequest")
        .ok_or_else(|| anyhow::anyhow!("VNRecognizeTextRequest fehlt"))?;
    let request: *mut AnyObject = unsafe { msg_send![req_cls, new] };
    let request = unsafe { Retained::from_raw(request) }
        .ok_or_else(|| anyhow::anyhow!("VNRecognizeTextRequest new fehlgeschlagen"))?;
    // recognitionLevel = accurate (1) if available
    let _: () = unsafe { msg_send![&*request, setRecognitionLevel: 1_usize] };

    let requests = NSArray::from_slice(&[&*request]);
    let mut err: *mut AnyObject = std::ptr::null_mut();
    let ok: bool = unsafe { msg_send![&*handler, performRequests: &*requests, error: &mut err] };
    if !ok {
        return Err(anyhow::anyhow!("Vision OCR fehlgeschlagen"));
    }

    let results: *mut AnyObject = unsafe { msg_send![&*request, results] };
    if results.is_null() {
        return Ok(Vec::new());
    }
    let count: usize = unsafe { msg_send![results, count] };
    let mut lines = Vec::new();
    for i in 0..count {
        let obs: *mut AnyObject = unsafe { msg_send![results, objectAtIndex: i] };
        if obs.is_null() {
            continue;
        }
        let candidates: *mut AnyObject = unsafe { msg_send![obs, topCandidates: 1_usize] };
        if candidates.is_null() {
            continue;
        }
        let cand_count: usize = unsafe { msg_send![candidates, count] };
        if cand_count == 0 {
            continue;
        }
        let cand: *mut AnyObject = unsafe { msg_send![candidates, objectAtIndex: 0_usize] };
        if cand.is_null() {
            continue;
        }
        let s: *mut AnyObject = unsafe { msg_send![cand, string] };
        if s.is_null() {
            continue;
        }
        // NSString → Rust
        let ns = unsafe { &*(s as *const NSString) };
        let text = ns.to_string();
        if text.trim().is_empty() {
            continue;
        }
        // boundingBox: normalisierter CGRect, Ursprung unten-links. Für das
        // CSS-Overlay auf oben-links flippen (top = 1 − (y + height)).
        let bbox: NSRect = unsafe { msg_send![obs, boundingBox] };
        lines.push(super::OcrLine {
            text,
            x: bbox.origin.x,
            y: 1.0 - (bbox.origin.y + bbox.size.height),
            w: bbox.size.width,
            h: bbox.size.height,
        });
    }
    Ok(lines)
}

/// Datei-Pfad oder URL im Standard-Handler öffnen (`open`). Die Scheme-/Typ-Prüfung
/// macht der Aufrufer (`history::open_entry`) — hier wird nur weitergereicht.
pub fn open_external(target: &str) -> anyhow::Result<()> {
    std::process::Command::new("open")
        .arg(target)
        .spawn()
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("open fehlgeschlagen: {e}"))
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

/// Runde Fenster-Ecken nativ (WKWebView-Host-Layer), zusätzlich zu CSS-Radius.
pub fn round_window_corners(window: &tauri::WebviewWindow, radius: f64) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    let Ok(ns_window) = window.ns_window() else {
        return;
    };
    let ns_window = ns_window as *mut AnyObject;
    if ns_window.is_null() {
        return;
    }
    unsafe {
        // contentView.wantsLayer = YES; layer.cornerRadius = radius; masksToBounds = YES
        let content: *mut AnyObject = msg_send![ns_window, contentView];
        if content.is_null() {
            return;
        }
        let _: () = msg_send![content, setWantsLayer: true];
        let layer: *mut AnyObject = msg_send![content, layer];
        if layer.is_null() {
            return;
        }
        let _: () = msg_send![layer, setCornerRadius: radius];
        let _: () = msg_send![layer, setMasksToBounds: true];
        // Auch WebView-Layer runden, falls vorhanden
        let subviews: *mut AnyObject = msg_send![content, subviews];
        if !subviews.is_null() {
            let count: usize = msg_send![subviews, count];
            for i in 0..count {
                let view: *mut AnyObject = msg_send![subviews, objectAtIndex: i];
                if view.is_null() {
                    continue;
                }
                let _: () = msg_send![view, setWantsLayer: true];
                let vlayer: *mut AnyObject = msg_send![view, layer];
                if !vlayer.is_null() {
                    let _: () = msg_send![vlayer, setCornerRadius: radius];
                    let _: () = msg_send![vlayer, setMasksToBounds: true];
                }
            }
        }
    }
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
