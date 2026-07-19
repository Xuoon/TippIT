use std::path::PathBuf;

/// Alle TippIT-Daten leben unter %USERPROFILE%\.labit\tippit\
#[derive(Clone, Debug)]
pub struct AppPaths {
    pub root: PathBuf,
}

impl AppPaths {
    pub fn resolve() -> anyhow::Result<Self> {
        let home =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("kein Home-Verzeichnis gefunden"))?;
        let root = home.join(".labit").join("tippit");
        std::fs::create_dir_all(root.join("logs"))?;
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

    pub fn sync_file(&self) -> PathBuf {
        self.root.join("sync.json")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }
}
