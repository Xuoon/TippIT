use rusqlite::{params, Connection, OptionalExtension};

use super::paths::AppPaths;

pub const KIND_TEXT: u8 = 0;
pub const KIND_IMAGE: u8 = 1;
pub const KIND_FILES: u8 = 2;
/// Kein Eintragstyp, sondern das AAD-Byte der `html`-Spalte: so lässt sich der
/// Rich-Text-Blob einer Zeile nicht gegen ihren Klartext-Blob tauschen.
pub const AAD_HTML: u8 = 3;

/// Wie lange gelöschte Einträge im Papierkorb bleiben, bevor sie endgültig gehen.
pub const TRASH_RETENTION_DAYS: i64 = 30;

#[derive(Clone, Debug)]
pub struct EntryRow {
    pub uuid: String,
    pub kind: u8,
    pub cipher: Option<Vec<u8>>,
    pub thumb: Option<Vec<u8>>,
    /// Sanitisiertes Clipboard-HTML, verschlüsselt (AAD-kind = [`AAD_HTML`]).
    /// Nur bei KIND_TEXT gesetzt und nur, wenn die Quelle Formatierung lieferte.
    pub html: Option<Vec<u8>>,
    pub size_bytes: i64,
    pub hash: Vec<u8>,
    pub created_at: i64,
    pub pinned: bool,
    /// 0 = aktiv, sonst Zeitpunkt der Löschung (Papierkorb).
    pub trashed_at: i64,
    /// Dauerhafter Textbaustein: nie durch Limit, Alter oder „Historie löschen" entfernt.
    pub snippet: bool,
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
    // CREATE bleibt v1-Shape; jede Erweiterung läuft als ALTER-Schritt darüber,
    // damit frische und gewachsene Datenbanken exakt denselben Pfad nehmen.
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
        CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT);
        "#,
    )?;
    if meta_get(conn, "schema_version")?.is_none() {
        meta_set(conn, "schema_version", "1")?;
    }
    loop {
        let v = meta_get(conn, "schema_version")?.unwrap_or_else(|| "1".into());
        match v.as_str() {
            "1" => migrate_v2(conn)?,
            "2" => migrate_v3(conn)?,
            "3" => migrate_v4(conn)?,
            "4" => break,
            other => {
                tracing::warn!("unbekannte schema_version={other}, skip migrate");
                break;
            }
        }
    }
    Ok(())
}

/// schema 1 → 2: Quellanwendung.
fn migrate_v2(conn: &Connection) -> anyhow::Result<()> {
    if !column_exists(conn, "entries", "source_app_id")? {
        conn.execute("ALTER TABLE entries ADD COLUMN source_app_id TEXT", [])?;
    }
    if !column_exists(conn, "entries", "source_app_name")? {
        conn.execute("ALTER TABLE entries ADD COLUMN source_app_name TEXT", [])?;
    }
    meta_set(conn, "schema_version", "2")
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
    meta_set(conn, "schema_version", "3")
}

/// schema 3 → 4: Sync-Rückbau. `deleted`/`lamport`/`sync_state`/`device_id` waren
/// reine Replikations-Metadaten; an ihre Stelle treten Papierkorb (`trashed_at`),
/// Textbausteine (`snippet`) und Rich-Text (`html`).
fn migrate_v4(conn: &Connection) -> anyhow::Result<()> {
    // Indizes zuerst: SQLite verweigert DROP COLUMN auf indizierten Spalten.
    conn.execute_batch(
        "DROP INDEX IF EXISTS idx_entries_dirty;
         DROP INDEX IF EXISTS idx_entries_created;
         DROP INDEX IF EXISTS idx_entries_hash;",
    )?;
    if !column_exists(conn, "entries", "trashed_at")? {
        conn.execute(
            "ALTER TABLE entries ADD COLUMN trashed_at INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if !column_exists(conn, "entries", "snippet")? {
        conn.execute(
            "ALTER TABLE entries ADD COLUMN snippet INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
    }
    if !column_exists(conn, "entries", "html")? {
        conn.execute("ALTER TABLE entries ADD COLUMN html BLOB", [])?;
    }
    if column_exists(conn, "entries", "deleted")? {
        // Alte Sync-Grabsteine tragen keinen Inhalt mehr und sind offline wertlos.
        conn.execute("DELETE FROM entries WHERE deleted = 1", [])?;
    }
    // Reihenfolge: erst die Indizes oben, sonst verweigert SQLite DROP COLUMN.
    for column in ["deleted", "sync_state", "lamport", "device_id"] {
        if column_exists(conn, "entries", column)? {
            conn.execute(&format!("ALTER TABLE entries DROP COLUMN {column}"), [])?;
        }
    }
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_entries_created ON entries(trashed_at, created_at DESC);
         CREATE INDEX IF NOT EXISTS idx_entries_hash ON entries(hash) WHERE trashed_at = 0;
         CREATE INDEX IF NOT EXISTS idx_entries_trash ON entries(trashed_at) WHERE trashed_at > 0;",
    )?;
    meta_set(conn, "schema_version", "4")
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

/// Existiert ein aktiver (nicht im Papierkorb liegender) Eintrag mit diesem
/// Inhalt bereits? → uuid. Textbausteine bleiben außen vor: eine erneute Kopie
/// desselben Texts soll den Baustein nicht als Historieneintrag umdeuten.
pub fn find_by_hash(conn: &Connection, hash: &[u8]) -> anyhow::Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT uuid FROM entries WHERE hash = ?1 AND trashed_at = 0 AND snippet = 0 LIMIT 1",
            params![hash],
            |r| r.get(0),
        )
        .optional()?)
}

