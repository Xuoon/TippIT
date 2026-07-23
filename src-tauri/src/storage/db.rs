use rusqlite::{params, Connection, OptionalExtension};

use super::paths::AppPaths;

pub const KIND_TEXT: u8 = 0;
pub const KIND_IMAGE: u8 = 1;
pub const KIND_FILES: u8 = 2;

pub const SYNC_DIRTY: u8 = 0;
pub const SYNC_SYNCED: u8 = 1;

#[derive(Clone, Debug)]
pub struct EntryRow {
    pub uuid: String,
    pub kind: u8,
    pub cipher: Option<Vec<u8>>,
    pub thumb: Option<Vec<u8>>,
    pub size_bytes: i64,
    pub hash: Vec<u8>,
    pub created_at: i64,
    pub pinned: bool,
    pub deleted: bool,
    pub device_id: String,
    pub lamport: i64,
    /// Geräte-lokal: Bundle-ID / exe path — nicht gesynct.
    pub source_app_id: Option<String>,
    pub source_app_name: Option<String>,
    /// Erste Erfassung (ändert sich bei touch/re-copy nicht).
    pub first_created_at: i64,
    /// Wie oft der Hash erneut kopiert / angetippt wurde.
    pub copy_count: i64,
}

/// Steuert, ob `touch` Source-Metadaten anfasst.
pub enum TouchSource<'a> {
    /// Timestamps only — Source unverändert (`copy_entry`, Lookup-None).
    Keep,
    /// Bekannte Vordergrund-App.
    Set { id: &'a str, name: &'a str },
    /// Bewusst leeren (TippIT self).
    Clear,
}

pub fn open(paths: &AppPaths) -> anyhow::Result<Connection> {
    let conn = Connection::open(paths.db_file())?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> anyhow::Result<()> {
    // CREATE bleibt v1-Shape; Upgrade per ALTER → schema_version 2.
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS entries (
            uuid        TEXT PRIMARY KEY,
            kind        INTEGER NOT NULL,
            cipher      BLOB,
            thumb       BLOB,
            size_bytes  INTEGER NOT NULL,
            hash        BLOB NOT NULL,
            created_at  INTEGER NOT NULL,
            pinned      INTEGER NOT NULL DEFAULT 0,
            deleted     INTEGER NOT NULL DEFAULT 0,
            device_id   TEXT NOT NULL,
            lamport     INTEGER NOT NULL,
            sync_state  INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_entries_created ON entries(deleted, created_at DESC);
        CREATE INDEX IF NOT EXISTS idx_entries_hash ON entries(hash) WHERE deleted = 0;
        CREATE INDEX IF NOT EXISTS idx_entries_dirty ON entries(sync_state) WHERE sync_state = 0;
        CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT);
        "#,
    )?;
    if meta_get(conn, "schema_version")?.is_none() {
        meta_set(conn, "schema_version", "1")?;
    }
    let v = meta_get(conn, "schema_version")?.unwrap_or_else(|| "1".into());
    match v.as_str() {
        "1" => {
            // Idempotent: Spalten nur adden wenn noch fehlend.
            if !column_exists(conn, "entries", "source_app_id")? {
                conn.execute("ALTER TABLE entries ADD COLUMN source_app_id TEXT", [])?;
            }
            if !column_exists(conn, "entries", "source_app_name")? {
                conn.execute("ALTER TABLE entries ADD COLUMN source_app_name TEXT", [])?;
            }
            meta_set(conn, "schema_version", "2")?;
            // fallthrough to v3
            migrate_v3(conn)?;
        }
        "2" => migrate_v3(conn)?,
        "3" => {}
        other => {
            tracing::warn!("unbekannte schema_version={other}, skip migrate");
        }
    }
    if meta_get(conn, "device_id")?.is_none() {
        meta_set(conn, "device_id", &uuid::Uuid::new_v4().to_string())?;
    }
    Ok(())
}

