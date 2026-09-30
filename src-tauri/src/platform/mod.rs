//! Plattform-Schicht: SÄMTLICHE OS-spezifischen Aufrufe (Win32 bzw.
//! AppKit/CoreGraphics) leben ausschließlich hier. Die Fachmodule (typing,
//! clipboard, windows_util, …) bleiben plattformneutral und rufen nur diese API.
//!
//! Beide Backends müssen dieselbe Semantik liefern; Fallstricke je Plattform
//! stehen in AGENTS.md.

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "macos")]
pub use mac::*;
#[cfg(target_os = "windows")]
mod win;
#[cfg(target_os = "windows")]
pub use win::*;

/// Fensterrahmen in logischen Einheiten des Monitors, auf den er sich bezieht
/// (physische Monitor-Koordinaten geteilt durch dessen Scale-Faktor).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Frame {
    /// Arbeitsbereich eines Monitors (ohne Taskleiste/Menüleiste/Dock).
    pub fn work_area(monitor: &tauri::Monitor) -> Self {
        let sf = monitor.scale_factor();
        let area = monitor.work_area();
        Self {
            x: f64::from(area.position.x) / sf,
            y: f64::from(area.position.y) / sf,
            w: f64::from(area.size.width) / sf,
            h: f64::from(area.size.height) / sf,
        }
    }
}

/// Tasten, die als echter Tastendruck statt als Unicode-Eingabe gesendet werden
/// (viele Anwendungen ignorieren ein reines Unicode-LF/-Tab).
#[derive(Clone, Copy)]
pub enum SpecialKey {
    Return,
    Tab,
}

/// Eine erkannte OCR-Textzeile mit Position (für markierbares Text-Overlay).
/// Koordinaten sind normalisiert [0,1] mit **Ursprung oben-links** (macOS flippt
/// dafür Visions unten-links-System), sodass das Frontend sie direkt als
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

/// Speicherort des laufenden App-Bundles. Translokiert oder vom DMG gestartet
/// schreiben Updater und Autostart an einen Wegwerfpfad.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    target_os = "windows",
    expect(dead_code, reason = "Nur macOS ermittelt den Speicherort")
)]
pub enum InstallLocation {
    Applications,
    Translocated,
    DiskImage,
    Downloads,
    Other,
}

impl InstallLocation {
    /// Wert für das Frontend (`PermissionStatus.location`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Applications => "applications",
            Self::Translocated => "translocated",
            Self::DiskImage => "dmg",
            Self::Downloads => "downloads",
            Self::Other => "other",
        }
    }

    /// Orte, an denen Updater und Autostart ins Leere schreiben würden.
    pub fn is_transient(self) -> bool {
        matches!(self, Self::Translocated | Self::DiskImage)
    }
}

/// Installationsdiagnose für den Einstellungs-Tab „Berechtigungen".
#[derive(Clone, Debug)]
pub struct InstallInfo {
    pub location: InstallLocation,
    /// Pfad des App-Bundles (macOS) bzw. leer (Windows).
    pub bundle_path: String,
}
