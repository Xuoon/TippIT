use aes_gcm::aead::{Aead, AeadInPlace, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce, Tag};
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
        crate::platform::write_key_file(&tmp, &wrapped)?;
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

/// Wie [`encrypt`], aber mit frei gewählter AAD und Nonce und direkt im Puffer —
/// für den Export, wo die AAD der Dateikopf ist, die Nonce dort im Klartext
/// steht und die Historie nicht ein zweites Mal als Kopie im Speicher liegen
/// soll. `buf` ‖ Rückgabe ergibt byte-gleich dasselbe wie `Aead::encrypt`.
pub fn encrypt_in_place_with_aad(
    keys: &CryptoKeys,
    aad: &[u8],
    nonce: &[u8; 12],
    buf: &mut [u8],
) -> anyhow::Result<[u8; 16]> {
    let tag = Aes256Gcm::new(keys.enc.as_ref().into())
        .encrypt_in_place_detached(Nonce::from_slice(nonce), aad, buf)
        .map_err(|_| anyhow::anyhow!("Verschlüsselung fehlgeschlagen"))?;
    let mut out = [0u8; 16];
    out.copy_from_slice(&tag);
    Ok(out)
}

/// Gegenstück zu [`encrypt_in_place_with_aad`]: `buf` ist danach Klartext.
pub fn decrypt_in_place_with_aad(
    keys: &CryptoKeys,
    aad: &[u8],
    nonce: &[u8],
    buf: &mut [u8],
    tag: &[u8],
) -> anyhow::Result<()> {
    if nonce.len() != 12 || tag.len() != 16 {
        anyhow::bail!("Entschlüsselung fehlgeschlagen");
    }
    Aes256Gcm::new(keys.enc.as_ref().into())
        .decrypt_in_place_detached(Nonce::from_slice(nonce), aad, buf, Tag::from_slice(tag))
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
    fn in_place_variant_matches_aead_output_and_binds_header() {
        let keys = Secret::generate().unwrap().derive_keys();
        let mut nonce = [0u8; 12];
        getrandom_fill(&mut nonce).unwrap();
        let expected = Aes256Gcm::new(keys.enc.as_ref().into())
            .encrypt(
                Nonce::from_slice(&nonce),
                Payload {
                    msg: b"sicherung",
                    aad: b"kopf",
                },
            )
            .unwrap();

        let mut buf = b"sicherung".to_vec();
        let tag = encrypt_in_place_with_aad(&keys, b"kopf", &nonce, &mut buf).unwrap();
        buf.extend_from_slice(&tag);
        // Byte-gleich zur bisherigen Ausgabe: alte Sicherungen bleiben lesbar.
        assert_eq!(buf, expected);

        let (body, tag) = buf.split_at_mut(9);
        let tag = tag.to_vec();
        decrypt_in_place_with_aad(&keys, b"kopf", &nonce, body, &tag).unwrap();
        assert_eq!(body, b"sicherung");

        // Verändertes Salt/Iterationen im Kopf → Entschlüsselung scheitert.
        let mut buf = expected[..9].to_vec();
        assert!(
            decrypt_in_place_with_aad(&keys, b"kopX", &nonce, &mut buf, &expected[9..]).is_err()
        );
    }
}