const INSERT_COLS: &str = "uuid, kind, cipher, thumb, html, size_bytes, hash, created_at, pinned, trashed_at, snippet, source_app_id, source_app_name, first_created_at, copy_count";

pub fn insert(conn: &Connection, row: &EntryRow) -> anyhow::Result<()> {
    conn.execute(
        &format!(
            "INSERT INTO entries({INSERT_COLS})
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)"
        ),
        params![
            row.uuid,
            row.kind,
            row.cipher,
            row.thumb,
            row.html,
            row.size_bytes,
            row.hash,
            row.created_at,
            row.pinned,
            row.trashed_at,
            row.snippet,
            row.source_app_id,
            row.source_app_name,
            row.first_created_at,
            row.copy_count,
        ],
    )?;
    Ok(())
}

/// Duplikat „nach oben schieben": Zeitstempel; Source per [`TouchSource`].
pub fn touch(
    conn: &Connection,
    uuid: &str,
    now_ms: i64,
    source: TouchSource<'_>,
) -> anyhow::Result<()> {
    match source {
        TouchSource::Keep => {
            conn.execute(
                "UPDATE entries SET created_at = ?2, copy_count = copy_count + 1 WHERE uuid = ?1",
                params![uuid, now_ms],
            )?;
        }
        TouchSource::Set { id, name } => {
            conn.execute(
                "UPDATE entries SET created_at = ?2, source_app_id = ?3, source_app_name = ?4,
                 copy_count = copy_count + 1 WHERE uuid = ?1",
                params![uuid, now_ms, id, name],
            )?;
        }
        TouchSource::Clear => {
            conn.execute(
                "UPDATE entries SET created_at = ?2, source_app_id = NULL, source_app_name = NULL,
                 copy_count = copy_count + 1 WHERE uuid = ?1",
                params![uuid, now_ms],
            )?;
        }
    }
    Ok(())
}

pub fn set_pinned(conn: &Connection, uuid: &str, pinned: bool) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE entries SET pinned = ?2 WHERE uuid = ?1",
        params![uuid, pinned],
    )?;
    Ok(())
}

pub fn set_snippet(conn: &Connection, uuid: &str, snippet: bool) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE entries SET snippet = ?2 WHERE uuid = ?1",
        params![uuid, snippet],
    )?;
    Ok(())
}

/// In den Papierkorb legen — Inhalt bleibt erhalten, bis er endgültig gelöscht wird.
pub fn trash(conn: &Connection, uuid: &str, now_ms: i64) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE entries SET trashed_at = ?2 WHERE uuid = ?1",
        params![uuid, now_ms],
    )?;
    Ok(())
}

pub fn restore(conn: &Connection, uuid: &str) -> anyhow::Result<()> {
    conn.execute(
        "UPDATE entries SET trashed_at = 0 WHERE uuid = ?1",
        params![uuid],
    )?;
    Ok(())
}

/// Endgültig entfernen (kein Weg zurück).
pub fn purge(conn: &Connection, uuid: &str) -> anyhow::Result<()> {
    conn.execute("DELETE FROM entries WHERE uuid = ?1", params![uuid])?;
    Ok(())
}

