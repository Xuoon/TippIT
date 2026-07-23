//! Plattform-Schicht: SÄMTLICHE OS-spezifischen Aufrufe (Win32 bzw.
//! AppKit/CoreGraphics) leben ausschließlich hier. Die Fachmodule (typing,
//! clipboard, windows_util, …) bleiben plattformneutral und rufen nur diese API.
//!
//! Beide Backends müssen dieselbe Semantik liefern — Details und Fallstricke
//! je Plattform: .claude/rules/windows.md und .claude/rules/macos.md.

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
pub use mac::*;
#[cfg(target_os = "windows")]
mod win;
#[cfg(target_os = "windows")]
pub use win::*;

/// Tasten, die als echter Tastendruck statt als Unicode-Eingabe gesendet werden
/// (viele Anwendungen ignorieren ein reines Unicode-LF/-Tab).
#[derive(Clone, Copy)]
pub enum SpecialKey {
    Return,
    Tab,
}

/// Eine erkannte OCR-Textzeile mit Position (für markierbares Text-Overlay).
/// Koordinaten sind normalisiert [0,1] mit **Ursprung oben-links** (bereits aus
/// Visions unten-links-System geflippt), sodass das Frontend sie direkt als
/// CSS-Prozente verwenden kann.
#[derive(Clone, Debug)]
pub struct OcrLine {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// Vordergrund-App beim Clipboard-Capture (Quell-Anwendung).
/// `None` von [`foreground_app_info`] = transient/unbekannt (nicht wipen).
/// `is_self` = TippIT selbst (Source Clear bei Duplikat).
#[derive(Clone, Debug)]
pub struct ForegroundApp {
    /// Cache-Schlüssel: Bundle-ID (macOS) bzw. lowercase full exe path (Windows).
    pub id: String,
    /// Anzeigename (localizedName / FileDescription / Dateiname).
    pub name: String,
    /// Optional PNG 32×32; Name ohne Icon ist Erfolg.
    pub icon_png: Option<Vec<u8>>,
    /// true wenn frontmost = TippIT.
    pub is_self: bool,
}

// `foreground_app_info` und `ocr_png` (→ Vec<OcrLine>) sind in mac.rs / win.rs
// implementiert.
