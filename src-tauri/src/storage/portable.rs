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

use std::collections::HashSet;
use std::ops::DerefMut;
use std::path::Path;

use data_encoding::BASE64;
use hmac::Hmac;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::Zeroizing;

use super::crypto::{self, CryptoKeys};
use super::db::{self, EntryRow, KIND_IMAGE};
use crate::state::AppState;

const MAGIC: &[u8; 16] = b"TIPPIT-EXPORT-v1";
const HEADER_LEN: usize = 16 + 16 + 4 + 12;
const TAG_LEN: usize = 16;
const PAYLOAD_VERSION: u32 = 1;
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

/// Klartext einer Sicherung: `{"version":1,"entries":[…]}`. Der Export schreibt
/// genau diese Form Zeile für Zeile selbst (siehe [`export_rows`]).
#[derive(Deserialize)]
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
    /// So viele aktive Einträge liegen nach dem Import über dem Eintragslimit.
    /// Der Import kürzt bewusst nicht selbst (übernommene Einträge behalten ihr
    /// altes Datum und wären die ersten Opfer); die nächste Kopie entfernt die
    /// ältesten ungepinnten davon endgültig.
    pub over_limit: usize,
}

/// Kopf mit frischem Salt und frischer Nonce. Der Klartext wird direkt dahinter
/// angehängt und von [`seal`] im selben Puffer verschlüsselt.
fn new_header(rounds: u32) -> anyhow::Result<Vec<u8>> {
    let mut salt = [0u8; 16];
    let mut nonce = [0u8; 12];
    crypto::getrandom_fill(&mut salt)?;
    crypto::getrandom_fill(&mut nonce)?;

    let mut header = Vec::with_capacity(HEADER_LEN);
    header.extend_from_slice(MAGIC);
    header.extend_from_slice(&salt);
    header.extend_from_slice(&rounds.to_be_bytes());
    header.extend_from_slice(&nonce);
    Ok(header)
}

/// Salt, Iterationszahl und Nonce aus einem Kopf von genau `HEADER_LEN` Bytes.
fn header_fields(header: &[u8]) -> ([u8; 16], u32, [u8; 12]) {
    let mut salt = [0u8; 16];
    salt.copy_from_slice(&header[16..32]);
    let rounds = u32::from_be_bytes([header[32], header[33], header[34], header[35]]);
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&header[36..HEADER_LEN]);
    (salt, rounds, nonce)
}

/// `buf` = Kopf aus [`new_header`] ‖ Klartext. Verschlüsselt den Klartext im
/// selben Puffer und hängt den Tag an; das Ergebnis ist die fertige Datei.
fn seal(password: &str, mut buf: Vec<u8>) -> anyhow::Result<Vec<u8>> {
    let (salt, rounds, nonce) = header_fields(&buf[..HEADER_LEN]);
    let keys = derive(password, &salt, rounds)?;
    let (header, body) = buf.split_at_mut(HEADER_LEN);
    let tag = crypto::encrypt_in_place_with_aad(&keys, header, &nonce, body)?;
    buf.extend_from_slice(&tag);
    Ok(buf)
}

/// Gegenstück zu [`seal`]: Kopf prüfen, Schlüssel ableiten, im Puffer
/// entschlüsseln. Gibt denselben Puffer als Klartext zurück.
fn unseal(password: &str, mut raw: Vec<u8>) -> anyhow::Result<Vec<u8>> {
    if raw.len() < HEADER_LEN + TAG_LEN || &raw[..16] != MAGIC {
        anyhow::bail!("Das ist keine TippIT-Sicherung");
    }
    let (salt, rounds, nonce) = header_fields(&raw[..HEADER_LEN]);
    if rounds == 0 || rounds > 5_000_000 {
        anyhow::bail!("Die Datei nennt eine unplausible Iterationszahl");
    }
    let keys = derive(password, &salt, rounds)?;
    let tag_at = raw.len() - TAG_LEN;
    let (header, rest) = raw.split_at_mut(HEADER_LEN);
    let (body, tag) = rest.split_at_mut(tag_at - HEADER_LEN);
    crypto::decrypt_in_place_with_aad(&keys, header, &nonce, body, tag)
        .map_err(|_| anyhow::anyhow!("Falsches Passwort oder beschädigte Datei"))?;
    raw.truncate(tag_at);
    raw.drain(..HEADER_LEN);
    Ok(raw)
}

