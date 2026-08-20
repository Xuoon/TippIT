use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
// aes-gcm bewusst auf der 0.10-Serie: klassische GenericArray-API.
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::paths::AppPaths;

const AAD_VERSION: u8 = 0x01;

/// Das 32-Byte-Master-Secret dieses Geräts. Verlässt das Gerät nie —
/// gespeichert wird es plattformgeschützt in key.bin (Windows: DPAPI).
pub struct Secret(Zeroizing<[u8; 32]>);

/// Aus dem Secret abgeleiteter Datenschlüssel.
#[derive(Clone)]
pub struct CryptoKeys {
    pub enc: Zeroizing<[u8; 32]>,
}

impl Secret {
    pub fn generate() -> anyhow::Result<Self> {
        let mut buf = Zeroizing::new([0u8; 32]);
        getrandom_fill(buf.as_mut())?;
        Ok(Self(buf))
    }

    pub fn derive_keys(&self) -> CryptoKeys {
        let hk = Hkdf::<Sha256>::new(None, self.0.as_ref());
        let mut enc = Zeroizing::new([0u8; 32]);
        hk.expand(b"tippit/v1/enc", enc.as_mut())
            .expect("HKDF expand");
        CryptoKeys { enc }
    }

    /// Secret plattformgeschützt in key.bin ablegen (atomar: tmp + rename).
    /// Windows: DPAPI (User-Scope); macOS: 0600 + FileVault (s. platform::protect).
    pub fn store(&self, paths: &AppPaths) -> anyhow::Result<()> {
        self.write_wrapped(&paths.key_file())
    }

    /// key.bin.new einer in einer früheren Version abgebrochenen Rotation
    /// übernehmen (atomar). TippIT rotiert selbst nicht mehr — der Pfad existiert
    /// nur, um solche Altbestände beim Start zu heilen.
    pub fn promote_pending(paths: &AppPaths) -> anyhow::Result<()> {
        std::fs::rename(paths.key_file_pending(), paths.key_file())?;
        Ok(())
    }

    pub fn remove_pending(paths: &AppPaths) {
        let _ = std::fs::remove_file(paths.key_file_pending());
    }

    fn write_wrapped(&self, file: &std::path::Path) -> anyhow::Result<()> {
        let wrapped = crate::platform::protect(self.0.as_ref())?;
        let name = file
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("key.bin");
        let tmp = file.with_file_name(format!("{name}.tmp"));
        std::fs::write(&tmp, wrapped)?;
        crate::platform::secure_key_file(&tmp)?;
        std::fs::rename(&tmp, file)?;
        Ok(())
    }

    pub fn load(paths: &AppPaths) -> anyhow::Result<Option<Self>> {
        Self::load_from(&paths.key_file())
    }

    /// key.bin.new einer ggf. abgebrochenen Rotation (s. `lib.rs::resolve_secret`).
    pub fn load_pending(paths: &AppPaths) -> anyhow::Result<Option<Self>> {
        Self::load_from(&paths.key_file_pending())
    }

    fn load_from(file: &std::path::Path) -> anyhow::Result<Option<Self>> {
        if !file.exists() {
            return Ok(None);
        }
        let wrapped = std::fs::read(file)?;
        let raw = crate::platform::unprotect(&wrapped)?;
        if raw.len() != 32 {
            anyhow::bail!("Schlüsseldatei hat unerwartete Länge");
        }
        let mut secret = Zeroizing::new([0u8; 32]);
        secret.copy_from_slice(&raw);
        Ok(Some(Self(secret)))
    }

    pub fn load_or_create(paths: &AppPaths) -> anyhow::Result<Self> {
        if let Some(s) = Self::load(paths)? {
            return Ok(s);
        }
        let s = Self::generate()?;
        s.store(paths)?;
        Ok(s)
    }
}

pub fn getrandom_fill(buf: &mut [u8]) -> anyhow::Result<()> {
    getrandom::fill(buf).map_err(|e| anyhow::anyhow!("OS-RNG fehlgeschlagen: {e}"))
}

