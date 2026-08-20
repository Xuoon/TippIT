use std::path::PathBuf;

/// Alle TippIT-Daten leben unter `~/.labi/tippit/` (Windows: %USERPROFILE%).
#[derive(Clone, Debug)]
pub struct AppPaths {
    pub root: PathBuf,
}

impl AppPaths {
    pub fn resolve() -> anyhow::Result<Self> {
        let home =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("kein Home-Verzeichnis gefunden"))?;
        let base = home.join(".labi");
        let root = base.join("tippit");
        std::fs::create_dir_all(root.join("logs"))?;
        // Der Punkt macht den Ordner für viele Werkzeuge unauffällig; unter
        // Windows sorgt zusätzlich das Hidden-Attribut für das erwartete Verhalten.
        if let Err(e) = crate::platform::hide_directory(&base) {
            tracing::warn!(".labi konnte nicht als ausgeblendet markiert werden: {e}");
        }
        Ok(Self { root })
    }

    pub fn settings_file(&self) -> PathBuf {
        self.root.join("settings.json")
    }

    pub fn key_file(&self) -> PathBuf {
        self.root.join("key.bin")
    }

    /// Zwei-Phasen-Schlüsselrotation: das neue Secret liegt hier, bis die DB
    /// vollständig umgeschlüsselt ist (Recovery: `lib.rs::resolve_secret`).
    pub fn key_file_pending(&self) -> PathBuf {
        self.root.join("key.bin.new")
    }

    pub fn db_file(&self) -> PathBuf {
        self.root.join("history.db")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    /// Disk-Cache für Quellanwendungs-Icons (32×32 PNG).
    pub fn app_icons_dir(&self) -> PathBuf {
        self.root.join("app-icons")
    }

    /// Dateipfad für App-Icon: sha256(app_id)[0..16].png — nie rohe User-Segmente joinen.
    pub fn app_icon_file(&self, app_id: &str) -> PathBuf {
        use sha2::{Digest, Sha256};
        let digest = Sha256::digest(app_id.as_bytes());
        let hex = data_encoding::HEXLOWER.encode(&digest[..8]);
        self.app_icons_dir().join(format!("{hex}.png"))
    }
}
