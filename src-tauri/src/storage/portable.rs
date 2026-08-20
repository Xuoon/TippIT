//! Verschlüsselter Export und Import der Historie.
//!
//! Der Gerätesschlüssel (`key.bin`) ist plattformgebunden — unter Windows per
//! DPAPI an Benutzer und Maschine. Eine Kopie des Datenverzeichnisses nützt auf
//! einem anderen Rechner also nichts. Diese Datei ist der einzige Umzugsweg:
//! die Einträge werden entschlüsselt und mit einem aus dem Passwort abgeleiteten
//! Schlüssel neu verschlüsselt.
//!
//! Dateiaufbau: `magic(16) ‖ salt(16) ‖ iterationen(u32 BE) ‖ nonce(12) ‖ AES-256-GCM`.
//! Der Kopf geht als AAD mit ein, damit an Salt und Iterationszahl nicht
//! unbemerkt gedreht werden kann.

use std::path::Path;

use data_encoding::BASE64;
use hmac::Hmac;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::Zeroizing;

use super::crypto::{self, CryptoKeys};
use super::db::{self, EntryRow, KIND_IMAGE};
use crate::state::AppState;

const MAGIC: &[u8; 16] = b"TIPPIT-EXPORT-v1";
const HEADER_LEN: usize = 16 + 16 + 4 + 12;
/// Der Schlüssel schützt die komplette Historie und wird genau einmal pro
/// Export/Import abgeleitet — die Wartezeit von rund einer Sekunde ist hier gut
/// investiert.
const PBKDF2_ROUNDS: u32 = 600_000;

/// Eine Zeile im Export: Klartext, wie er ohne Verschlüsselung aussähe.
#[derive(Serialize, Deserialize)]
struct PortableEntry {
    uuid: String,
    kind: u8,
    /// Base64 der entschlüsselten Payload (Text-Bytes, Dateilisten-JSON oder PNG).
    data: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    html: Option<String>,
    created_at: i64,
    first_created_at: i64,
    copy_count: i64,
    pinned: bool,
    snippet: bool,
    /// 0 = aktiv. Gelöschtes bleibt auch nach dem Import gelöscht — der Export
    /// nimmt den Papierkorb mit, holt ihn aber nicht als Historie zurück.
    #[serde(default)]
    trashed_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source_app_name: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Payload {
    version: u32,
    entries: Vec<PortableEntry>,
}

#[derive(Serialize)]
pub struct ImportReport {
    /// Neu übernommene Einträge.
    pub imported: usize,
    /// Übersprungen, weil derselbe Inhalt schon vorhanden war.
    pub skipped: usize,
}

/// Kopf + Ciphertext einer Sicherung erzeugen. Getrennt von [`export`], damit
/// das Dateiformat ohne laufende App testbar bleibt.
fn seal(password: &str, plain: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 12];
    crypto::getrandom_fill(&mut salt)?;
    crypto::getrandom_fill(&mut nonce)?;

    let mut header = Vec::with_capacity(HEADER_LEN);
    header.extend_from_slice(MAGIC);
    header.extend_from_slice(&salt);
    header.extend_from_slice(&PBKDF2_ROUNDS.to_be_bytes());
    header.extend_from_slice(&nonce);

    let keys = derive(password, &salt, PBKDF2_ROUNDS)?;
    let mut out = header.clone();
    out.extend_from_slice(&crypto::encrypt_with_aad(&keys, &header, &nonce, plain)?);
    Ok(out)
}

/// Gegenstück zu [`seal`]: Kopf prüfen, Schlüssel ableiten, entschlüsseln.
fn unseal(password: &str, raw: &[u8]) -> anyhow::Result<Vec<u8>> {
    if raw.len() < HEADER_LEN + 16 || &raw[..16] != MAGIC {
        anyhow::bail!("Das ist keine TippIT-Sicherung");
    }
    let header = &raw[..HEADER_LEN];
    let salt = &raw[16..32];
    let rounds = u32::from_be_bytes([raw[32], raw[33], raw[34], raw[35]]);
    if rounds == 0 || rounds > 5_000_000 {
        anyhow::bail!("Die Datei nennt eine unplausible Iterationszahl");
    }
    let nonce = &raw[36..HEADER_LEN];
    let keys = derive(password, salt, rounds)?;
    crypto::decrypt_with_aad(&keys, header, nonce, &raw[HEADER_LEN..])
        .map_err(|_| anyhow::anyhow!("Falsches Passwort oder beschädigte Datei"))
}