/// schema 2 → 3: first_created_at + copy_count für Sortierung.
fn migrate_v3(conn: &Connection) -> anyhow::Result<()> {
    if !column_exists(conn, "entries", "first_created_at")? {
        conn.execute(
            "ALTER TABLE entries ADD COLUMN first_created_at INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
        // Bestehende Zeilen: first = created_at
        conn.execute(
            "UPDATE entries SET first_created_at = created_at WHERE first_created_at = 0",
            [],
        )?;
    }
    if !column_exists(conn, "entries", "copy_count")? {
        conn.execute(
            "ALTER TABLE entries ADD COLUMN copy_count INTEGER NOT NULL DEFAULT 1",
            [],
        )?;
    }
    meta_set(conn, "schema_version", "3")?;
    Ok(())
}

fn column_exists(conn: &Connection, table: &str, column: &str) -> anyhow::Result<bool> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let cols = stmt.query_map([], |r| r.get::<_, String>(1))?;
    for c in cols {
        if c? == column {
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn meta_get(conn: &Connection, key: &str) -> anyhow::Result<Option<String>> {
    Ok(conn
        .query_row("SELECT v FROM meta WHERE k = ?1", params![key], |r| {
            r.get(0)
        })
        .optional()?)
}

pub fn meta_set(conn: &Connection, key: &str, value: &str) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO meta(k, v) VALUES(?1, ?2) ON CONFLICT(k) DO UPDATE SET v = excluded.v",
        params![key, value],
    )?;
    Ok(())
}

pub fn device_id(conn: &Connection) -> anyhow::Result<String> {
    Ok(meta_get(conn, "device_id")?.expect("device_id wird in migrate() angelegt"))
}

/// Nächster Lamport-Wert (monoton, über meta persistiert).
pub fn next_lamport(conn: &Connection) -> anyhow::Result<i64> {
    next_lamport_batch(conn, 1)
}

/// Reserviert `count` aufeinanderfolgende Lamport-Werte und gibt den ersten zurück.
/// Wichtig für Massen-Operationen (prune/clear): jede vergebene Nummer muss den
/// Zähler wirklich verbrauchen, sonst entstehen doppelte (lamport, device)-Paare.
pub fn next_lamport_batch(conn: &Connection, count: i64) -> anyhow::Result<i64> {
    let current: i64 = meta_get(conn, "lamport")?
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    meta_set(conn, "lamport", &(current + count).to_string())?;
    Ok(current + 1)
}

pub fn bump_lamport_to(conn: &Connection, at_least: i64) -> anyhow::Result<()> {
    let current: i64 = meta_get(conn, "lamport")?
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    if at_least > current {
        meta_set(conn, "lamport", &at_least.to_string())?;
    }
    Ok(())
}

/// Existiert ein nicht gelöschter Eintrag mit diesem Inhalt bereits? → uuid
pub fn find_by_hash(conn: &Connection, hash: &[u8]) -> anyhow::Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT uuid FROM entries WHERE hash = ?1 AND deleted = 0 LIMIT 1",
            params![hash],
            |r| r.get(0),
        )
        .optional()?)
}

pub fn insert(conn: &Connection, row: &EntryRow) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO entries(uuid, kind, cipher, thumb, size_bytes, hash, created_at, pinned, deleted, device_id, lamport, sync_state, source_app_id, source_app_name, first_created_at, copy_count)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
        params![
            row.uuid,
            row.kind,
            row.cipher,
            row.thumb,
            row.size_bytes,
            row.hash,
            row.created_at,
            row.pinned,
            row.deleted,
            row.device_id,
            row.lamport,
            SYNC_DIRTY,
            row.source_app_id,
            row.source_app_name,
            row.first_created_at,
            row.copy_count,
        ],
    )?;
    Ok(())
}

/// Duplikat „nach oben schieben": Zeitstempel + Lamport; Source per [`TouchSource`].
pub fn touch(
    conn: &Connection,
    uuid: &str,
    now_ms: i64,
    lamport: i64,
    source: TouchSource<'_>,
) -> anyhow::Result<()> {
    match source {
        TouchSource::Keep => {
            conn.execute(
                "UPDATE entries SET created_at = ?2, lamport = ?3, sync_state = 0,
                 copy_count = copy_count + 1 WHERE uuid = ?1",
                params![uuid, now_ms, lamport],
            )?;
        }
        TouchSource::Set { id, name } => {
            conn.execute(
                "UPDATE entries SET created_at = ?2, lamport = ?3, sync_state = 0,
                 source_app_id = ?4, source_app_name = ?5, copy_count = copy_count + 1 WHERE uuid = ?1",
                params![uuid, now_ms, lamport, id, name],
            )?;
        }
        TouchSource::Clear => {
            conn.execute(
                "UPDATE entries SET created_at = ?2, lamport = ?3, sync_state = 0,
                 source_app_id = NULL, source_app_name = NULL, copy_count = copy_count + 1 WHERE uuid = ?1",
                params![uuid, now_ms, lamport],
            )?;
        }
    }
    Ok(())
}

