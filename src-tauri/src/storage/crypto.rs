use aes_gcm::aead::{Aead, Payload};
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
// aes-gcm bewusst auf der 0.10-Serie: klassische GenericArray-API.
use data_encoding::{Encoding, Specification};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use std::sync::LazyLock;
use zeroize::Zeroizing;

use super::paths::AppPaths;

/// Crockford-Base32: keine verwechselbaren Zeichen (I/L/O/U fehlen).
static CROCKFORD: LazyLock<Encoding> = LazyLock::new(|| {
    let mut spec = Specification::new();
    spec.symbols.push_str("0123456789ABCDEFGHJKMNPQRSTVWXYZ");
    spec.encoding().expect("gültige Base32-Spezifikation")
});

const PAIRING_VERSION: u8 = 0x01;
const AAD_VERSION: u8 = 0x01;

/// Das 32-Byte-Master-Secret. Beim Erstellen einer Sync-Gruppe wird es zum Gruppen-Secret;
/// beim Koppeln wird es durch das der Gruppe ersetzt.
pub struct Secret(Zeroizing<[u8; 32]>);

/// Aus dem Secret abgeleitete Schlüssel. `enc` verlässt nie das Gerät;
/// `auth` autorisiert gegenüber Convex; `group_id` identifiziert die Gruppe.
#[derive(Clone)]
pub struct CryptoKeys {
    pub enc: Zeroizing<[u8; 32]>,
    pub auth: [u8; 32],
    pub group_id: String,
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
        let mut auth = [0u8; 32];
        let mut gid = [0u8; 16];
        hk.expand(b"tippit/v1/enc", enc.as_mut())
            .expect("HKDF expand");
        hk.expand(b"tippit/v1/auth", &mut auth)
            .expect("HKDF expand");
        hk.expand(b"tippit/v1/gid", &mut gid).expect("HKDF expand");
        CryptoKeys {
            enc,
            auth,
            group_id: CROCKFORD.encode(&gid),
        }
    }

    /// Kopplungscode: TIPPIT-XXXXX-XXXXX-… (Version ‖ Secret ‖ 4-Byte-Checksumme, Base32).
    pub fn to_pairing_code(&self) -> String {
        let mut payload = Vec::with_capacity(37);
        payload.push(PAIRING_VERSION);
        payload.extend_from_slice(self.0.as_ref());
        payload.extend_from_slice(&checksum(&payload));
        let encoded = CROCKFORD.encode(&payload);
        let groups: Vec<&str> = encoded
            .as_bytes()
            .chunks(5)
            .map(|c| std::str::from_utf8(c).unwrap())
            .collect();
        format!("TIPPIT-{}", groups.join("-"))
    }

    pub fn from_pairing_code(code: &str) -> anyhow::Result<Self> {
        let upper = code.trim().to_uppercase();
        let cleaned: String = upper
            .strip_prefix("TIPPIT")
            .unwrap_or(&upper)
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        let bytes = CROCKFORD
            .decode(cleaned.as_bytes())
            .map_err(|_| anyhow::anyhow!("Kopplungscode enthält ungültige Zeichen"))?;
        if bytes.len() != 37 || bytes[0] != PAIRING_VERSION {
            anyhow::bail!("Kopplungscode hat das falsche Format");
        }
        if checksum(&bytes[..33]) != bytes[33..37] {
            anyhow::bail!("Kopplungscode-Prüfsumme stimmt nicht — Tippfehler?");
        }
        let mut secret = Zeroizing::new([0u8; 32]);
        secret.copy_from_slice(&bytes[1..33]);
        Ok(Self(secret))
    }

    /// Secret plattformgeschützt in key.bin ablegen (atomar: tmp + rename).
    /// Windows: DPAPI (User-Scope); macOS: 0600 + FileVault (s. platform::protect).
    pub fn store(&self, paths: &AppPaths) -> anyhow::Result<()> {
        self.write_wrapped(&paths.key_file())
    }

    /// Phase 1 der Schlüsselrotation: neues Secret nach key.bin.new schreiben,
    /// BEVOR die DB umgeschlüsselt wird. key.bin bleibt bis `promote_pending` unberührt —
    /// so ist jeder Abbruch-/Crash-Zeitpunkt per Probe-Decrypt recoverbar.
    pub fn store_pending(&self, paths: &AppPaths) -> anyhow::Result<()> {
        self.write_wrapped(&paths.key_file_pending())
    }

    /// Phase 2: nach erfolgreicher Umschlüsselung key.bin.new → key.bin (atomar).
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

fn checksum(data: &[u8]) -> [u8; 4] {
    let digest = Sha256::digest(data);
    [digest[0], digest[1], digest[2], digest[3]]
}

fn getrandom_fill(buf: &mut [u8]) -> anyhow::Result<()> {
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

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_code_roundtrip() {
        let secret = Secret::generate().unwrap();
        let code = secret.to_pairing_code();
        assert!(code.starts_with("TIPPIT-"));
        let restored = Secret::from_pairing_code(&code).unwrap();
        assert_eq!(secret.0.as_ref(), restored.0.as_ref());
    }

    #[test]
    fn pairing_code_checksum_catches_typo() {
        let secret = Secret::generate().unwrap();
        let mut code = secret.to_pairing_code();
        // ein Zeichen verfälschen (letztes Zeichen rotieren)
        let last = code.pop().unwrap();
        code.push(if last == 'A' { 'B' } else { 'A' });
        assert!(Secret::from_pairing_code(&code).is_err());
    }

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
}
