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
    /// Monitor, auf dem die Historie öffnet.
    pub window_screen: HistoryScreen,
    /// Historie verstecken, sobald der Fokus in eine fremde App wechselt.
    /// Eigene Fenster und Dialoge (z. B. „Bild speichern") schließen sie nicht.
    pub close_on_blur: bool,
}

/// JSON: `{"kind":"cursor"}` | `{"kind":"primary"}` | `{"kind":"monitor","name":"…"}`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", content = "name", rename_all = "snake_case")]
pub enum HistoryScreen {
    /// Monitor unter dem Mauszeiger.
    Cursor,
    /// Hauptmonitor des Systems.
    Primary,
    /// Gewählter Monitor, Kennung aus `platform::monitor_id` (kein Anzeigename);
    /// ist er nicht angeschlossen, gilt `Cursor`.
    Monitor(String),
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
            window_screen: HistoryScreen::Cursor,
            close_on_blur: true,
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
        let settings = match std::fs::read_to_string(&file) {
            Ok(raw) => match serde_json::from_str::<Self>(&raw) {
                Ok(s) => s,
                Err(e) => {
                    // Nicht still überschreiben: sonst wären z. B. die ausgeschlossenen
                    // Apps beim nächsten Speichern weg, und Passwortmanager würden
                    // wieder erfasst.
                    let broken = file.with_extension("json.broken");
                    match std::fs::rename(&file, &broken) {
                        Ok(()) => tracing::error!(
                            "settings.json unlesbar ({e}), gesichert als {broken:?}; verwende Defaults"
                        ),
                        Err(re) => tracing::error!(
                            "settings.json unlesbar ({e}) und nicht sicherbar ({re}); verwende Defaults"
                        ),
                    }
                    Self::shipped_defaults()
                }
            },
            Err(_) => Self::shipped_defaults(),
        };
        settings.sanitized()
    }

    /// Nur harte Untergrenzen, die sonst Schaden anrichten (max_entries 0 ließe
    /// prune alles löschen, window_scale 0 ein unsichtbares Fenster). Die
    /// Komfortgrenzen der Slider bleiben allein im Frontend.
    pub fn sanitized(mut self) -> Self {
        self.history.max_entries = self.history.max_entries.max(1);
        self.history.window_scale = self.history.window_scale.max(10);
        self
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_screen_json_shapes() {
        let cases = [
            (HistoryScreen::Cursor, r#"{"kind":"cursor"}"#),
            (HistoryScreen::Primary, r#"{"kind":"primary"}"#),
            (
                HistoryScreen::Monitor("10ac-a0c4-0".into()),
                r#"{"kind":"monitor","name":"10ac-a0c4-0"}"#,
            ),
        ];
        for (value, json) in cases {
            assert_eq!(serde_json::to_string(&value).unwrap(), json);
            assert_eq!(serde_json::from_str::<HistoryScreen>(json).unwrap(), value);
        }
    }

    #[test]
    fn old_settings_get_new_defaults() {
        let s: Settings = serde_json::from_str(r#"{"history":{"max_entries":42}}"#).unwrap();
        assert_eq!(s.history.max_entries, 42);
        assert_eq!(s.history.window_screen, HistoryScreen::Cursor);
        assert!(s.history.close_on_blur);
    }

    #[test]
    fn sanitized_enforces_lower_bounds_only() {
        let mut s = Settings::default();
        s.history.max_entries = 0;
        s.history.window_scale = 0;
        let s = s.sanitized();
        assert_eq!(s.history.max_entries, 1);
        assert_eq!(s.history.window_scale, 10);

        let mut s = Settings::default();
        s.history.max_entries = 99_999;
        s.history.window_scale = 400;
        let s = s.sanitized();
        assert_eq!(s.history.max_entries, 99_999);
        assert_eq!(s.history.window_scale, 400);
    }
}