pub fn set_pinned(conn: &Connection, uuid: &str, pinned: bool, lamport: i64) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE entries SET pinned = ?2, lamport = ?3, sync_state = 0 WHERE uuid = ?1",
        params![uuid, pinned, lamport],
    )?;
    Ok(())
}

/// Tombstone: Inhalt + Source entfernen, Zeile für den Sync behalten.
pub fn mark_deleted(conn: &Connection, uuid: &str, lamport: i64) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE entries SET deleted = 1, cipher = NULL, thumb = NULL,
         source_app_id = NULL, source_app_name = NULL, lamport = ?2, sync_state = 0 WHERE uuid = ?1",
        params![uuid, lamport],
    )?;
    Ok(())
}

const SELECT_COLS: &str = "uuid, kind, cipher, thumb, size_bytes, hash, created_at, pinned, deleted, device_id, lamport, source_app_id, source_app_name, first_created_at, copy_count";

pub fn get(conn: &Connection, uuid: &str) -> anyhow::Result<Option<EntryRow>> {
    Ok(conn
        .query_row(
            &format!("SELECT {SELECT_COLS} FROM entries WHERE uuid = ?1"),
            params![uuid],
            row_from,
        )
        .optional()?)
}

pub fn list_active(conn: &Connection) -> anyhow::Result<Vec<EntryRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLS} FROM entries WHERE deleted = 0 ORDER BY created_at DESC"
    ))?;
    let rows = stmt
        .query_map([], row_from)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn row_from(r: &rusqlite::Row<'_>) -> rusqlite::Result<EntryRow> {
    Ok(EntryRow {
        uuid: r.get(0)?,
        kind: r.get(1)?,
        cipher: r.get(2)?,
        thumb: r.get(3)?,
        size_bytes: r.get(4)?,
        hash: r.get(5)?,
        created_at: r.get(6)?,
        pinned: r.get(7)?,
        deleted: r.get(8)?,
        device_id: r.get(9)?,
        lamport: r.get(10)?,
        source_app_id: r.get(11)?,
        source_app_name: r.get(12)?,
        first_created_at: r.get(13)?,
        copy_count: r.get(14)?,
    })
}

/// Dirty-Zeilen für den Push (sync_state = 0).
pub fn list_dirty(conn: &Connection, limit: u32) -> anyhow::Result<Vec<EntryRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLS} FROM entries WHERE sync_state = 0 ORDER BY lamport ASC LIMIT ?1"
    ))?;
    let rows = stmt
        .query_map(params![limit], row_from)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn mark_synced(conn: &Connection, uuid: &str, lamport: i64) -> anyhow::Result<()> {
    // Nur markieren, wenn die Zeile seit dem Push nicht erneut geändert wurde.
    conn.execute(
        "UPDATE entries SET sync_state = 1 WHERE uuid = ?1 AND lamport = ?2",
        params![uuid, lamport],
    )?;
    Ok(())
}

/// Für den Gruppenbeitritt: gesamte Historie erneut hochladen.
pub fn mark_all_dirty(conn: &Connection) -> anyhow::Result<()> {
    conn.execute("UPDATE entries SET sync_state = 0", [])?;
    Ok(())
}

/// Nur Schlüsselrotation: Ciphertexte ersetzen, Metadaten unangetastet.
pub fn update_cipher(
    conn: &Connection,
    uuid: &str,
    cipher: Option<&[u8]>,
    thumb: Option<&[u8]>,
) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE entries SET cipher = ?2, thumb = ?3 WHERE uuid = ?1",
        params![uuid, cipher, thumb],
    )?;
    Ok(())
}