fn derive(password: &str, salt: &[u8], rounds: u32) -> anyhow::Result<CryptoKeys> {
    if password.chars().count() < 8 {
        anyhow::bail!("Das Passwort muss mindestens 8 Zeichen haben");
    }
    let mut enc = Zeroizing::new([0u8; 32]);
    pbkdf2::pbkdf2::<Hmac<Sha256>>(password.as_bytes(), salt, rounds, enc.as_mut())
        .map_err(|_| anyhow::anyhow!("Schlüsselableitung fehlgeschlagen"))?;
    Ok(CryptoKeys { enc })
}

/// Alle Einträge (inklusive Papierkorb) in eine Datei schreiben. Gibt die Anzahl
/// exportierter Einträge zurück.
pub fn export(state: &AppState, path: &Path, password: &str) -> anyhow::Result<usize> {
    let rows = {
        let db = state.db.lock().unwrap();
        db::list_all(&db)?
    };
    let keys = state.keys.clone();

    let mut entries = Vec::with_capacity(rows.len());
    for row in &rows {
        let Some(cipher) = row.cipher.as_ref() else {
            continue;
        };
        let Ok(plain) = crypto::decrypt(&keys, &row.uuid, row.kind, cipher) else {
            tracing::warn!(
                "Eintrag {} nicht entschlüsselbar — nicht exportiert",
                row.uuid
            );
            continue;
        };
        let html = row
            .html
            .as_ref()
            .and_then(|blob| crypto::decrypt(&keys, &row.uuid, db::AAD_HTML, blob).ok())
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
        entries.push(PortableEntry {
            uuid: row.uuid.clone(),
            kind: row.kind,
            data: BASE64.encode(&plain),
            html,
            created_at: row.created_at,
            first_created_at: row.first_created_at,
            copy_count: row.copy_count,
            pinned: row.pinned,
            snippet: row.snippet,
            trashed_at: row.trashed_at,
            source_app_name: row.source_app_name.clone(),
        });
    }
    let count = entries.len();
    let json = serde_json::to_vec(&Payload {
        version: 1,
        entries,
    })?;

    let out = seal(password, &json)?;

    // Atomar schreiben: ein Absturz hinterlässt keine halbe Sicherung.
    let tmp = path.with_extension("tippit.tmp");
    std::fs::write(&tmp, &out)?;
    std::fs::rename(&tmp, path)?;
    Ok(count)
}

