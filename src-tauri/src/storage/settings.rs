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
    /// Monitor, auf dem die Historie öffnet, solange keine verschobene
    /// Position gilt (und als Rückfall, wenn deren Monitor fehlt).
    pub window_screen: HistoryScreen,
    /// Zuletzt per Ziehen gewählte Position; hat Vorrang vor `window_screen`.
    pub window_position: Option<WindowPosition>,
    /// Fensterhöhe in Prozent des Arbeitsbereichs; die Breite folgt dem
    /// Seitenverhältnis. Relativ, damit das Fenster auf 4K und 1080p gleich wirkt.
    pub window_size: u32,
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

/// Verschobene Position: Monitor-Kennung (`platform::monitor_id`) plus Lage im
/// Arbeitsbereich als Anteil des freien Raums (0 = links/oben, 1 = rechts/unten).
/// Anteile statt Pixel überstehen Auflösungs- und Skalierungswechsel.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WindowPosition {
    pub monitor: String,
    pub x: f64,
    pub y: f64,
}

impl WindowPosition {
    fn sanitized(self) -> Option<Self> {
        let unit = |v: f64| v.is_finite().then(|| v.clamp(0.0, 1.0));
        Some(Self {
            x: unit(self.x)?,
            y: unit(self.y)?,
            monitor: self.monitor,
        })
        .filter(|p| !p.monitor.is_empty())
    }
}

/// Grenzen von `history.window_size` (Prozent der Arbeitsbereichshöhe).
pub const WINDOW_SIZE_MIN: u32 = 40;
pub const WINDOW_SIZE_MAX: u32 = 90;
/// Wirkt auf einem 1080p-Monitor wie die frühere feste Größe (600 von ~1040 px).
const WINDOW_SIZE_DEFAULT: u32 = 58;