fn check_password(password: &str) -> anyhow::Result<()> {
    if password.chars().count() < 8 {
        anyhow::bail!("Das Passwort muss mindestens 8 Zeichen haben");
    }
    Ok(())
}

fn derive(password: &str, salt: &[u8], rounds: u32) -> anyhow::Result<CryptoKeys> {
    check_password(password)?;
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
    let (out, count) = export_rows(rows, &state.keys, password, PBKDF2_ROUNDS)?;

    // Atomar schreiben: ein Absturz hinterlässt keine halbe Sicherung.
    let tmp = path.with_extension("tippit.tmp");
    std::fs::write(&tmp, &out)?;
    std::fs::rename(&tmp, path)?;
    Ok(count)
}

/// Kern von [`export`]: Zeilen entschlüsseln, als JSON direkt hinter den Kopf
/// schreiben und im selben Puffer verschlüsseln. Jede Zeile samt Bild-Blob wird
/// gleich nach dem Schreiben freigegeben, damit die Historie nicht mehrfach
/// gleichzeitig im Speicher liegt.
fn export_rows(
    rows: Vec<EntryRow>,
    keys: &CryptoKeys,
    password: &str,
    rounds: u32,
) -> anyhow::Result<(Vec<u8>, usize)> {
    check_password(password)?;
    let mut buf = new_header(rounds)?;
    buf.extend_from_slice(format!("{{\"version\":{PAYLOAD_VERSION},\"entries\":[").as_bytes());
    let mut count = 0;
    for row in rows {
        let Some(entry) = portable_entry(row, keys) else {
            continue;
        };
        if count > 0 {
            buf.push(b',');
        }
        serde_json::to_writer(&mut buf, &entry)?;
        count += 1;
    }
    buf.extend_from_slice(b"]}");
    Ok((seal(password, buf)?, count))
}

fn portable_entry(row: EntryRow, keys: &CryptoKeys) -> Option<PortableEntry> {
    let cipher = row.cipher.as_ref()?;
    let Ok(plain) = crypto::decrypt(keys, &row.uuid, row.kind, cipher) else {
        tracing::warn!(
            "Eintrag {} nicht entschlüsselbar — nicht exportiert",
            row.uuid
        );
        return None;
    };
    let html = row
        .html
        .as_ref()
        .and_then(|blob| crypto::decrypt(keys, &row.uuid, db::AAD_HTML, blob).ok())
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    Some(PortableEntry {
        data: BASE64.encode(&plain),
        html,
        uuid: row.uuid,
        kind: row.kind,
        created_at: row.created_at,
        first_created_at: row.first_created_at,
        copy_count: row.copy_count,
        pinned: row.pinned,
        snippet: row.snippet,
        trashed_at: row.trashed_at,
        source_app_name: row.source_app_name,
    })
}

/// Export-Datei einlesen und in die bestehende Historie mischen. Vorhandene
/// Einträge bleiben unangetastet; gleiche Inhalte werden übersprungen.
pub fn import(state: &AppState, path: &Path, password: &str) -> anyhow::Result<ImportReport> {
    let raw = std::fs::read(path)?;
    let (accepted, imported, skipped) =
        import_with(|| state.db.lock().unwrap(), &state.keys, password, raw)?;

    // Der Suchindex wird erst nach dem Commit nachgezogen, sonst zeigte er
    // Einträge, die ein Abbruch gar nicht übernommen hätte.
    {
        let mut index = state.index.write().unwrap();
        for row in &accepted {
            index.upsert(row, &state.keys);
        }
    }
    let active = {
        let db = state.db.lock().unwrap();
        db::count_active(&db)?
    };
    let limit = i64::from(state.settings.read().unwrap().history.max_entries);
    Ok(ImportReport {
        imported,
        skipped,
        over_limit: usize::try_from(active - limit).unwrap_or(0),
    })
}