/// Papierkorb leeren, optional nur was älter als `before_ms` ist (0 = alles).
pub fn purge_trash(conn: &Connection, before_ms: i64) -> anyhow::Result<usize> {
    let n = conn.execute(
        "DELETE FROM entries WHERE trashed_at > 0 AND trashed_at <= ?1",
        params![if before_ms == 0 { i64::MAX } else { before_ms }],
    )?;
    Ok(n)
}

const SELECT_COLS: &str = "uuid, kind, cipher, thumb, html, size_bytes, hash, created_at, pinned, trashed_at, snippet, source_app_id, source_app_name, first_created_at, copy_count";

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
        "SELECT {SELECT_COLS} FROM entries WHERE trashed_at = 0 ORDER BY created_at DESC"
    ))?;
    let rows = stmt
        .query_map([], row_from)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Papierkorb-Inhalt, zuletzt Gelöschtes zuerst.
pub fn list_trashed(conn: &Connection) -> anyhow::Result<Vec<EntryRow>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {SELECT_COLS} FROM entries WHERE trashed_at > 0 ORDER BY trashed_at DESC"
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
        html: r.get(4)?,
        size_bytes: r.get(5)?,
        hash: r.get(6)?,
        created_at: r.get(7)?,
        pinned: r.get(8)?,
        trashed_at: r.get(9)?,
        snippet: r.get(10)?,
        source_app_id: r.get(11)?,
        source_app_name: r.get(12)?,
        first_created_at: r.get(13)?,
        copy_count: r.get(14)?,
    })
}

/// Alle Zeilen inkl. Papierkorb (für den Export).
pub fn list_all(conn: &Connection) -> anyhow::Result<Vec<EntryRow>> {
    let mut stmt = conn.prepare(&format!("SELECT {SELECT_COLS} FROM entries"))?;
    let rows = stmt
        .query_map([], row_from)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Älteste ungepinnte Einträge über dem Limit endgültig löschen. Gibt die uuids
/// zurück. Bewusst kein Papierkorb: das Limit greift bei jeder Kopie, ein voller
/// Papierkorb wäre nur eine zweite unbegrenzte Historie.
pub fn prune(conn: &Connection, max_entries: u32) -> anyhow::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT uuid FROM entries WHERE trashed_at = 0 AND pinned = 0 AND snippet = 0
         ORDER BY created_at DESC LIMIT -1 OFFSET ?1",
    )?;
    // OFFSET zählt über alle aktiven Einträge; gepinnte und Textbausteine belegen
    // ebenfalls Plätze, werden aber nie selbst entfernt.
    let active: i64 = conn.query_row(
        "SELECT COUNT(*) FROM entries WHERE trashed_at = 0",
        [],
        |r| r.get(0),
    )?;
    let overflow = active - max_entries as i64;
    if overflow <= 0 {
        return Ok(vec![]);
    }
    let keep = {
        let removable: i64 = conn.query_row(
            "SELECT COUNT(*) FROM entries WHERE trashed_at = 0 AND pinned = 0 AND snippet = 0",
            [],
            |r| r.get(0),
        )?;
        (removable - overflow).max(0)
    };
    let uuids = stmt
        .query_map(params![keep], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for uuid in &uuids {
        purge(conn, uuid)?;
    }
    Ok(uuids)
}

/// Aufbewahrungsfrist: alles Ungepinnte, das älter als `before_ms` ist, wandert
/// in den Papierkorb (nicht sofort weg — ein zu knapp eingestelltes Alter soll
/// nicht unwiederbringlich löschen).
pub fn trash_older_than(
    conn: &Connection,
    before_ms: i64,
    now_ms: i64,
) -> anyhow::Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT uuid FROM entries
         WHERE trashed_at = 0 AND pinned = 0 AND snippet = 0 AND created_at < ?1",
    )?;
    let uuids = stmt
        .query_map(params![before_ms], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for uuid in &uuids {
        trash(conn, uuid, now_ms)?;
    }
    Ok(uuids)
}

