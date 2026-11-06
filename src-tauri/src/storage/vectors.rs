//! Vektoren der Suche nach Bedeutung: verschlüsselt in `history.db`
//! (Tabelle `vectors`, Schema v5), entschlüsselt nur im RAM.
//!
//! Ein Vektor gehört zu genau einem Eintrag und einem Modell (`model`, z. B.
//! `granite-835ad14087e1`). Endgültig gelöschte Einträge nehmen ihren Vektor per
//! Trigger mit; Vektoren eines anderen Modells räumt `delete_stale` weg.

use std::collections::{HashMap, HashSet};

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

    /// Nur Vektoren dieser Einträge behalten (endgültig gelöschte fallen weg).
    pub fn retain(&mut self, keep: &HashSet<&str>) {
        self.rows.retain(|u, _| keep.contains(u.as_str()));
    }

    /// RAM der Vektoren in Bytes (ohne Verwaltung der Map).
    pub fn bytes(&self) -> usize {
        self.rows.values().map(|v| v.len() * 4).sum()
    }

    /// Kosinus zur (normalisierten) Anfrage, falls der Eintrag einen Vektor hat.
    pub fn cosine(&self, uuid: &str, query: &[f32]) -> Option<f32> {
        self.rows
            .get(uuid)
            .filter(|v| v.len() == query.len())
            .map(|v| dot(v, query))
    }
}

/// Treffer nach Bedeutung: Einträge, die der Anfrage deutlich ähnlicher sind
/// als der Rest. `all` sind die Kosinus aller aktiven Einträge (die
/// Verteilung), `eligible` die davon, die als Treffer infrage kommen (Filter,
/// `-wort`, kein Worttreffer). Ein Treffer liegt mindestens `MIN_Z`
/// Standardabweichungen über dem Mittel und höchstens `MAX_GAP` hinter dem
/// besten infrage kommenden; die besten `MAX_SIMILAR`, bester zuerst.
/// Relativ statt mit fester Schwelle, weil Granite eng streut (fast alles
/// liegt zwischen 0,7 und 0,8) und eine neue Modellrevision anders streuen kann.
pub fn pick_similar<T>(all: &[f32], mut eligible: Vec<(T, f32)>) -> Vec<(T, f32)> {
    if all.len() < MIN_VECTORS || eligible.is_empty() {
        return Vec::new();
    }
    let n = all.len() as f32;
    let mean = all.iter().sum::<f32>() / n;
    let sd = (all.iter().map(|s| (s - mean).powi(2)).sum::<f32>() / n).sqrt();
    if sd.is_nan() || sd <= 0.0 {
        return Vec::new();
    }
    let best = eligible.iter().map(|s| s.1).fold(f32::MIN, f32::max);
    let floor = (mean + MIN_Z * sd).max(best - MAX_GAP * sd);
    eligible.retain(|s| s.1 >= floor);
    eligible.sort_by(|a, b| b.1.total_cmp(&a.1));
    eligible.truncate(MAX_SIMILAR);
    eligible
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
    let plain = encode(model, v);
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
        let v = crypto::decrypt(keys, &uuid, AAD_VECTOR, &cipher)
            .ok()
            .and_then(|plain| decode(model, &plain));
        match v {
            Some(v) => {
                out.insert(uuid, v);
            }
            None => tracing::warn!("Vektor zu {uuid} nicht lesbar, wird neu berechnet"),
        }
    }
    Ok(out)
}

/// Klartext `len(model) ‖ model ‖ f32 LE …`: die Modellkennung steht
/// authentifiziert im Ciphertext, die Spalte `model` dient nur dem Filtern.
/// Sonst machte ein umetikettierter alter Vektor die Neuberechnung zunichte.
fn encode(model: &str, v: &[f32]) -> Vec<u8> {
    let id = model.as_bytes();
    let mut out = Vec::with_capacity(1 + id.len() + v.len() * 4);
    out.push(u8::try_from(id.len()).expect("Modellkennung über 255 Bytes"));
    out.extend_from_slice(id);
    out.extend(v.iter().flat_map(|x| x.to_le_bytes()));
    out
}

/// Vektor nur, wenn er zu `model` gehört.
fn decode(model: &str, plain: &[u8]) -> Option<Vec<f32>> {
    let (&len, rest) = plain.split_first()?;
    let (id, floats) = rest.split_at_checked(usize::from(len))?;
    if id != model.as_bytes() || floats.len() % 4 != 0 {
        return None;
    }
    Some(
        floats
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect(),
    )
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
        // Alter Vektor umetikettiert: die Kennung im Ciphertext passt nicht.
        conn.execute("UPDATE vectors SET model = 'm1' WHERE uuid = 'c'", [])
            .unwrap();
        assert!(!load(&conn, &keys, "m1").unwrap().contains_key("c"));
        conn.execute("UPDATE vectors SET model = 'm0' WHERE uuid = 'c'", [])
            .unwrap();

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
    /// Was nicht infrage kommt (Papierkorb, Filter), belegt keinen der fünf
    /// Plätze und verschiebt nicht den Abstand zum besten.
    #[test]
    fn similar_picks_clear_outliers_only() {
        let unit = |a: f32| vec![a.cos(), a.sin()];
        let mut rows: HashMap<String, Vec<f32>> = (0..100)
            .map(|i| (format!("n{i}"), unit(1.2 + (i % 5) as f32 * 0.05)))
            .collect();
        rows.insert("treffer".into(), unit(0.02));
        rows.insert("nah".into(), unit(0.3));
        for i in 0..6 {
            rows.insert(format!("hit{i}"), unit(0.001 * i as f32));
        }
        let mut store = VectorStore::new("m", rows);
        let q = unit(0.0);
        let cos = |store: &VectorStore, ids: &[&str]| -> Vec<(String, f32)> {
            ids.iter()
                .filter_map(|u| Some((u.to_string(), store.cosine(u, &q)?)))
                .collect()
        };
        let background: Vec<String> = (0..100).map(|i| format!("n{i}")).collect();
        let mut active: Vec<&str> = background.iter().map(String::as_str).collect();
        active.extend(["treffer", "nah"]);
        let all: Vec<f32> = cos(&store, &active).into_iter().map(|c| c.1).collect();
        let ids = |hits: Vec<(String, f32)>| hits.into_iter().map(|h| h.0).collect::<Vec<_>>();
        assert_eq!(
            ids(pick_similar(&all, cos(&store, &active))),
            ["treffer", "nah"]
        );

        // hit0–4 sind gelöscht oder gefiltert, nur hit5 kommt infrage.
        let mut eligible = active.clone();
        eligible.push("hit5");
        assert_eq!(ids(pick_similar(&all, cos(&store, &eligible)))[0], "hit5");

        let keep: HashSet<&str> = active.iter().copied().collect();
        store.retain(&keep);
        assert!(!store.contains("hit0") && store.contains("treffer"));

        assert!(pick_similar(&all[..MIN_VECTORS - 1], cos(&store, &active)).is_empty());
    }
}
