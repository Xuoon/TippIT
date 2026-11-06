//! Modelldateien von Hugging Face laden: feste Revision, Größe und SHA-256
//! je Datei geprüft, erst danach umbenannt. Gesendet wird nur die Datei-URL,
//! nie etwas aus der Historie.

use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

use super::models::{ModelFile, ModelSpec};

/// Markiert einen vollständig geprüften Modellordner.
const COMPLETE: &str = ".complete";

pub fn is_complete(dir: &Path) -> bool {
    dir.join(COMPLETE).is_file()
}

/// Lädt fehlende Dateien nach `dir`. `progress(geladen, gesamt)` in Bytes;
/// `cancel` bricht zwischen zwei Blöcken ab.
pub async fn fetch(
    spec: &ModelSpec,
    dir: &Path,
    cancel: &AtomicBool,
    progress: impl Fn(u64, u64),
) -> Result<()> {
    if is_complete(dir) {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    // Wie der Updater: rustls mit ring, ohne zweite TLS-Bibliothek.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .user_agent(concat!("TippIT/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(std::time::Duration::from_secs(20))
        .build()?;
    let total = spec.download_bytes();
    let mut done = 0;
    for file in spec.files {
        let dest = dir.join(file.name);
        if !(dest.is_file() && verify(&dest, file).is_ok()) {
            fetch_file(&client, &spec.url(file), file, &dest, cancel, |n| {
                progress(done + n, total);
            })
            .await
            .with_context(|| format!("{} laden", file.name))?;
        }
        done += file.size;
        progress(done, total);
    }
    std::fs::write(dir.join(COMPLETE), spec.revision)?;
    Ok(())
}

async fn fetch_file(
    client: &reqwest::Client,
    url: &str,
    file: &ModelFile,
    dest: &Path,
    cancel: &AtomicBool,
    progress: impl Fn(u64),
) -> Result<()> {
    let part = dest.with_extension("part");
    let mut response = client.get(url).send().await?.error_for_status()?;
    let mut out = std::io::BufWriter::new(std::fs::File::create(&part)?);
    let mut hasher = Sha256::new();
    let mut got: u64 = 0;
    let result = async {
        while let Some(chunk) = response.chunk().await? {
            if cancel.load(Ordering::Relaxed) {
                bail!("abgebrochen");
            }
            got += chunk.len() as u64;
            if got > file.size {
                bail!("größer als erwartet");
            }
            hasher.update(&chunk);
            out.write_all(&chunk)?;
            progress(got);
        }
        out.flush()?;
        check(got, &hex(&hasher.finalize()), file)
    }
    .await;
    drop(out);
    match result {
        Ok(()) => Ok(std::fs::rename(&part, dest)?),
        Err(e) => {
            let _ = std::fs::remove_file(&part);
            Err(e)
        }
    }
}

/// Bereits vorhandene Datei prüfen (Abbruch nach dem Umbenennen, alte Reste).
fn verify(path: &Path, file: &ModelFile) -> Result<()> {
    let mut f = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let size = std::io::copy(&mut f, &mut hasher)?;
    check(size, &hex(&hasher.finalize()), file)
}

fn check(size: u64, sha256: &str, file: &ModelFile) -> Result<()> {
    if size != file.size {
        bail!("Größe {size} statt {}", file.size);
    }
    if sha256 != file.sha256 {
        bail!("Prüfsumme stimmt nicht");
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    data_encoding::HEXLOWER.encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_checks_size_and_hash() {
        let dir = std::env::temp_dir().join(format!("tippit-dl-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("f");
        std::fs::write(&path, b"abc").unwrap();
        let file = |size, sha256| ModelFile {
            name: "f",
            size,
            sha256,
        };
        const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(verify(&path, &file(3, ABC)).is_ok());
        assert!(verify(&path, &file(4, ABC)).is_err());
        assert!(verify(&path, &file(3, &ABC[1..])).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
