use serde::{Deserialize, Serialize};

use super::paths::AppPaths;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TypingMode {
    /// Alles in einem Rutsch (Parität zur AutoIt-Version).
    Bulk,
    /// Zeichenweise mit Verzögerung — für RDP/Citrix-Felder, die schnelle Eingaben verschlucken.
    PerChar,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TypingSettings {
    /// Verzögerung vor dem Tippen in ms (Zeit, um das Zielfeld zu fokussieren).
    pub pre_delay_ms: u64,
    pub mode: TypingMode,
    /// Verzögerung zwischen Zeichen im PerChar-Modus (ms).
    pub char_delay_ms: u64,
    /// Whitespace vorn/hinten vor dem Tippen entfernen.
    pub trim: bool,
}

impl Default for TypingSettings {
    fn default() -> Self {
        Self {
            pre_delay_ms: 1000,
            // Zeichenweise als Standard: funktioniert überall zuverlässig (auch RDP/Citrix).
            mode: TypingMode::PerChar,
            char_delay_ms: 15,
            trim: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct HistorySettings {
    pub max_entries: u32,
    pub capture_images: bool,
    pub capture_files: bool,
    /// Formatierung (Clipboard-HTML) mitspeichern — sanitisiert, siehe `clipboard::html`.
    pub capture_html: bool,
    /// Einträge automatisch in den Papierkorb legen, wenn sie älter sind als
    /// so viele Tage. 0 = aus.
    pub retention_days: u32,
    /// Quellanwendungen, aus denen NICHTS erfasst wird (Passwortmanager, Banking).
    /// Ein Eintrag greift, wenn er der Plattform-ID (Bundle-ID bzw. exe-Pfad)
    /// ODER dem Anzeigenamen der App entspricht — Groß-/Kleinschreibung egal.
    pub excluded_apps: Vec<String>,
    /// Größe des Historie-Fensters in Prozent der Basisgröße (100 = Standard).
    pub window_scale: u32,
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self {
            max_entries: 500,
            capture_images: true,
            capture_files: true,
            capture_html: true,
            retention_days: 0,
            excluded_apps: Vec::new(),
            window_scale: 100,
        }
    }
}

/// Abbrechen des Tippens ist bewusst KEIN Setting: immer ESC, nur während
/// eines Tipp-Vorgangs global registriert (`typing::EscCancelGuard`).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeySettings {
    pub paste: String,
    pub history: String,
}

impl Default for HotkeySettings {
    fn default() -> Self {
        let (paste, history) = crate::platform::default_hotkeys();
        Self {
            paste: paste.into(),
            history: history.into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub sounds: bool,
    pub theme: String,
    pub hotkeys: HotkeySettings,
    pub typing: TypingSettings,
    pub history: HistorySettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            sounds: true,
            theme: "system".into(),
            hotkeys: HotkeySettings::default(),
            typing: TypingSettings::default(),
            history: HistorySettings::default(),
        }
    }
}

impl Settings {
    /// Auslieferungs-Standardwerte: in der EXE eingebettete defaults.json
    /// über den Code-Defaults.
    pub fn shipped_defaults() -> Self {
        serde_json::from_str(include_str!("../../defaults.json")).unwrap_or_else(|e| {
            tracing::warn!("defaults.json unlesbar ({e}), verwende Code-Defaults");
            Self::default()
        })
    }

    pub fn load(paths: &AppPaths) -> Self {
        let file = paths.settings_file();
        match std::fs::read_to_string(&file) {
            Ok(raw) => match serde_json::from_str(&raw) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("settings.json unlesbar ({e}), verwende Defaults");
                    Self::shipped_defaults()
                }
            },
            Err(_) => Self::shipped_defaults(),
        }
    }

    /// Atomarer Write: Temp-Datei + Rename, damit ein Absturz nie eine halbe Datei hinterlässt.
    pub fn save(&self, paths: &AppPaths) -> anyhow::Result<()> {
        let file = paths.settings_file();
        let tmp = file.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(&tmp, &file)?;
        Ok(())
    }
}