/// Kern von [`import`]. `lock` liefert die Datenbank und wird nur zweimal kurz
/// gehalten: für den Abgleich mit dem Bestand und fürs Einfügen. Dekodieren,
/// Verschlüsseln und Thumbnails laufen dazwischen ohne Lock, sonst warteten
/// Capture-Worker und Commands den ganzen Import über auf die Datenbank.
/// Liefert die übernommenen Zeilen (Bilder ohne Inhalt, der Index braucht ihn
/// nicht), die Zahl der übernommenen und die der übersprungenen Einträge.
fn import_with<G: DerefMut<Target = Connection>>(
    lock: impl Fn() -> G,
    keys: &CryptoKeys,
    password: &str,
    raw: Vec<u8>,
) -> anyhow::Result<(Vec<EntryRow>, usize, usize)> {
    let payload = open_payload(password, raw)?;
    let known = db::known_keys(&lock())?;
    let (rows, mut skipped) = prepare_rows(payload, keys, known)?;

    let mut conn = lock();
    // Alles oder nichts: bricht eine Zeile ab, darf keine halb eingelesene
    // Sicherung zurückbleiben.
    let tx = conn.transaction()?;
    // Erneut prüfen: während Phase 1 kann der Monitor dieselbe Kopie erfasst
    // haben. Untereinander sind die Zeilen schon dedupliziert.
    let (uuids, hashes) = db::known_keys(&tx)?;
    let mut accepted = Vec::with_capacity(rows.len());
    for mut row in rows {
        if uuids.contains(&row.uuid) || hashes.contains(&row.hash) {
            skipped += 1;
            continue;
        }
        db::insert(&tx, &row)?;
        if row.kind == KIND_IMAGE {
            row.cipher = None;
        }
        accepted.push(row);
    }
    tx.commit()?;
    let imported = accepted.len();
    Ok((accepted, imported, skipped))
}

/// Datei entschlüsseln und parsen. Der Klartext-Puffer lebt nur bis hier.
fn open_payload(password: &str, raw: Vec<u8>) -> anyhow::Result<Payload> {
    let plain = unseal(password, raw)?;
    let payload: Payload = serde_json::from_slice(&plain)?;
    if payload.version != PAYLOAD_VERSION {
        anyhow::bail!("Die Sicherung stammt aus einer neueren TippIT-Version");
    }
    Ok(payload)
}

