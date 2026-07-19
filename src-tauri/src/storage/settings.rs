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
}

impl Default for HistorySettings {
    fn default() -> Self {
        Self {
            max_entries: 500,
            capture_images: true,
            capture_files: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeySettings {
    pub paste: String,
    pub history: String,
}

impl Default for HotkeySettings {
    fn default() -> Self {
        Self {
            paste: "ctrl+e".into(),
            history: "ctrl+shift+e".into(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SyncSettings {
    pub deployment_url: String,
    pub sync_text: bool,
    pub sync_settings: bool,
    pub sync_images: bool,
    /// Maximale Bildgröße für den Sync in Bytes.
    pub image_max_bytes: u64,
    /// 0 = sofort, ansonsten gebündelter Upload/Pull in diesem Minutenabstand.
    pub interval_minutes: u64,
    pub allow_mobile_data: bool,
    pub allow_energy_saver: bool,
    pub allow_data_saver: bool,
}

impl Default for SyncSettings {
    fn default() -> Self {
        Self {
            deployment_url: String::new(),
            sync_text: true,
            sync_settings: true,
            sync_images: false,
            image_max_bytes: 1024 * 1024,
            // 0 = sofort: entspricht dem Verhalten der ausgelieferten 0.2.x-Versionen;
            // ein anderer Default würde Bestandsinstallationen still auf Batch-Sync umstellen.
            interval_minutes: 0,
            allow_mobile_data: false,
            allow_energy_saver: false,
            allow_data_saver: false,
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
    pub sync: SyncSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            sounds: true,
            theme: "system".into(),
            hotkeys: HotkeySettings::default(),
            typing: TypingSettings::default(),
            history: HistorySettings::default(),
            sync: SyncSettings::default(),
        }
    }
}

impl Settings {
    /// Auslieferungs-Standardwerte: in der EXE eingebettete defaults.json
    /// (z. B. vorausgefüllte Convex-URL) über den Code-Defaults.
    pub fn shipped_defaults() -> Self {
        serde_json::from_str(include_str!("../../defaults.json")).unwrap_or_else(|e| {
            tracing::warn!("defaults.json unlesbar ({e}), verwende Code-Defaults");
            Self::default()
        })
    }

    pub fn load(paths: &AppPaths) -> Self {
        let file = paths.settings_file();
        let mut settings = match std::fs::read_to_string(&file) {
            Ok(raw) => match serde_json::from_str(&raw) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("settings.json unlesbar ({e}), verwende Defaults");
                    Self::shipped_defaults()
                }
            },
            Err(_) => Self::shipped_defaults(),
        };
        // Leere URL = nie konfiguriert → mit Auslieferungs-Default vorbefüllen.
        if settings.sync.deployment_url.trim().is_empty() {
            settings.sync.deployment_url = Self::shipped_defaults().sync.deployment_url;
        }
        settings
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