/// Export-Datei einlesen und in die bestehende Historie mischen. Vorhandene
/// Einträge bleiben unangetastet; gleiche Inhalte werden übersprungen.
pub fn import(state: &AppState, path: &Path, password: &str) -> anyhow::Result<ImportReport> {
    let raw = std::fs::read(path)?;
    let plain = unseal(password, &raw)?;
    let payload: Payload = serde_json::from_slice(&plain)?;
    if payload.version != 1 {
        anyhow::bail!("Die Sicherung stammt aus einer neueren TippIT-Version");
    }

    let keys = state.keys.clone();
    let mut db = state.db.lock().unwrap();
    let mut report = ImportReport {
        imported: 0,
        skipped: 0,
    };

    // Alles oder nichts: bricht eine Zeile ab, darf keine halb eingelesene
    // Sicherung zurückbleiben. Der Suchindex wird deshalb auch erst NACH dem
    // Commit nachgezogen — sonst zeigte er Einträge, die es nicht mehr gibt.
    let tx = db.transaction()?;
    let mut accepted: Vec<EntryRow> = Vec::new();

    for entry in payload.entries {
        let Ok(data) = BASE64.decode(entry.data.as_bytes()) else {
            report.skipped += 1;
            continue;
        };
        let hash = crypto::sha256(&data);
        if db::get(&tx, &entry.uuid)?.is_some() || db::find_by_hash(&tx, &hash)?.is_some() {
            report.skipped += 1;
            continue;
        }
        // Neu verschlüsseln: der Import-Schlüssel bleibt in der Datei, gespeichert
        // wird ausschließlich mit dem Gerätesschlüssel.
        let cipher = crypto::encrypt(&keys, &entry.uuid, entry.kind, &data)?;
        let thumb = (entry.kind == KIND_IMAGE)
            .then(|| crate::clipboard::read::thumbnail_png(&data))
            .flatten()
            .map(|png| crypto::encrypt(&keys, &entry.uuid, entry.kind, &png))
            .transpose()?;
        let html = entry
            .html
            .as_deref()
            .and_then(crate::clipboard::html::sanitize)
            .map(|clean| crypto::encrypt(&keys, &entry.uuid, db::AAD_HTML, clean.as_bytes()))
            .transpose()?;
        let row = EntryRow {
            uuid: entry.uuid,
            kind: entry.kind,
            cipher: Some(cipher),
            thumb,
            html,
            size_bytes: data.len() as i64,
            hash: hash.to_vec(),
            created_at: entry.created_at,
            pinned: entry.pinned,
            trashed_at: entry.trashed_at,
            snippet: entry.snippet,
            source_app_id: None,
            source_app_name: entry.source_app_name,
            first_created_at: entry.first_created_at,
            copy_count: entry.copy_count.max(1),
        };
        db::insert(&tx, &row)?;
        accepted.push(row);
        report.imported += 1;
    }
    tx.commit()?;
    drop(db);

    let mut index = state.index.write().unwrap();
    for row in &accepted {
        index.upsert(row, &keys);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bewusst wenige Runden: die Tests prüfen das Format, nicht die Härte der
    /// Ableitung — mit den echten 600 000 Runden liefe die Suite in Zeitlupe.
    fn seal_with(password: &str, rounds: u32, plain: &[u8]) -> Vec<u8> {
        let salt = [7u8; 16];
        let nonce = [9u8; 12];
        let mut header = Vec::with_capacity(HEADER_LEN);
        header.extend_from_slice(MAGIC);
        header.extend_from_slice(&salt);
        header.extend_from_slice(&rounds.to_be_bytes());
        header.extend_from_slice(&nonce);
        let keys = derive(password, &salt, rounds).unwrap();
        let mut out = header.clone();
        out.extend_from_slice(&crypto::encrypt_with_aad(&keys, &header, &nonce, plain).unwrap());
        out
    }

    #[test]
    fn roundtrip_and_wrong_password() {
        let file = seal_with("geheimes-passwort", 1_000, b"nutzlast");
        assert_eq!(unseal("geheimes-passwort", &file).unwrap(), b"nutzlast");
        assert!(unseal("falsches-passwort", &file).is_err());
    }

    #[test]
    fn header_is_authenticated() {
        let mut file = seal_with("geheimes-passwort", 1_000, b"nutzlast");
        // Salt drehen: die Ableitung liefert einen anderen Schlüssel UND die AAD
        // stimmt nicht mehr — beides muss auffallen.
        file[20] ^= 0xff;
        assert!(unseal("geheimes-passwort", &file).is_err());

        let mut file = seal_with("geheimes-passwort", 1_000, b"nutzlast");
        // Iterationszahl heruntersetzen darf die Datei nicht schwächen.
        file[32..36].copy_from_slice(&1u32.to_be_bytes());
        assert!(unseal("geheimes-passwort", &file).is_err());
    }

    #[test]
    fn foreign_files_are_rejected() {
        assert!(unseal("geheimes-passwort", b"kein tippit export").is_err());
        let mut file = seal_with("geheimes-passwort", 1_000, b"nutzlast");
        file[0] = b'X';
        assert!(unseal("geheimes-passwort", &file).is_err());
    }

    #[test]
    fn short_passwords_are_refused() {
        assert!(derive("kurz", &[0u8; 16], 1_000).is_err());
    }
}