/// Remote-Eintrag übernehmen (LWW-Sieger). Normale Updates bewahren die lokale
/// Source-App; ein Tombstone entfernt sie wie eine lokale Löschung.
pub fn upsert_remote(conn: &Connection, row: &EntryRow) -> anyhow::Result<()> {
    conn.execute(
        "INSERT INTO entries(uuid, kind, cipher, thumb, size_bytes, hash, created_at, pinned, deleted, device_id, lamport, sync_state, source_app_id, source_app_name, first_created_at, copy_count)
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, NULL, NULL, ?13, 1)
         ON CONFLICT(uuid) DO UPDATE SET
           kind = excluded.kind, cipher = excluded.cipher, thumb = excluded.thumb,
           size_bytes = excluded.size_bytes, hash = excluded.hash, created_at = excluded.created_at,
           pinned = excluded.pinned, deleted = excluded.deleted, device_id = excluded.device_id,
           lamport = excluded.lamport, sync_state = excluded.sync_state,
           source_app_id = CASE WHEN excluded.deleted THEN NULL ELSE entries.source_app_id END,
           source_app_name = CASE WHEN excluded.deleted THEN NULL ELSE entries.source_app_name END",
        params![
            row.uuid,
            row.kind,
            row.cipher,
            row.thumb,
            row.size_bytes,
            row.hash,
            row.created_at,
            row.pinned,
            row.deleted,
            row.device_id,
            row.lamport,
            SYNC_SYNCED,
            row.created_at, // first_created_at fallback for remote insert
        ],
    )?;
    Ok(())
}

/// Alle Zeilen inkl. Tombstones (für Schlüsselrotation).
pub fn list_all(conn: &Connection) -> anyhow::Result<Vec<EntryRow>> {
    let mut stmt = conn.prepare(&format!("SELECT {SELECT_COLS} FROM entries"))?;
    let rows = stmt
        .query_map([], row_from)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Älteste ungepinnte Einträge über dem Limit LOKAL löschen. Gibt die uuids zurück.
/// Bewusst kein Sync-Tombstone: max_entries ist ein Geräte-Limit — sonst würde
/// das Gerät mit dem kleinsten Limit die Historie ALLER Gruppen-Geräte stutzen.
pub fn prune(conn: &Connection, max_entries: u32) -> anyhow::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT uuid FROM entries WHERE deleted = 0 AND pinned = 0
         ORDER BY created_at DESC LIMIT -1 OFFSET ?1",
    )?;
    // OFFSET zählt über alle aktiven Einträge; gepinnte belegen ebenfalls Plätze,
    // werden aber nie selbst entfernt.
    let active: i64 =
        conn.query_row("SELECT COUNT(*) FROM entries WHERE deleted = 0", [], |r| {
            r.get(0)
        })?;
    let overflow = active - max_entries as i64;
    if overflow <= 0 {
        return Ok(vec![]);
    }
    let unpinned_keep = {
        let unpinned: i64 = conn.query_row(
            "SELECT COUNT(*) FROM entries WHERE deleted = 0 AND pinned = 0",
            [],
            |r| r.get(0),
        )?;
        (unpinned - overflow).max(0)
    };
    let uuids = stmt
        .query_map(params![unpinned_keep], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for uuid in &uuids {
        conn.execute("DELETE FROM entries WHERE uuid = ?1", params![uuid])?;
    }
    Ok(uuids)
}

/// Alle ungepinnten Einträge tombstonen (UI-Aktion „Historie löschen").
/// Anders als prune() eine bewusste Nutzer-Löschung → synct auf alle Geräte.
pub fn clear_unpinned(conn: &Connection) -> anyhow::Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT uuid FROM entries WHERE deleted = 0 AND pinned = 0")?;
    let uuids = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    let base = next_lamport_batch(conn, uuids.len() as i64)?;
    for (i, uuid) in uuids.iter().enumerate() {
        mark_deleted(conn, uuid, base + i as i64)?;
    }
    Ok(uuids)
}