/// Alle ungepinnten Einträge in den Papierkorb legen (UI-Aktion „Historie löschen").
pub fn clear_unpinned(conn: &Connection, now_ms: i64) -> anyhow::Result<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT uuid FROM entries WHERE trashed_at = 0 AND pinned = 0 AND snippet = 0")?;
    let uuids = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    for uuid in &uuids {
        trash(conn, uuid, now_ms)?;
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

    fn temp_db(tag: &str) -> (std::path::PathBuf, Connection) {
        let dir = std::env::temp_dir().join(format!("tippit-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let conn = Connection::open(dir.join("t.db")).unwrap();
        migrate(&conn).unwrap();
        (dir, conn)
    }

    fn sample(uuid: &str) -> EntryRow {
        EntryRow {
            uuid: uuid.into(),
            kind: KIND_TEXT,
            cipher: Some(vec![1]),
            thumb: None,
            html: None,
            size_bytes: 1,
            hash: vec![1; 32],
            created_at: 1,
            pinned: false,
            trashed_at: 0,
            snippet: false,
            source_app_id: Some("com.chrome".into()),
            source_app_name: Some("Chrome".into()),
            first_created_at: 1,
            copy_count: 1,
        }
    }

    #[test]
    fn migrate_reaches_v4_and_drops_sync_columns() {
        let (dir, conn) = temp_db("mig");
        assert_eq!(
            meta_get(&conn, "schema_version").unwrap().as_deref(),
            Some("4")
        );
        for col in [
            "source_app_id",
            "first_created_at",
            "trashed_at",
            "snippet",
            "html",
        ] {
            assert!(column_exists(&conn, "entries", col).unwrap(), "{col} fehlt");
        }
        for col in ["deleted", "lamport", "sync_state", "device_id"] {
            assert!(
                !column_exists(&conn, "entries", col).unwrap(),
                "{col} sollte weg sein"
            );
        }
        // erneuter Lauf ist ein No-op
        migrate(&conn).unwrap();
        assert_eq!(
            meta_get(&conn, "schema_version").unwrap().as_deref(),
            Some("4")
        );
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn touch_keep_set_clear() {
        let (dir, conn) = temp_db("touch");
        insert(&conn, &sample("u1")).unwrap();
        touch(&conn, "u1", 2, TouchSource::Keep).unwrap();
        let got = get(&conn, "u1").unwrap().unwrap();
        assert_eq!(got.source_app_name.as_deref(), Some("Chrome"));
        assert_eq!(got.created_at, 2);
        touch(
            &conn,
            "u1",
            3,
            TouchSource::Set {
                id: "com.safari",
                name: "Safari",
            },
        )
        .unwrap();
        assert_eq!(
            get(&conn, "u1")
                .unwrap()
                .unwrap()
                .source_app_name
                .as_deref(),
            Some("Safari")
        );
        touch(&conn, "u1", 4, TouchSource::Clear).unwrap();
        let got = get(&conn, "u1").unwrap().unwrap();
        assert!(got.source_app_id.is_none());
        assert!(got.source_app_name.is_none());
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn trash_keeps_content_until_purged() {
        let (dir, conn) = temp_db("trash");
        insert(&conn, &sample("u1")).unwrap();
        trash(&conn, "u1", 5_000).unwrap();
        // Inhalt bleibt erhalten, damit Wiederherstellen etwas zurückbringt.
        let got = get(&conn, "u1").unwrap().unwrap();
        assert_eq!(got.trashed_at, 5_000);
        assert!(got.cipher.is_some());
        assert!(list_active(&conn).unwrap().is_empty());
        assert_eq!(list_trashed(&conn).unwrap().len(), 1);
        // Ein gelöschter Eintrag darf ein erneutes Kopieren nicht blockieren.
        assert!(find_by_hash(&conn, &[1; 32]).unwrap().is_none());
        restore(&conn, "u1").unwrap();
        assert_eq!(list_active(&conn).unwrap().len(), 1);
        trash(&conn, "u1", 5_000).unwrap();
        assert_eq!(purge_trash(&conn, 6_000).unwrap(), 1);
        assert!(get(&conn, "u1").unwrap().is_none());
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prune_and_retention_spare_pinned_and_snippets() {
        let (dir, conn) = temp_db("prune");
        for (i, uuid) in ["a", "b", "c", "d"].iter().enumerate() {
            let mut row = sample(uuid);
            row.created_at = 100 + i as i64;
            row.hash = vec![i as u8; 32];
            row.pinned = *uuid == "a";
            row.snippet = *uuid == "b";
            insert(&conn, &row).unwrap();
        }
        // Limit 3: ein Eintrag zu viel — entfernt wird der älteste Entfernbare.
        assert_eq!(prune(&conn, 3).unwrap(), vec!["c".to_string()]);
        assert_eq!(list_active(&conn).unwrap().len(), 3);
        // Aufbewahrungsfrist verschont Angepinntes und Bausteine ebenfalls.
        assert_eq!(
            trash_older_than(&conn, 1_000, 9_000).unwrap(),
            vec!["d".to_string()]
        );
        assert_eq!(list_active(&conn).unwrap().len(), 2);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