/// AAD bindet den Ciphertext an seine Zeile — verhindert Vertauschen von Blobs.
fn aad(uuid: &str, kind: u8) -> Vec<u8> {
    let mut v = Vec::with_capacity(uuid.len() + 2);
    v.push(AAD_VERSION);
    v.push(kind);
    v.extend_from_slice(uuid.as_bytes());
    v
}

/// nonce(12) ‖ AES-256-GCM(plaintext) mit frischer Zufalls-Nonce.
pub fn encrypt(
    keys: &CryptoKeys,
    uuid: &str,
    kind: u8,
    plaintext: &[u8],
) -> anyhow::Result<Vec<u8>> {
    let cipher = Aes256Gcm::new(keys.enc.as_ref().into());
    let mut nonce = [0u8; 12];
    getrandom_fill(&mut nonce)?;
    let ct = cipher
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: &aad(uuid, kind),
            },
        )
        .map_err(|_| anyhow::anyhow!("Verschlüsselung fehlgeschlagen"))?;
    let mut out = Vec::with_capacity(12 + ct.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

pub fn decrypt(keys: &CryptoKeys, uuid: &str, kind: u8, blob: &[u8]) -> anyhow::Result<Vec<u8>> {
    if blob.len() < 13 {
        anyhow::bail!("Ciphertext zu kurz");
    }
    let (nonce, ct) = blob.split_at(12);
    let cipher = Aes256Gcm::new(keys.enc.as_ref().into());
    cipher
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: ct,
                aad: &aad(uuid, kind),
            },
        )
        .map_err(|_| anyhow::anyhow!("Entschlüsselung fehlgeschlagen (falscher Schlüssel?)"))
}

/// Wie [`encrypt`], aber mit frei gewählter AAD und Nonce — für den Export, wo
/// die AAD der Dateikopf ist und die Nonce dort bereits im Klartext steht.
/// Liefert nur den Ciphertext (ohne vorangestellte Nonce).
pub fn encrypt_with_aad(
    keys: &CryptoKeys,
    aad: &[u8],
    nonce: &[u8; 12],
    plaintext: &[u8],
) -> anyhow::Result<Vec<u8>> {
    Aes256Gcm::new(keys.enc.as_ref().into())
        .encrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .map_err(|_| anyhow::anyhow!("Verschlüsselung fehlgeschlagen"))
}

pub fn decrypt_with_aad(
    keys: &CryptoKeys,
    aad: &[u8],
    nonce: &[u8],
    ciphertext: &[u8],
) -> anyhow::Result<Vec<u8>> {
    Aes256Gcm::new(keys.enc.as_ref().into())
        .decrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: ciphertext,
                aad,
            },
        )
        .map_err(|_| anyhow::anyhow!("Entschlüsselung fehlgeschlagen"))
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_roundtrip_and_aad_binding() {
        let secret = Secret::generate().unwrap();
        let keys = secret.derive_keys();
        let ct = encrypt(&keys, "uuid-1", 0, b"geheim").unwrap();
        assert_eq!(decrypt(&keys, "uuid-1", 0, &ct).unwrap(), b"geheim");
        // andere uuid/kind → AAD-Mismatch
        assert!(decrypt(&keys, "uuid-2", 0, &ct).is_err());
        assert!(decrypt(&keys, "uuid-1", 1, &ct).is_err());
    }

    #[test]
    fn aad_variant_binds_header() {
        let keys = Secret::generate().unwrap().derive_keys();
        let nonce = [7u8; 12];
        let ct = encrypt_with_aad(&keys, b"kopf", &nonce, b"sicherung").unwrap();
        assert_eq!(
            decrypt_with_aad(&keys, b"kopf", &nonce, &ct).unwrap(),
            b"sicherung"
        );
        // Verändertes Salt/Iterationen im Kopf → Entschlüsselung scheitert.
        assert!(decrypt_with_aad(&keys, b"kopX", &nonce, &ct).is_err());
    }
}