/// Irgendeine Zeile mit Ciphertext — Probe für die Schlüssel-Recovery beim Start.
pub fn probe_cipher(conn: &Connection) -> anyhow::Result<Option<(String, u8, Vec<u8>)>> {
    Ok(conn
        .query_row(
            "SELECT uuid, kind, cipher FROM entries WHERE cipher IS NOT NULL LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?)
}

/// Nur Thumbnail + kind laden (entry_thumb braucht den großen cipher-Blob nicht).
pub fn get_thumb(conn: &Connection, uuid: &str) -> anyhow::Result<Option<(u8, Option<Vec<u8>>)>> {
    Ok(conn
        .query_row(
            "SELECT kind, thumb FROM entries WHERE uuid = ?1",
            params![uuid],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

/// Entschlüsselte Payload → anzeigbarer Text (Text direkt, Dateiliste als Zeilen).
pub fn payload_to_text(kind: u8, plain: &[u8]) -> Option<String> {
    match kind {
        KIND_TEXT => Some(String::from_utf8_lossy(plain).into_owned()),
        KIND_FILES => serde_json::from_slice::<Vec<String>>(plain)
            .ok()
            .map(|v| v.join("\n")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_sets_v2_and_columns() {
        let dir = std::env::temp_dir().join(format!("tippit-mig-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.db");
        {
            let conn = Connection::open(&path).unwrap();
            migrate(&conn).unwrap();
            assert_eq!(
                meta_get(&conn, "schema_version").unwrap().as_deref(),
                Some("3")
            );
            assert!(column_exists(&conn, "entries", "source_app_id").unwrap());
            assert!(column_exists(&conn, "entries", "source_app_name").unwrap());
            assert!(column_exists(&conn, "entries", "first_created_at").unwrap());
            assert!(column_exists(&conn, "entries", "copy_count").unwrap());
            // second migrate is no-op
            migrate(&conn).unwrap();
            assert_eq!(
                meta_get(&conn, "schema_version").unwrap().as_deref(),
                Some("3")
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn touch_keep_set_clear() {
        let dir = std::env::temp_dir().join(format!("tippit-touch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.db");
        let conn = Connection::open(&path).unwrap();
        migrate(&conn).unwrap();
        let row = EntryRow {
            uuid: "u1".into(),
            kind: KIND_TEXT,
            cipher: None,
            thumb: None,
            size_bytes: 0,
            hash: vec![1; 32],
            created_at: 1,
            pinned: false,
            deleted: false,
            device_id: "d".into(),
            lamport: 1,
            source_app_id: Some("com.chrome".into()),
            source_app_name: Some("Chrome".into()),
            first_created_at: 1,
            copy_count: 1,
        };
        insert(&conn, &row).unwrap();
        touch(&conn, "u1", 2, 2, TouchSource::Keep).unwrap();
        let got = get(&conn, "u1").unwrap().unwrap();
        assert_eq!(got.source_app_name.as_deref(), Some("Chrome"));
        assert_eq!(got.created_at, 2);
        touch(
            &conn,
            "u1",
            3,
            3,
            TouchSource::Set {
                id: "com.safari",
                name: "Safari",
            },
        )
        .unwrap();
        let got = get(&conn, "u1").unwrap().unwrap();
        assert_eq!(got.source_app_name.as_deref(), Some("Safari"));
        touch(&conn, "u1", 4, 4, TouchSource::Clear).unwrap();
        let got = get(&conn, "u1").unwrap().unwrap();
        assert!(got.source_app_id.is_none());
        assert!(got.source_app_name.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remote_update_preserves_source_but_tombstone_clears_it() {
        let dir = std::env::temp_dir().join(format!("tippit-remote-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("t.db");
        let conn = Connection::open(&path).unwrap();
        migrate(&conn).unwrap();
        let local = EntryRow {
            uuid: "u1".into(),
            kind: KIND_TEXT,
            cipher: Some(vec![1]),
            thumb: None,
            size_bytes: 1,
            hash: vec![1; 32],
            created_at: 1,
            pinned: false,
            deleted: false,
            device_id: "local".into(),
            lamport: 1,
            source_app_id: Some("com.chrome".into()),
            source_app_name: Some("Chrome".into()),
            first_created_at: 1,
            copy_count: 1,
        };
        insert(&conn, &local).unwrap();

        let mut remote = local.clone();
        remote.device_id = "remote".into();
        remote.lamport = 2;
        remote.source_app_id = None;
        remote.source_app_name = None;
        upsert_remote(&conn, &remote).unwrap();
        let got = get(&conn, "u1").unwrap().unwrap();
        assert_eq!(got.source_app_name.as_deref(), Some("Chrome"));

        remote.deleted = true;
        remote.cipher = None;
        remote.lamport = 3;
        upsert_remote(&conn, &remote).unwrap();
        let got = get(&conn, "u1").unwrap().unwrap();
        assert!(got.source_app_id.is_none());
        assert!(got.source_app_name.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
