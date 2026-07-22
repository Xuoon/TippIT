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