impl Default for HistorySettings {
    fn default() -> Self {
        Self {
            max_entries: 500,
            capture_images: true,
            capture_files: true,
            capture_html: true,
            retention_days: 0,
            excluded_apps: Vec::new(),
            window_screen: HistoryScreen::Cursor,
            window_position: None,
            window_size: WINDOW_SIZE_DEFAULT,
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
            Ok(raw) => match Self::parse(&raw) {
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

    /// Gespeicherte Datei lesen und Felder älterer Versionen übernehmen.
    fn parse(raw: &str) -> serde_json::Result<Self> {
        let mut value: serde_json::Value = serde_json::from_str(raw)?;
        migrate(&mut value);
        serde_json::from_value(value)
    }

    /// Harte Grenzen gegen Werte, die sonst Schaden anrichten (max_entries 0
    /// ließe prune alles löschen, eine Fenstergröße außerhalb der Grenzen wäre
    /// unbedienbar, eine Stunde Verzögerung sähe wie ein Hänger aus). Die
    /// Komfortgrenzen der Slider bleiben im Frontend.
    pub fn sanitized(mut self) -> Self {
        let h = &mut self.history;
        h.max_entries = h.max_entries.max(1);
        h.window_size = h.window_size.clamp(WINDOW_SIZE_MIN, WINDOW_SIZE_MAX);
        h.window_position = h.window_position.take().and_then(WindowPosition::sanitized);
        let mut seen = std::collections::HashSet::new();
        h.excluded_apps = std::mem::take(&mut h.excluded_apps)
            .into_iter()
            .map(|a| a.trim().to_owned())
            .filter(|a| !a.is_empty() && seen.insert(a.to_lowercase()))
            .collect();
        self.typing.pre_delay_ms = self.typing.pre_delay_ms.min(10_000);
        self.typing.char_delay_ms = self.typing.char_delay_ms.min(1_000);
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

/// Felder älterer Versionen auf das aktuelle Format abbilden. Bis 2.1 hieß die
/// Fenstergröße `history.window_scale` (Prozent einer festen Basisgröße, 100 =
/// Standard); sie wird zum gleichen Anteil des neuen Standards.
fn migrate(value: &mut serde_json::Value) {
    let Some(history) = value
        .get_mut("history")
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    let Some(scale) = history.remove("window_scale") else {
        return;
    };
    if history.contains_key("window_size") {
        return;
    }
    if let Some(scale) = scale.as_f64() {
        let size = (f64::from(WINDOW_SIZE_DEFAULT) * scale / 100.0).round();
        history.insert("window_size".into(), serde_json::json!(size as u32));
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
    fn sanitized_enforces_hard_bounds() {
        let mut s = Settings::default();
        s.history.max_entries = 0;
        s.history.window_size = 0;
        s.typing.pre_delay_ms = 3_600_000;
        let s = s.sanitized();
        assert_eq!(s.history.max_entries, 1);
        assert_eq!(s.history.window_size, WINDOW_SIZE_MIN);
        assert_eq!(s.typing.pre_delay_ms, 10_000);

        let mut s = Settings::default();
        s.history.max_entries = 99_999;
        s.history.window_size = 400;
        let s = s.sanitized();
        assert_eq!(s.history.max_entries, 99_999);
        assert_eq!(s.history.window_size, WINDOW_SIZE_MAX);
    }

    #[test]
    fn sanitized_cleans_position_and_excluded_apps() {
        let mut s = Settings::default();
        s.history.window_position = Some(WindowPosition {
            monitor: "m".into(),
            x: 1.7,
            y: -0.2,
        });
        s.history.excluded_apps = vec![" KeePass ".into(), "keepass".into(), "  ".into()];
        let s = s.sanitized();
        let p = s.history.window_position.unwrap();
        assert_eq!((p.x, p.y), (1.0, 0.0));
        assert_eq!(s.history.excluded_apps, ["KeePass"]);

        let mut s = Settings::default();
        s.history.window_position = Some(WindowPosition {
            monitor: "m".into(),
            x: f64::NAN,
            y: 0.5,
        });
        assert_eq!(s.sanitized().history.window_position, None);
    }

    #[test]
    fn legacy_window_scale_becomes_relative_size() {
        let s = Settings::parse(r#"{"history":{"window_scale":150,"max_entries":7}}"#).unwrap();
        assert_eq!(s.history.window_size, 87);
        assert_eq!(s.history.max_entries, 7);
        let s = Settings::parse(r#"{"history":{"window_scale":100}}"#).unwrap();
        assert_eq!(s.history.window_size, WINDOW_SIZE_DEFAULT);
        // Ein schon vorhandener neuer Wert gewinnt.
        let s = Settings::parse(r#"{"history":{"window_scale":150,"window_size":50}}"#).unwrap();
        assert_eq!(s.history.window_size, 50);
        let saved = serde_json::to_string(&s).unwrap();
        assert!(!saved.contains("window_scale"));
    }

    #[test]
    fn settings_file_from_main_loads() {
        let raw = r#"{
            "sounds": false,
            "theme": "light",
            "hotkeys": {"paste": "Ctrl+E", "history": "Ctrl+Shift+E"},
            "typing": {"pre_delay_ms": 500, "mode": "bulk", "char_delay_ms": 20, "trim": false},
            "history": {
                "max_entries": 300, "capture_images": false, "capture_files": true,
                "capture_html": false, "retention_days": 30,
                "excluded_apps": ["KeePassXC"], "window_scale": 120,
                "window_screen": {"kind": "monitor", "name": "10ac-a0c4-0"},
                "close_on_blur": false
            }
        }"#;
        let s = Settings::parse(raw).unwrap().sanitized();
        assert!(!s.sounds);
        assert_eq!(s.typing.mode, TypingMode::Bulk);
        assert_eq!(s.typing.pre_delay_ms, 500);
        assert_eq!(s.history.max_entries, 300);
        assert_eq!(s.history.retention_days, 30);
        assert_eq!(s.history.excluded_apps, ["KeePassXC"]);
        assert_eq!(s.history.window_size, 70);
        assert_eq!(
            s.history.window_screen,
            HistoryScreen::Monitor("10ac-a0c4-0".into())
        );
        assert_eq!(s.history.window_position, None);
        assert!(!s.history.close_on_blur);
    }

    #[test]
    fn window_position_round_trips() {
        let json = r#"{"history":{"window_position":{"monitor":"10ac-a0c4-0","x":0.25,"y":1.0}}}"#;
        let s = Settings::parse(json).unwrap();
        assert_eq!(
            s.history.window_position,
            Some(WindowPosition {
                monitor: "10ac-a0c4-0".into(),
                x: 0.25,
                y: 1.0
            })
        );
        assert_eq!(Settings::parse("{}").unwrap().history.window_position, None);
    }

    #[test]
    fn shipped_defaults_parse() {
        let s = Settings::shipped_defaults();
        assert_eq!(s.typing.mode, TypingMode::PerChar);
        assert_eq!(s.history.window_size, WINDOW_SIZE_DEFAULT);
    }
}
