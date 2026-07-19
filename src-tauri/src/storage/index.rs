use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use serde::Serialize;

use super::crypto::{self, CryptoKeys};
use super::db::{EntryRow, KIND_FILES, KIND_IMAGE, KIND_TEXT};

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
        if row.deleted {
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

    pub fn touch(&mut self, uuid: &str, created_at: i64) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.dto.uuid == uuid) {
            e.dto.created_at = created_at;
        }
        self.sort();
    }

    pub fn set_pinned(&mut self, uuid: &str, pinned: bool) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.dto.uuid == uuid) {
            e.dto.pinned = pinned;
        }
    }

    /// Leere Query → alles (neueste zuerst); sonst Fuzzy-Score.
    /// Gepinnte immer vor ungepinnten.
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
            b.pinned
                .cmp(&a.pinned)
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
    if row.deleted {
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
        KIND_IMAGE => {
            let kb = (row.size_bytes as f64 / 1024.0).round().max(1.0);
            (String::new(), format!("Bild ({kb:.0} KB)"))
        }
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
        },
        haystack,
    })
}

fn make_preview(text: &str, kind: u8) -> String {
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
