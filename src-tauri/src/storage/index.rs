use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use serde::Serialize;

use super::crypto::{self, CryptoKeys};
use super::db::{EntryRow, TouchSource, KIND_FILES, KIND_IMAGE, KIND_TEXT};

/// Wieviel Text pro Eintrag maximal in den Suchindex wandert.
const MAX_INDEXED_CHARS: usize = 16 * 1024;
const PREVIEW_CHARS: usize = 200;

#[derive(Clone, Serialize)]
pub struct EntryDto {
    pub uuid: String,
    pub kind: u8,
    pub preview: String,
    pub created_at: i64,
    pub pinned: bool,
    pub size_bytes: i64,
    pub has_thumb: bool,
    pub source_app_id: Option<String>,
    pub source_app_name: Option<String>,
    pub first_created_at: i64,
    pub copy_count: i64,
    /// Dauerhafter Textbaustein statt erfasster Kopie.
    pub snippet: bool,
    /// Es liegt formatierte Fassung vor (Rich-Text-Einfügen möglich).
    pub has_html: bool,
}

struct IndexedEntry {
    dto: EntryDto,
    haystack: String,
}

/// Entschlüsselter In-Memory-Index: bei ≤ ein paar tausend Einträgen ist
/// Fuzzy-Matching über alles in <1 ms machbar — echtes Filtern beim Tippen.
pub struct SearchIndex {
    entries: Vec<IndexedEntry>,
    matcher: Matcher,
}

impl SearchIndex {
    pub fn build(rows: &[EntryRow], keys: &CryptoKeys) -> Self {
        let mut index = Self {
            entries: Vec::with_capacity(rows.len()),
            matcher: Matcher::new(Config::DEFAULT),
        };
        for row in rows {
            if let Some(entry) = indexed_from_row(row, keys) {
                index.entries.push(entry);
            }
        }
        index.sort();
        index
    }

    fn sort(&mut self) {
        self.entries
            .sort_by_key(|e| std::cmp::Reverse(e.dto.created_at));
    }

    pub fn upsert(&mut self, row: &EntryRow, keys: &CryptoKeys) {
        self.remove(&row.uuid);
        if row.trashed_at != 0 {
            return;
        }
        if let Some(entry) = indexed_from_row(row, keys) {
            self.entries.push(entry);
            self.sort();
        }
    }

    pub fn remove(&mut self, uuid: &str) {
        self.entries.retain(|e| e.dto.uuid != uuid);
    }

    /// Spiegel von `db::touch`: Zeitstempel, Zähler und Source nach derselben Policy.
    pub fn touch(&mut self, uuid: &str, created_at: i64, source: &TouchSource<'_>) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.dto.uuid == uuid) {
            e.dto.created_at = created_at;
            e.dto.copy_count = e.dto.copy_count.saturating_add(1);
            match source {
                TouchSource::Keep => {}
                TouchSource::Set { id, name } => {
                    e.dto.source_app_id = Some((*id).to_string());
                    e.dto.source_app_name = Some((*name).to_string());
                }
                TouchSource::Clear => {
                    e.dto.source_app_id = None;
                    e.dto.source_app_name = None;
                }
            }
        }
        self.sort();
    }

    pub fn set_pinned(&mut self, uuid: &str, pinned: bool) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.dto.uuid == uuid) {
            e.dto.pinned = pinned;
        }
    }

    pub fn set_snippet(&mut self, uuid: &str, snippet: bool) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.dto.uuid == uuid) {
            e.dto.snippet = snippet;
        }
    }

    /// Leere Query → alles (neueste zuerst); sonst Fuzzy-Score.
    /// Textbausteine stehen vor Angepinntem, Angepinntes vor dem Rest.
    pub fn search(&mut self, query: &str, kind: Option<u8>, limit: usize) -> Vec<EntryDto> {
        let query = query.trim();
        let mut scored: Vec<(u32, &EntryDto)> = if query.is_empty() {
            self.entries
                .iter()
                .filter(|e| kind.is_none_or(|k| e.dto.kind == k))
                .map(|e| (0u32, &e.dto))
                .collect()
        } else {
            let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
            let mut buf = Vec::new();
            let matcher = &mut self.matcher;
            self.entries
                .iter()
                .filter(|e| kind.is_none_or(|k| e.dto.kind == k))
                .filter_map(|e| {
                    pattern
                        .score(Utf32Str::new(&e.haystack, &mut buf), matcher)
                        .map(|s| (s, &e.dto))
                })
                .collect()
        };
        scored.sort_by(|(sa, a), (sb, b)| {
            b.snippet
                .cmp(&a.snippet)
                .then(b.pinned.cmp(&a.pinned))
                .then(sb.cmp(sa))
                .then(b.created_at.cmp(&a.created_at))
        });
        scored
            .into_iter()
            .take(limit)
            .map(|(_, d)| d.clone())
            .collect()
    }
}

