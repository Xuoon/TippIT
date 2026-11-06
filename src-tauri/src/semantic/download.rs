//! Modelldateien von Hugging Face laden: feste Revision, Größe und SHA-256
//! je Datei geprüft, erst danach umbenannt. Gesendet wird nur die Datei-URL,
//! nie etwas aus der Historie.

use std::future::Future;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

use super::models::{ModelFile, ModelSpec};

/// Markiert einen vollständig geprüften Modellordner; Inhalt ist die Revision.
const COMPLETE: &str = ".complete";
/// Länger darf der Server zwischen zwei Datenblöcken nicht schweigen.
const READ_TIMEOUT: Duration = Duration::from_secs(30);
/// So oft schauen wartende Netzaufrufe nach dem Abbruch.
const CANCEL_POLL: Duration = Duration::from_millis(200);

/// Marker passt zur Revision und alle Dateien haben die erwartete Größe.
/// Die Prüfsummen prüft `Embedder::load` beim Einlesen.
pub fn is_complete(spec: &ModelSpec, dir: &Path) -> bool {
    std::fs::read_to_string(dir.join(COMPLETE)).is_ok_and(|r| r == spec.revision)
        && spec.files.iter().all(|f| {
            std::fs::metadata(dir.join(f.name)).is_ok_and(|m| m.is_file() && m.len() == f.size)
        })
}

/// Marker verwerfen: der nächste `fetch` prüft jede Datei und lädt neu, was
/// nicht stimmt.
pub fn invalidate(dir: &Path) {
    let _ = std::fs::remove_file(dir.join(COMPLETE));
}

/// Lädt fehlende oder falsche Dateien nach `dir`. `progress(geladen, gesamt)`
/// in Bytes; `cancelled` bricht ab, auch während auf das Netz gewartet wird.
pub async fn fetch(
    spec: &ModelSpec,
    dir: &Path,
    cancelled: &(dyn Fn() -> bool + Sync),
    progress: impl Fn(u64, u64),
) -> Result<()> {
    if is_complete(spec, dir) {
        return Ok(());
    }
    std::fs::create_dir_all(dir)?;
    // Wie der Updater: rustls mit ring, ohne zweite TLS-Bibliothek.
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .user_agent(concat!("TippIT/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(READ_TIMEOUT)
        .build()?;
    let total = spec.download_bytes();
    let mut done = 0;
    for file in spec.files {
        if cancelled() {
            bail!("abgebrochen");
        }
        let dest = dir.join(file.name);
        if !(dest.is_file() && verify(&dest, file).is_ok()) {
            fetch_file(&client, &spec.url(file), file, &dest, cancelled, |n| {
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

/// `fut` abwarten, außer `cancelled` meldet vorher Abbruch.
async fn unless_cancelled<T>(
    fut: impl Future<Output = T>,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<T> {
    let watch = async {
        while !cancelled() {
            tokio::time::sleep(CANCEL_POLL).await;
        }
    };
    tokio::select! {
        out = fut => Ok(out),
        () = watch => bail!("abgebrochen"),
    }
}

async fn fetch_file(
    client: &reqwest::Client,
    url: &str,
    file: &ModelFile,
    dest: &Path,
    cancelled: &(dyn Fn() -> bool + Sync),
    progress: impl Fn(u64),
) -> Result<()> {
    let part = dest.with_extension("part");
    let result = async {
        let mut response = unless_cancelled(client.get(url).send(), cancelled)
            .await??
            .error_for_status()?;
        let mut out = std::io::BufWriter::new(std::fs::File::create(&part)?);
        let mut hasher = Sha256::new();
        let mut got: u64 = 0;
        while let Some(chunk) = unless_cancelled(response.chunk(), cancelled).await?? {
            got += chunk.len() as u64;
            if got > file.size {
                bail!("größer als erwartet");
            }
            hasher.update(&chunk);
            out.write_all(&chunk)?;
            progress(got);
        }
        out.flush()?;
        // Datei schließen, bevor sie umbenannt oder gelöscht wird (Windows).
        drop(out);
        check(got, &hex(&hasher.finalize()), file)
    }
    .await;
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

/// Größe und Prüfsumme gegen das Manifest; auch für schon gelesene Bytes.
pub fn check_bytes(bytes: &[u8], file: &ModelFile) -> Result<()> {
    check(bytes.len() as u64, &hex(&Sha256::digest(bytes)), file)
        .with_context(|| format!("{} beschädigt", file.name))
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

    const ABC: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
    const FILES: &[ModelFile] = &[ModelFile {
        name: "f",
        size: 3,
        sha256: ABC,
    }];
    const SPEC: ModelSpec = ModelSpec {
        repo: "r",
        revision: "rev",
        files: FILES,
        ram_bytes: 0,
    };

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
        assert!(verify(&path, &file(3, ABC)).is_ok());
        assert!(verify(&path, &file(4, ABC)).is_err());
        assert!(verify(&path, &file(3, &ABC[1..])).is_err());
        assert!(check_bytes(b"abc", &FILES[0]).is_ok());
        assert!(check_bytes(b"abd", &FILES[0]).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Der Marker allein reicht nicht: falsche Revision oder fehlende bzw.
    /// falsch große Dateien gelten als unvollständig.
    #[test]
    fn complete_needs_matching_marker_and_files() {
        let dir = std::env::temp_dir().join(format!("tippit-dl-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(COMPLETE), "rev").unwrap();
        assert!(!is_complete(&SPEC, &dir));
        std::fs::write(dir.join("f"), b"abc").unwrap();
        assert!(is_complete(&SPEC, &dir));
        std::fs::write(dir.join("f"), b"abcd").unwrap();
        assert!(!is_complete(&SPEC, &dir));
        std::fs::write(dir.join("f"), b"abc").unwrap();
        std::fs::write(dir.join(COMPLETE), "alt").unwrap();
        assert!(!is_complete(&SPEC, &dir));
        invalidate(&dir);
        assert!(!dir.join(COMPLETE).exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