/// Einträge der Sicherung in Zeilen für diese Datenbank verwandeln: vorhandene
/// uuids und aktive Inhalte überspringen, dann mit dem Geräteschlüssel neu
/// verschlüsseln. Gibt die Zeilen und die Zahl der übersprungenen zurück.
fn prepare_rows(
    payload: Payload,
    keys: &CryptoKeys,
    (mut uuids, mut hashes): (HashSet<String>, HashSet<Vec<u8>>),
) -> anyhow::Result<(Vec<EntryRow>, usize)> {
    let mut rows = Vec::with_capacity(payload.entries.len());
    let mut skipped = 0;
    for entry in payload.entries {
        // Unbekannte Typen würden eingefügt, aber nie angezeigt, und zählten
        // trotzdem gegen das Eintragslimit.
        if entry.kind > db::KIND_FILES {
            skipped += 1;
            continue;
        }
        let Ok(data) = BASE64.decode(entry.data.as_bytes()) else {
            skipped += 1;
            continue;
        };
        let hash = crypto::sha256(&data).to_vec();
        if uuids.contains(&entry.uuid) || hashes.contains(&hash) {
            skipped += 1;
            continue;
        }
        // Neu verschlüsseln: der Import-Schlüssel bleibt in der Datei, gespeichert
        // wird ausschließlich mit dem Gerätesschlüssel.
        let cipher = crypto::encrypt(keys, &entry.uuid, entry.kind, &data)?;
        let thumb = (entry.kind == KIND_IMAGE)
            .then(|| crate::clipboard::read::thumbnail_png(&data))
            .flatten()
            .map(|png| crypto::encrypt(keys, &entry.uuid, entry.kind, &png))
            .transpose()?;
        let html = entry
            .html
            .as_deref()
            .and_then(crate::clipboard::html::sanitize)
            .map(|clean| crypto::encrypt(keys, &entry.uuid, db::AAD_HTML, clean.as_bytes()))
            .transpose()?;
        // Dieselben Regeln wie `db::find_by_hash`: nur aktive Nicht-Bausteine
        // sperren einen gleichen Inhalt.
        uuids.insert(entry.uuid.clone());
        if entry.trashed_at == 0 && !entry.snippet {
            hashes.insert(hash.clone());
        }
        rows.push(EntryRow {
            uuid: entry.uuid,
            kind: entry.kind,
            cipher: Some(cipher),
            thumb,
            html,
            size_bytes: data.len() as i64,
            hash,
            created_at: entry.created_at,
            pinned: entry.pinned,
            trashed_at: entry.trashed_at,
            snippet: entry.snippet,
            source_app_id: None,
            source_app_name: entry.source_app_name,
            first_created_at: entry.first_created_at,
            copy_count: entry.copy_count.max(1),
        });
    }
    Ok((rows, skipped))
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::sync::Mutex;

    use image::codecs::png::PngEncoder;
    use image::ImageEncoder;

    use super::*;
    use crate::storage::crypto::Secret;
    use crate::storage::db::{KIND_FILES, KIND_TEXT};

    /// Bewusst wenige Runden: die Tests prüfen das Format, nicht die Härte der
    /// Ableitung — mit den echten 600 000 Runden liefe die Suite in Zeitlupe.
    const ROUNDS: u32 = 1_000;
    const PASSWORD: &str = "geheimes-passwort";

    fn seal_with(password: &str, rounds: u32, plain: &[u8]) -> Vec<u8> {
        let mut buf = new_header(rounds).unwrap();
        buf.extend_from_slice(plain);
        seal(password, buf).unwrap()
    }

    fn mem_db() -> Mutex<Connection> {
        let conn = Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        Mutex::new(conn)
    }

    fn import_into(
        conn: &Mutex<Connection>,
        keys: &CryptoKeys,
        file: &[u8],
    ) -> anyhow::Result<(Vec<EntryRow>, usize, usize)> {
        import_with(|| conn.lock().unwrap(), keys, PASSWORD, file.to_vec())
    }

    fn tiny_png() -> Vec<u8> {
        let mut png = Vec::new();
        PngEncoder::new(Cursor::new(&mut png))
            .write_image(&[255u8; 2 * 2 * 4], 2, 2, image::ExtendedColorType::Rgba8)
            .unwrap();
        png
    }

    fn row(keys: &CryptoKeys, uuid: &str, kind: u8, plain: &[u8]) -> EntryRow {
        EntryRow {
            uuid: uuid.into(),
            kind,
            cipher: Some(crypto::encrypt(keys, uuid, kind, plain).unwrap()),
            thumb: None,
            html: None,
            size_bytes: plain.len() as i64,
            hash: crypto::sha256(plain).to_vec(),
            created_at: 100,
            pinned: false,
            trashed_at: 0,
            snippet: false,
            source_app_id: Some("com.chrome".into()),
            source_app_name: Some("Chrome".into()),
            first_created_at: 50,
            copy_count: 2,
        }
    }

    /// Quelle mit je einem Eintrag jeder Art: Text mit Formatierung, Dateiliste,
    /// Bild, Papierkorb und Baustein.
    fn source_db(keys: &CryptoKeys) -> Mutex<Connection> {
        let conn = mem_db();
        {
            let db = conn.lock().unwrap();
            let mut rich = row(keys, "text", KIND_TEXT, b"hallo welt");
            rich.pinned = true;
            rich.html =
                Some(crypto::encrypt(keys, "text", db::AAD_HTML, b"<b>hallo</b> welt").unwrap());
            db::insert(&db, &rich).unwrap();
            db::insert(&db, &row(keys, "files", KIND_FILES, br#"["/tmp/a"]"#)).unwrap();
            db::insert(&db, &row(keys, "bild", KIND_IMAGE, &tiny_png())).unwrap();
            let mut trashed = row(keys, "weg", KIND_TEXT, b"geloescht");
            trashed.trashed_at = 999;
            db::insert(&db, &trashed).unwrap();
            let mut snippet = row(keys, "baustein", KIND_TEXT, b"Gruss {datum}");
            snippet.snippet = true;
            db::insert(&db, &snippet).unwrap();
        }
        conn
    }

    fn export_file(conn: &Mutex<Connection>, keys: &CryptoKeys) -> (Vec<u8>, usize) {
        let rows = db::list_all(&conn.lock().unwrap()).unwrap();
        export_rows(rows, keys, PASSWORD, ROUNDS).unwrap()
    }

    #[test]
    fn roundtrip_and_wrong_password() {
        let file = seal_with(PASSWORD, ROUNDS, b"nutzlast");
        assert_eq!(unseal(PASSWORD, file.clone()).unwrap(), b"nutzlast");
        assert!(unseal("falsches-passwort", file).is_err());
    }

    #[test]
    fn header_is_authenticated() {
        let mut file = seal_with(PASSWORD, ROUNDS, b"nutzlast");
        // Salt drehen: die Ableitung liefert einen anderen Schlüssel UND die AAD
        // stimmt nicht mehr — beides muss auffallen.
        file[20] ^= 0xff;
        assert!(unseal(PASSWORD, file).is_err());

        let mut file = seal_with(PASSWORD, ROUNDS, b"nutzlast");
        // Iterationszahl heruntersetzen darf die Datei nicht schwächen.
        file[32..36].copy_from_slice(&1u32.to_be_bytes());
        assert!(unseal(PASSWORD, file).is_err());
    }

    #[test]
    fn foreign_files_are_rejected() {
        assert!(unseal(PASSWORD, b"kein tippit export".to_vec()).is_err());
        let mut file = seal_with(PASSWORD, ROUNDS, b"nutzlast");
        file[0] = b'X';
        assert!(unseal(PASSWORD, file).is_err());
    }

    #[test]
    fn short_passwords_are_refused() {
        assert!(derive("kurz", &[0u8; 16], ROUNDS).is_err());
        assert!(export_rows(
            Vec::new(),
            &Secret::generate().unwrap().derive_keys(),
            "kurz",
            ROUNDS
        )
        .is_err());
    }

    #[test]
    fn export_writes_the_payload_json() {
        let keys = Secret::generate().unwrap().derive_keys();
        let (file, count) = export_file(&source_db(&keys), &keys);
        assert_eq!(count, 5);
        let plain = unseal(PASSWORD, file).unwrap();
        let payload: Payload = serde_json::from_slice(&plain).unwrap();
        assert_eq!(payload.version, 1);
        assert_eq!(payload.entries.len(), 5);
        assert!(plain.starts_with(br#"{"version":1,"entries":[{"uuid":"#));
    }

    #[test]
    fn import_roundtrip_keeps_state_and_rekeys() {
        let source_keys = Secret::generate().unwrap().derive_keys();
        let (file, _) = export_file(&source_db(&source_keys), &source_keys);

        let keys = Secret::generate().unwrap().derive_keys();
        let target = mem_db();
        let (accepted, imported, skipped) = import_into(&target, &keys, &file).unwrap();
        assert_eq!((imported, skipped), (5, 0));
        // Für den Index reicht bei Bildern die Zeile ohne Inhalt.
        let image = accepted.iter().find(|r| r.uuid == "bild").unwrap();
        assert!(image.cipher.is_none());
        assert!(image.thumb.is_some());

        let db = target.lock().unwrap();
        let text = db::get(&db, "text").unwrap().unwrap();
        assert!(text.pinned);
        assert_eq!(text.created_at, 100);
        assert_eq!(text.first_created_at, 50);
        assert_eq!(text.copy_count, 2);
        assert_eq!(text.source_app_id, None);
        assert_eq!(text.source_app_name.as_deref(), Some("Chrome"));
        assert_eq!(
            crypto::decrypt(&keys, "text", KIND_TEXT, text.cipher.as_ref().unwrap()).unwrap(),
            b"hallo welt"
        );
        // Der Rich-Text-Blob ist nur mit AAD_HTML lesbar, nie als Klartext-Blob.
        let html = text.html.as_ref().unwrap();
        assert_eq!(
            crypto::decrypt(&keys, "text", db::AAD_HTML, html).unwrap(),
            b"<b>hallo</b> welt"
        );
        assert!(crypto::decrypt(&keys, "text", KIND_TEXT, html).is_err());
        assert!(crypto::decrypt(&source_keys, "text", db::AAD_HTML, html).is_err());

        let image = db::get(&db, "bild").unwrap().unwrap();
        assert_eq!(
            crypto::decrypt(&keys, "bild", KIND_IMAGE, image.cipher.as_ref().unwrap()).unwrap(),
            tiny_png()
        );
        // Papierkorb bleibt Papierkorb, Baustein bleibt Baustein.
        assert_eq!(db::get(&db, "weg").unwrap().unwrap().trashed_at, 999);
        assert!(db::get(&db, "baustein").unwrap().unwrap().snippet);
        assert_eq!(db::count_active(&db).unwrap(), 4);
    }

    #[test]
    fn importing_twice_adds_nothing() {
        let keys = Secret::generate().unwrap().derive_keys();
        let (file, _) = export_file(&source_db(&keys), &keys);
        let target = mem_db();
        assert_eq!(import_into(&target, &keys, &file).unwrap().1, 5);
        let (accepted, imported, skipped) = import_into(&target, &keys, &file).unwrap();
        assert!(accepted.is_empty());
        assert_eq!((imported, skipped), (0, 5));
    }

    #[test]
    fn import_skips_unknown_kinds_and_duplicates_within_the_file() {
        let entry = |uuid: &str, kind: u8, data: &[u8]| {
            serde_json::json!({
                "uuid": uuid, "kind": kind, "data": BASE64.encode(data),
                "created_at": 1, "first_created_at": 1, "copy_count": 1,
                "pinned": false, "snippet": false,
            })
        };
        let payload = serde_json::json!({
            "version": 1,
            "entries": [
                entry("a", KIND_TEXT, b"eins"),
                entry("b", KIND_TEXT, b"eins"),
                entry("c", 7, b"zukunft"),
                entry("a", KIND_TEXT, b"zwei"),
            ],
        });
        let file = seal_with(PASSWORD, ROUNDS, &serde_json::to_vec(&payload).unwrap());
        let keys = Secret::generate().unwrap().derive_keys();
        let target = mem_db();
        let (_, imported, skipped) = import_into(&target, &keys, &file).unwrap();
        assert_eq!((imported, skipped), (1, 3));
    }

    #[test]
    fn failed_import_leaves_database_untouched() {
        let keys = Secret::generate().unwrap().derive_keys();
        let (file, _) = export_file(&source_db(&keys), &keys);
        let target = mem_db();
        assert!(import_with(|| target.lock().unwrap(), &keys, "falsches-passwort", file).is_err());
        assert!(db::list_all(&target.lock().unwrap()).unwrap().is_empty());
    }
}
