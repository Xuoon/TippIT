use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows::core::PCWSTR;
use windows::Win32::Storage::FileSystem::{
    GetFileAttributesW, SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN, FILE_FLAGS_AND_ATTRIBUTES,
    INVALID_FILE_ATTRIBUTES,
};

/// Alle TippIT-Daten leben unter %USERPROFILE%\.labi\tippit\.
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
        if let Err(e) = hide_directory(&base) {
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

    pub fn sync_file(&self) -> PathBuf {
        self.root.join("sync.json")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }
}

/// Hidden-Attribut direkt per Win32 setzen — kein `attrib`-Kindprozess, der im
/// GUI-Subsystem ein Konsolenfenster aufblitzen ließe. Bestehende Attribute
/// bleiben erhalten (Semantik von `attrib +H`).
fn hide_directory(path: &Path) -> windows::core::Result<()> {
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let path = PCWSTR(wide.as_ptr());
    let attrs = match unsafe { GetFileAttributesW(path) } {
        INVALID_FILE_ATTRIBUTES => FILE_ATTRIBUTE_HIDDEN,
        attrs => FILE_FLAGS_AND_ATTRIBUTES(attrs) | FILE_ATTRIBUTE_HIDDEN,
    };
    unsafe { SetFileAttributesW(path, attrs) }
}
