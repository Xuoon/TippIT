use serde::Serialize;

use crate::storage::settings::SyncSettings;

/// Welche Systemrichtlinie den Sync gerade pausiert (für die Status-Anzeige).
/// Unter macOS ist nur `EnergySaver` erkennbar (Stromsparmodus); WWAN-/
/// Datensparmodus-Profile gibt es dort nicht.
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
// macOS konstruiert nur EnergySaver — die Varianten bleiben trotzdem Teil
// des Frontend-Vertrags (Status-Anzeige) und der Windows-Implementierung.
#[cfg_attr(not(windows), allow(dead_code))]
pub enum BlockReason {
    EnergySaver,
    MobileData,
    DataSaver,
}

/// Prüft die aktuellen Systemrichtlinien direkt vor Hintergrundtransfers.
/// Nicht verfügbare Systeminformationen blockieren den Sync nicht.
pub fn block_reason(settings: &SyncSettings) -> Option<BlockReason> {
    crate::platform::block_reason(settings)
}
