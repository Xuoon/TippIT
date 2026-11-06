//! Vektoren der Suche nach Bedeutung: verschlüsselt in `history.db`
//! (Tabelle `vectors`, Schema v5), entschlüsselt nur im RAM.
//!
//! Ein Vektor gehört zu genau einem Eintrag und einem Modell (`model`, z. B.
//! `granite-835ad14087e1`). Endgültig gelöschte Einträge nehmen ihren Vektor per
//! Trigger mit; Vektoren eines anderen Modells räumt `delete_stale` weg.

use std::collections::HashMap;

use rusqlite::{params, Connection};

use super::crypto::{self, CryptoKeys};
use super::db::AAD_VECTOR;

/// Normalisierte Vektoren des aktiven Modells, je Eintrag.
#[derive(Default)]
pub struct VectorStore {
    pub model: String,
    rows: HashMap<String, Vec<f32>>,
}

/// Unter so vielen Vektoren sagt die Verteilung der Ähnlichkeiten zu wenig.
const MIN_VECTORS: usize = 20;
/// Ein Treffer muss so viele Standardabweichungen über dem Mittel liegen und
/// darf höchstens `MAX_GAP` hinter dem besten zurückbleiben. Kalibriert mit 24
/// deutschen Alltagsanfragen gegen 1525 Einträge: 13 von 24
/// Zielen, rund 1,4 weitere Zeilen je Anfrage, kaum Treffer bei fremden Themen.
const MIN_Z: f32 = 3.0;
const MAX_GAP: f32 = 0.5;
/// Höchstens so viele Treffer nach Bedeutung je Suche.
pub const MAX_SIMILAR: usize = 5;

impl VectorStore {
    pub fn new(model: &str, rows: HashMap<String, Vec<f32>>) -> Self {
        Self {
            model: model.into(),
            rows,
        }
    }

    pub fn contains(&self, uuid: &str) -> bool {
        self.rows.contains_key(uuid)
    }

    pub fn insert(&mut self, uuid: String, v: Vec<f32>) {
        self.rows.insert(uuid, v);
    }

    /// RAM der Vektoren in Bytes (ohne Verwaltung der Map).
    pub fn bytes(&self) -> usize {
        self.rows.values().map(|v| v.len() * 4).sum()
    }

    /// Einträge, die der Anfrage deutlich ähnlicher sind als der Rest:
    /// Kosinus mindestens `MIN_Z` Standardabweichungen über dem Mittel aller
    /// Vektoren, die besten `MAX_SIMILAR`, bester zuerst. Relativ statt mit
    /// fester Schwelle, weil Granite eng streut (fast alles liegt zwischen
    /// 0,7 und 0,8) und eine neue Modellrevision anders streuen kann.
    pub fn similar(&self, query: &[f32]) -> Vec<(String, f32)> {
        if self.rows.len() < MIN_VECTORS {
            return Vec::new();
        }
        let sims: Vec<(&String, f32)> = self
            .rows
            .iter()
            .filter(|(_, v)| v.len() == query.len())
            .map(|(u, v)| (u, dot(v, query)))
            .collect();
        let n = sims.len() as f32;
        let mean = sims.iter().map(|s| s.1).sum::<f32>() / n;
        let var = sims.iter().map(|s| (s.1 - mean).powi(2)).sum::<f32>() / n;
        let sd = var.sqrt();
        if sd.is_nan() || sd <= 0.0 {
            return Vec::new();
        }
        let best = sims.iter().map(|s| s.1).fold(f32::MIN, f32::max);
        let floor = (mean + MIN_Z * sd).max(best - MAX_GAP * sd);
        let mut hits: Vec<(String, f32)> = sims
            .into_iter()
            .filter(|s| s.1 >= floor)
            .map(|(u, s)| (u.clone(), s))
            .collect();
        hits.sort_by(|a, b| b.1.total_cmp(&a.1));
        hits.truncate(MAX_SIMILAR);
        hits
    }
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Belegung auf der Platte.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct VectorStats {
    /// Vektoren des aktiven Modells.
    pub current: i64,
    /// Vektoren anderer Modelle oder ohne Eintrag: `delete_stale` entfernt sie.
    pub stale: i64,
    /// Summe der verschlüsselten Blobs in Bytes.
    pub bytes: i64,
}

pub fn put(
    conn: &Connection,
    keys: &CryptoKeys,
    uuid: &str,
    model: &str,
    v: &[f32],
) -> anyhow::Result<()> {
    let plain: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
    let cipher = crypto::encrypt(keys, uuid, AAD_VECTOR, &plain)?;
    conn.execute(
        "INSERT INTO vectors(uuid, model, cipher) VALUES(?1, ?2, ?3)
         ON CONFLICT(uuid) DO UPDATE SET model = excluded.model, cipher = excluded.cipher",
        params![uuid, model, cipher],
    )?;
    Ok(())
}

/// Alle Vektoren eines Modells, entschlüsselt. Unlesbare überspringt sie
/// (der Eintrag wird beim nächsten Lauf neu eingebettet).
pub fn load(
    conn: &Connection,
    keys: &CryptoKeys,
    model: &str,
) -> anyhow::Result<HashMap<String, Vec<f32>>> {
    let mut stmt = conn.prepare(
        "SELECT v.uuid, v.cipher FROM vectors v JOIN entries e ON e.uuid = v.uuid WHERE v.model = ?1",
    )?;
    let rows = stmt.query_map(params![model], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
    })?;
    let mut out = HashMap::new();
    for row in rows {
        let (uuid, cipher) = row?;
        match crypto::decrypt(keys, &uuid, AAD_VECTOR, &cipher) {
            Ok(plain) if plain.len() % 4 == 0 => {
                let v = plain
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| f32::from_le_bytes(*b))
                    .collect();
                out.insert(uuid, v);
            }
            _ => tracing::warn!("Vektor zu {uuid} nicht lesbar, wird neu berechnet"),
        }
    }
    Ok(out)
}

