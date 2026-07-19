use serde::Serialize;

use crate::storage::settings::SyncSettings;

use windows::Networking::Connectivity::NetworkInformation;
use windows::System::Power::{EnergySaverStatus, PowerManager};

/// Welche Windows-Richtlinie den Sync gerade pausiert (für die Status-Anzeige).
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockReason {
    EnergySaver,
    MobileData,
    DataSaver,
}

/// Prüft die aktuellen Windows-Richtlinien direkt vor Hintergrundtransfers.
/// Nicht verfügbare Systeminformationen blockieren den Sync nicht.
pub fn block_reason(settings: &SyncSettings) -> Option<BlockReason> {
    if !settings.allow_energy_saver
        && matches!(PowerManager::EnergySaverStatus(), Ok(EnergySaverStatus::On))
    {
        return Some(BlockReason::EnergySaver);
    }

    let Ok(profile) = NetworkInformation::GetInternetConnectionProfile() else {
        return None;
    };
    if !settings.allow_mobile_data && profile.IsWwanConnectionProfile().unwrap_or(false) {
        return Some(BlockReason::MobileData);
    }

    if !settings.allow_data_saver {
        if let Ok(cost) = profile.GetConnectionCost() {
            if cost.BackgroundDataUsageRestricted().unwrap_or(false)
                || cost.OverDataLimit().unwrap_or(false)
            {
                return Some(BlockReason::DataSaver);
            }
        }
    }

    None
}