fn indexed_from_row(row: &EntryRow, keys: &CryptoKeys) -> Option<IndexedEntry> {
    if row.trashed_at != 0 {
        return None;
    }
    let (haystack, preview) = match row.kind {
        KIND_TEXT | KIND_FILES => {
            let cipher = row.cipher.as_ref()?;
            let plain = match crypto::decrypt(keys, &row.uuid, row.kind, cipher) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!("Eintrag {} nicht entschlüsselbar: {e}", row.uuid);
                    return None;
                }
            };
            let text = super::db::payload_to_text(row.kind, &plain).unwrap_or_default();
            let haystack: String = text.chars().take(MAX_INDEXED_CHARS).collect();
            let preview = make_preview(&text, row.kind);
            (haystack, preview)
        }
        KIND_IMAGE => (String::new(), image_preview(row.size_bytes)),
        _ => return None,
    };
    Some(IndexedEntry {
        dto: EntryDto {
            uuid: row.uuid.clone(),
            kind: row.kind,
            preview,
            created_at: row.created_at,
            pinned: row.pinned,
            size_bytes: row.size_bytes,
            has_thumb: row.thumb.is_some(),
            source_app_id: row.source_app_id.clone(),
            source_app_name: row.source_app_name.clone(),
            first_created_at: if row.first_created_at > 0 {
                row.first_created_at
            } else {
                row.created_at
            },
            copy_count: row.copy_count.max(1),
            snippet: row.snippet,
            has_html: row.html.is_some(),
        },
        haystack,
    })
}

pub fn image_preview(size_bytes: i64) -> String {
    let kb = (size_bytes as f64 / 1024.0).round().max(1.0);
    format!("Bild ({kb:.0} KB)")
}

pub fn make_preview(text: &str, kind: u8) -> String {
    if kind == KIND_FILES {
        let mut lines = text.lines();
        let first = lines.next().unwrap_or("");
        let rest = lines.count();
        return if rest > 0 {
            format!("{first} (+{rest} weitere)")
        } else {
            first.to_string()
        };
    }
    let single_line: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    single_line.chars().take(PREVIEW_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::crypto::Secret;

    fn row(keys: &CryptoKeys, uuid: &str, kind: u8, text: &str, created_at: i64) -> EntryRow {
        let plain = if kind == KIND_FILES {
            serde_json::to_vec(&[text]).unwrap()
        } else {
            text.as_bytes().to_vec()
        };
        EntryRow {
            uuid: uuid.into(),
            kind,
            cipher: Some(crypto::encrypt(keys, uuid, kind, &plain).unwrap()),
            thumb: None,
            html: None,
            size_bytes: plain.len() as i64,
            hash: crypto::sha256(&plain).to_vec(),
            created_at,
            pinned: false,
            trashed_at: 0,
            snippet: false,
            source_app_id: None,
            source_app_name: None,
            first_created_at: created_at,
            copy_count: 1,
        }
    }

    fn uuids(entries: &[EntryDto]) -> Vec<&str> {
        entries.iter().map(|e| e.uuid.as_str()).collect()
    }

    /// Bausteine vor Angepinntem, Angepinntes vor dem Rest; innerhalb einer
    /// Stufe entscheidet bei einer Suche der Score, sonst das Datum.
    #[test]
    fn search_orders_snippets_pinned_score_then_time() {
        let keys = Secret::generate().unwrap().derive_keys();
        let mut snippet = row(&keys, "baustein", KIND_TEXT, "rechnung vorlage", 1);
        snippet.snippet = true;
        let mut pinned = row(&keys, "pin", KIND_TEXT, "r e c h n u n g", 2);
        pinned.pinned = true;
        let rows = vec![
            snippet,
            pinned,
            row(&keys, "alt", KIND_TEXT, "rechnung", 3),
            row(&keys, "neu", KIND_TEXT, "rechnung", 5),
            row(&keys, "unscharf", KIND_TEXT, "r-e-c-h-n-u-n-g", 9),
            row(&keys, "datei", KIND_FILES, "/tmp/rechnung.pdf", 4),
            row(&keys, "anderes", KIND_TEXT, "einkauf", 8),
        ];
        let mut index = SearchIndex::build(&rows, &keys);

        let all = index.search("", None, usize::MAX);
        assert_eq!(
            uuids(&all),
            ["baustein", "pin", "unscharf", "anderes", "neu", "datei", "alt"]
        );

        let hits = index.search("rechnung", None, usize::MAX);
        let order = uuids(&hits);
        assert_eq!(&order[..2], ["baustein", "pin"]);
        assert!(!order.contains(&"anderes"));
        // Gleicher Score: das Neuere zuerst; der exakte Treffer vor dem unscharfen.
        let pos = |u: &str| order.iter().position(|x| *x == u).unwrap();
        assert!(pos("neu") < pos("alt"));
        assert!(pos("neu") < pos("unscharf"));

        // Der Typ-Filter wirkt auf die leere und die echte Suche.
        assert_eq!(
            uuids(&index.search("", Some(KIND_FILES), usize::MAX)),
            ["datei"]
        );
        assert_eq!(
            uuids(&index.search("rechnung", Some(KIND_FILES), 10)),
            ["datei"]
        );
        assert_eq!(index.search("rechnung", None, 2).len(), 2);

        // Gelöschtes fällt beim upsert heraus.
        let mut trashed = row(&keys, "neu", KIND_TEXT, "rechnung", 5);
        trashed.trashed_at = 10;
        index.upsert(&trashed, &keys);
        assert!(!uuids(&index.search("", None, usize::MAX)).contains(&"neu"));
    }
}