pub fn stats(conn: &Connection, model: &str) -> anyhow::Result<VectorStats> {
    Ok(conn.query_row(
        "SELECT
            COALESCE(SUM(model = ?1 AND uuid IN (SELECT uuid FROM entries)), 0),
            COALESCE(SUM(model != ?1 OR uuid NOT IN (SELECT uuid FROM entries)), 0),
            COALESCE(SUM(length(cipher)), 0)
         FROM vectors",
        params![model],
        |r| {
            Ok(VectorStats {
                current: r.get(0)?,
                stale: r.get(1)?,
                bytes: r.get(2)?,
            })
        },
    )?)
}

/// Vektoren aller Modelle außer `keep` und verwaiste entfernen; liefert die Anzahl.
pub fn delete_stale(conn: &Connection, keep: &[String]) -> anyhow::Result<usize> {
    let marks = vec!["?"; keep.len()].join(",");
    Ok(conn.execute(
        &format!(
            "DELETE FROM vectors WHERE model NOT IN ({marks}) OR uuid NOT IN (SELECT uuid FROM entries)"
        ),
        rusqlite::params_from_iter(keep),
    )?)
}

pub fn delete_all(conn: &Connection) -> anyhow::Result<usize> {
    Ok(conn.execute("DELETE FROM vectors", [])?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::crypto::Secret;
    use crate::storage::db::{self, EntryRow, KIND_TEXT};

    fn entry(uuid: &str) -> EntryRow {
        EntryRow {
            uuid: uuid.into(),
            kind: KIND_TEXT,
            cipher: Some(vec![1]),
            thumb: None,
            html: None,
            size_bytes: 1,
            hash: uuid.as_bytes().to_vec(),
            created_at: 1,
            pinned: false,
            trashed_at: 0,
            snippet: false,
            source_app_id: None,
            source_app_name: None,
            first_created_at: 1,
            copy_count: 1,
        }
    }

    /// Verschlüsselt gespeichert, an Eintrag und Modell gebunden, und beim
    /// endgültigen Löschen des Eintrags verschwindet auch der Vektor.
    #[test]
    fn vectors_roundtrip_bind_and_follow_entries() {
        let keys = Secret::generate().unwrap().derive_keys();
        let conn = Connection::open_in_memory().unwrap();
        db::migrate(&conn).unwrap();
        for u in ["a", "b", "c"] {
            db::insert(&conn, &entry(u)).unwrap();
        }
        put(&conn, &keys, "a", "m1", &[0.5, -1.25]).unwrap();
        put(&conn, &keys, "b", "m1", &[1.0, 0.0]).unwrap();
        put(&conn, &keys, "c", "m0", &[0.0, 1.0]).unwrap();

        let raw: Vec<u8> = conn
            .query_row("SELECT cipher FROM vectors WHERE uuid = 'a'", [], |r| {
                r.get(0)
            })
            .unwrap();
        let plain: Vec<u8> = [0.5f32, -1.25]
            .iter()
            .flat_map(|x| x.to_le_bytes())
            .collect();
        assert!(!raw.windows(plain.len()).any(|w| w == plain.as_slice()));

        let m1 = load(&conn, &keys, "m1").unwrap();
        assert_eq!(m1.len(), 2);
        assert_eq!(m1["a"], [0.5, -1.25]);
        // Blob einer anderen Zeile untergeschoben: AAD passt nicht, Vektor fehlt.
        conn.execute(
            "UPDATE vectors SET cipher = (SELECT cipher FROM vectors WHERE uuid = 'a') WHERE uuid = 'b'",
            [],
        )
        .unwrap();
        assert!(!load(&conn, &keys, "m1").unwrap().contains_key("b"));

        assert_eq!(stats(&conn, "m1").unwrap().current, 2);
        assert_eq!(stats(&conn, "m1").unwrap().stale, 1);
        db::purge(&conn, "a").unwrap();
        assert_eq!(stats(&conn, "m1").unwrap().current, 1);
        assert_eq!(delete_stale(&conn, &["m1".into()]).unwrap(), 1);
        assert_eq!(stats(&conn, "m1").unwrap().stale, 0);
        assert_eq!(delete_all(&conn).unwrap(), 1);
    }

    /// Nur was deutlich herausragt, zählt; bei wenigen Vektoren gar nichts.
    #[test]
    fn similar_picks_clear_outliers_only() {
        let unit = |a: f32| vec![a.cos(), a.sin()];
        let mut rows: HashMap<String, Vec<f32>> = (0..40)
            .map(|i| (format!("n{i}"), unit(1.2 + (i % 5) as f32 * 0.05)))
            .collect();
        rows.insert("treffer".into(), unit(0.02));
        rows.insert("nah".into(), unit(0.3));
        let store = VectorStore::new("m", rows);
        let hits: Vec<String> = store.similar(&unit(0.0)).into_iter().map(|h| h.0).collect();
        assert_eq!(hits, ["treffer", "nah"]);

        let few: HashMap<_, _> = (0..MIN_VECTORS - 1)
            .map(|i| (format!("n{i}"), unit(i as f32)))
            .collect();
        assert!(VectorStore::new("m", few).similar(&unit(0.0)).is_empty());
    }
}
