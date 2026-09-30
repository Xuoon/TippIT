use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use serde::{Deserialize, Serialize};

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

/// Treffer einer echten Suche bleiben gedeckelt: die schwächsten Fuzzy-Treffer
/// helfen niemandem, und die Sortierung nach Datum soll nur unter den besten laufen.
pub const MAX_HITS: usize = 200;
/// Ab so vielen Kandidaten lohnt es, das Scoring auf Threads zu verteilen.
const PARALLEL_MIN: usize = 1024;
const MAX_THREADS: usize = 8;

/// Sortierung der Liste; Bausteine und Angepinntes stehen immer vorn.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SortKey {
    #[default]
    LastCopy,
    FirstCopy,
    CopyCount,
    Size,
}

/// Filter, die mehr als den Eintragstyp prüfen (Spiegel von `FILTERS` in
/// `src/lib/entry-kinds.ts`).
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Refine {
    Pinned,
    Snippets,
    Links,
    Totp,
}

// Ohne Debug: query und keep können Inhalte der Zwischenablage tragen.
#[derive(Default, Deserialize)]
#[serde(default)]
pub struct SearchParams {
    pub query: String,
    pub kind: Option<u8>,
    pub refine: Option<Refine>,
    /// `None`: Reihenfolge nach Score, dann Zeit.
    pub sort: Option<SortKey>,
    pub reverse: bool,
    pub offset: usize,
    pub limit: usize,
    /// Die Seite reicht mindestens bis zu diesem Eintrag, damit die Auswahl
    /// nach einem Neuladen erhalten bleibt.
    pub keep: Option<String>,
}

#[derive(Serialize)]
pub struct SearchPage {
    /// Treffer insgesamt, nicht nur auf dieser Seite.
    pub total: usize,
    pub entries: Vec<EntryDto>,
}

/// Entschlüsselter In-Memory-Index. Die Suche braucht nur `&self`: der
/// Matcher entsteht pro Suche, damit Suchen nur eine Lesesperre halten und
/// die Erfassung nicht blockieren.
pub struct SearchIndex {
    entries: Vec<IndexedEntry>,
}

impl SearchIndex {
    pub fn build(rows: &[EntryRow], keys: &CryptoKeys) -> Self {
        let mut index = Self {
            entries: Vec::with_capacity(rows.len()),
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

    /// Die `n` zuletzt kopierten Einträge, ohne Vorrang für Bausteine oder
    /// Angepinntes (Mini-Palette).
    pub fn recent(&self, n: usize) -> Vec<EntryDto> {
        self.entries.iter().take(n).map(|e| e.dto.clone()).collect()
    }

    pub fn set_snippet(&mut self, uuid: &str, snippet: bool) {
        if let Some(e) = self.entries.iter_mut().find(|e| e.dto.uuid == uuid) {
            e.dto.snippet = snippet;
        }
    }

    /// Leere Query → alles; sonst die besten `MAX_HITS` Fuzzy-Treffer.
    /// Textbausteine stehen vor Angepinntem, Angepinntes vor dem Rest; darin
    /// entscheidet `sort`, bei Gleichstand (oder ohne `sort`) Score, dann Zeit.
    pub fn search(&self, p: &SearchParams) -> SearchPage {
        let query = p.query.trim();
        let candidates: Vec<&IndexedEntry> = self
            .entries
            .iter()
            .filter(|e| p.kind.is_none_or(|k| e.dto.kind == k))
            .filter(|e| p.refine.is_none_or(|r| refine_matches(r, &e.dto)))
            .collect();
        let mut scored = if query.is_empty() {
            candidates.iter().map(|e| (0u32, &e.dto)).collect()
        } else {
            score_all(query, &candidates)
        };
        scored.sort_by(|(sa, a), (sb, b)| {
            b.snippet
                .cmp(&a.snippet)
                .then(b.pinned.cmp(&a.pinned))
                .then(sb.cmp(sa))
                .then(b.created_at.cmp(&a.created_at))
        });
        if !query.is_empty() {
            scored.truncate(MAX_HITS);
        }
        if let Some(key) = p.sort {
            // Stabil: gleiche Werte behalten die Score-Reihenfolge.
            scored.sort_by(|(_, a), (_, b)| {
                let by_key = sort_value(key, b).cmp(&sort_value(key, a));
                b.snippet
                    .cmp(&a.snippet)
                    .then(b.pinned.cmp(&a.pinned))
                    .then(if p.reverse { by_key.reverse() } else { by_key })
            });
        }
        let total = scored.len();
        let mut end = p.offset.saturating_add(p.limit).min(total);
        if let Some(keep) = p.keep.as_deref() {
            if let Some(pos) = scored.iter().position(|(_, d)| d.uuid == keep) {
                end = end.max(pos + 1);
            }
        }
        let start = p.offset.min(end);
        SearchPage {
            total,
            entries: scored[start..end]
                .iter()
                .map(|(_, d)| (*d).clone())
                .collect(),
        }
    }
}

/// Fuzzy-Score je Kandidat; ab `PARALLEL_MIN` Kandidaten auf mehrere Threads
/// verteilt, jeder mit eigenem Matcher.
fn score_all<'a>(query: &str, candidates: &[&'a IndexedEntry]) -> Vec<(u32, &'a EntryDto)> {
    let pattern = Pattern::parse(query, CaseMatching::Ignore, Normalization::Smart);
    let score_chunk = |chunk: &[&'a IndexedEntry]| {
        let mut matcher = Matcher::new(Config::DEFAULT);
        let mut buf = Vec::new();
        chunk
            .iter()
            .filter_map(|e| {
                pattern
                    .score(Utf32Str::new(&e.haystack, &mut buf), &mut matcher)
                    .map(|s| (s, &e.dto))
            })
            .collect::<Vec<_>>()
    };
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(MAX_THREADS);
    if candidates.len() < PARALLEL_MIN || threads < 2 {
        return score_chunk(candidates);
    }
    let chunk_len = candidates.len().div_ceil(threads);
    std::thread::scope(|s| {
        let handles: Vec<_> = candidates
            .chunks(chunk_len)
            .map(|chunk| s.spawn(move || score_chunk(chunk)))
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect()
    })
}

fn sort_value(key: SortKey, d: &EntryDto) -> i64 {
    match key {
        SortKey::LastCopy => d.created_at,
        SortKey::FirstCopy => d.first_created_at,
        SortKey::CopyCount => d.copy_count,
        SortKey::Size => d.size_bytes,
    }
}

fn refine_matches(refine: Refine, d: &EntryDto) -> bool {
    match refine {
        Refine::Pinned => d.pinned,
        Refine::Snippets => d.snippet,
        Refine::Links => is_link(d),
        Refine::Totp => is_totp(d),
    }
}

/// Spiegel von `isLink` (`src/lib/entry-kinds.ts`): die Vorschau ist genau
/// eine http(s)-Adresse.
fn is_link(d: &EntryDto) -> bool {
    if d.kind != KIND_TEXT {
        return false;
    }
    let t = d.preview.trim();
    let lower = t.get(..8).unwrap_or(t).to_ascii_lowercase();
    let rest = if lower.starts_with("https://") {
        &t[8..]
    } else if lower.starts_with("http://") {
        &t[7..]
    } else {
        return false;
    };
    !rest.is_empty() && !rest.chars().any(char::is_whitespace)
}

/// Spiegel von `isTotp` (`src/lib/entry-kinds.ts`): otpauth-URI oder reines
/// Base32-Secret mit 16 bis 64 Zeichen, Padding am Ende erlaubt.
fn is_totp(d: &EntryDto) -> bool {
    if d.kind != KIND_TEXT {
        return false;
    }
    let t = d.preview.trim();
    if t.get(..10)
        .is_some_and(|p| p.eq_ignore_ascii_case("otpauth://"))
    {
        return true;
    }
    let t = t.trim_end_matches('=');
    (16..=64).contains(&t.len())
        && t.bytes()
            .all(|b| b.is_ascii_alphabetic() || (b'2'..=b'7').contains(&b))
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

    pub(super) fn row(
        keys: &CryptoKeys,
        uuid: &str,
        kind: u8,
        text: &str,
        created_at: i64,
    ) -> EntryRow {
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

    fn find(index: &SearchIndex, query: &str, kind: Option<u8>, limit: usize) -> Vec<String> {
        let page = index.search(&SearchParams {
            query: query.into(),
            kind,
            limit,
            ..Default::default()
        });
        page.entries.into_iter().map(|e| e.uuid).collect()
    }

    #[test]
    fn recent_ignores_snippets_and_pins() {
        let keys = Secret::generate().unwrap().derive_keys();
        let mut snippet = row(&keys, "baustein", KIND_TEXT, "vorlage", 1);
        snippet.snippet = true;
        let mut pinned = row(&keys, "pin", KIND_TEXT, "gepinnt", 2);
        pinned.pinned = true;
        let rows = vec![
            snippet,
            pinned,
            row(&keys, "a", KIND_TEXT, "a", 3),
            row(&keys, "b", KIND_TEXT, "b", 4),
        ];
        let mut index = SearchIndex::build(&rows, &keys);
        let uuids =
            |i: &SearchIndex, n| i.recent(n).into_iter().map(|e| e.uuid).collect::<Vec<_>>();
        assert_eq!(uuids(&index, 3), ["b", "a", "pin"]);
        index.touch("baustein", 9, &TouchSource::Keep);
        assert_eq!(uuids(&index, 2), ["baustein", "b"]);
        assert_eq!(uuids(&index, 9).len(), 4);
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

        let all = find(&index, "", None, usize::MAX);
        assert_eq!(
            all,
            ["baustein", "pin", "unscharf", "anderes", "neu", "datei", "alt"]
        );

        let order = find(&index, "rechnung", None, usize::MAX);
        assert_eq!(&order[..2], ["baustein", "pin"]);
        assert!(!order.contains(&"anderes".to_string()));
        // Gleicher Score: das Neuere zuerst; der exakte Treffer vor dem unscharfen.
        let pos = |u: &str| order.iter().position(|x| x == u).unwrap();
        assert!(pos("neu") < pos("alt"));
        assert!(pos("neu") < pos("unscharf"));

        // Der Typ-Filter wirkt auf die leere und die echte Suche.
        assert_eq!(find(&index, "", Some(KIND_FILES), usize::MAX), ["datei"]);
        assert_eq!(find(&index, "rechnung", Some(KIND_FILES), 10), ["datei"]);
        assert_eq!(find(&index, "rechnung", None, 2).len(), 2);

        // Gelöschtes fällt beim upsert heraus.
        let mut trashed = row(&keys, "neu", KIND_TEXT, "rechnung", 5);
        trashed.trashed_at = 10;
        index.upsert(&trashed, &keys);
        assert!(!find(&index, "", None, usize::MAX).contains(&"neu".to_string()));
    }

    fn sorted(index: &SearchIndex, query: &str, key: SortKey, reverse: bool) -> Vec<String> {
        let page = index.search(&SearchParams {
            query: query.into(),
            sort: Some(key),
            reverse,
            limit: usize::MAX,
            ..Default::default()
        });
        page.entries.into_iter().map(|e| e.uuid).collect()
    }

    /// Sortierung der Liste: Bausteine und Angepinntes bleiben vorn, auch umgekehrt.
    #[test]
    fn search_sorts_by_key_and_reverse() {
        let keys = Secret::generate().unwrap().derive_keys();
        let mut alt = row(&keys, "alt", KIND_TEXT, "a".repeat(50).as_str(), 1);
        alt.copy_count = 3;
        let neu = row(&keys, "neu", KIND_TEXT, "b".repeat(10).as_str(), 3);
        let mut pin = row(&keys, "pin", KIND_TEXT, "c", 0);
        pin.pinned = true;
        let mut snip = row(&keys, "snip", KIND_TEXT, "d", 0);
        snip.snippet = true;
        let mut mitte = row(&keys, "mitte", KIND_TEXT, "e".repeat(30).as_str(), 2);
        mitte.first_created_at = 5;
        mitte.copy_count = 2;
        let index = SearchIndex::build(&[alt, neu, pin, snip, mitte], &keys);

        let key = |k, rev| sorted(&index, "", k, rev);
        assert_eq!(
            key(SortKey::LastCopy, false),
            ["snip", "pin", "neu", "mitte", "alt"]
        );
        assert_eq!(
            key(SortKey::LastCopy, true),
            ["snip", "pin", "alt", "mitte", "neu"]
        );
        assert_eq!(
            key(SortKey::FirstCopy, false),
            ["snip", "pin", "mitte", "neu", "alt"]
        );
        assert_eq!(
            key(SortKey::CopyCount, false),
            ["snip", "pin", "alt", "mitte", "neu"]
        );
        assert_eq!(
            key(SortKey::Size, false),
            ["snip", "pin", "alt", "mitte", "neu"]
        );
    }

    /// Mit Suchtext gewinnt die gewählte Sortierung; der Score entscheidet nur
    /// bei Gleichstand, gedeckelt wird vorher nach Score.
    #[test]
    fn search_sort_wins_over_score_and_caps_hits() {
        let keys = Secret::generate().unwrap().derive_keys();
        let mut rows = vec![
            row(&keys, "exakt-alt", KIND_TEXT, "rechnung", 1),
            row(&keys, "unscharf-neu", KIND_TEXT, "r-e-c-h-n-u-n-g", 9),
        ];
        rows.extend((0..MAX_HITS + 5).map(|i| {
            row(
                &keys,
                &format!("fuell-{i}"),
                KIND_TEXT,
                "rechnung",
                2 + i as i64,
            )
        }));
        let index = SearchIndex::build(&rows, &keys);
        let by_time = sorted(&index, "rechnung", SortKey::LastCopy, false);
        assert_eq!(by_time.len(), MAX_HITS);
        // Der unscharfe Treffer fällt beim Deckeln nach Score heraus.
        assert!(!by_time.contains(&"unscharf-neu".to_string()));
        assert_eq!(by_time[0], format!("fuell-{}", MAX_HITS + 4));
    }

    #[test]
    fn search_pages_and_keeps_selection() {
        let keys = Secret::generate().unwrap().derive_keys();
        let rows: Vec<_> = (0..10)
            .map(|i| row(&keys, &format!("e{i}"), KIND_TEXT, "x", i))
            .collect();
        let index = SearchIndex::build(&rows, &keys);
        let page = |offset, limit, keep: Option<&str>| {
            let p = index.search(&SearchParams {
                offset,
                limit,
                keep: keep.map(Into::into),
                sort: Some(SortKey::LastCopy),
                ..Default::default()
            });
            let ids: Vec<String> = p.entries.into_iter().map(|e| e.uuid).collect();
            (p.total, ids)
        };
        assert_eq!(
            page(0, 3, None),
            (10, vec!["e9".into(), "e8".into(), "e7".into()])
        );
        assert_eq!(page(3, 2, None), (10, vec!["e6".into(), "e5".into()]));
        assert_eq!(page(9, 5, None).1, ["e0"]);
        assert!(page(20, 5, None).1.is_empty());
        // Die Seite wächst bis zum gemerkten Eintrag, nie darüber hinaus.
        assert_eq!(page(0, 2, Some("e5")).1.len(), 5);
        assert_eq!(page(0, 2, Some("e9")).1.len(), 2);
        assert_eq!(page(0, 2, Some("fehlt")).1.len(), 2);
    }

    /// Dieselben Fälle wie `isLink`/`isTotp` in `src/lib/entry-kinds.test.ts`.
    #[test]
    fn refine_mirrors_frontend() {
        let dto = |kind, preview: &str| EntryDto {
            uuid: "u".into(),
            kind,
            preview: preview.into(),
            created_at: 0,
            pinned: false,
            size_bytes: 0,
            has_thumb: false,
            source_app_id: None,
            source_app_name: None,
            first_created_at: 0,
            copy_count: 1,
            snippet: false,
            has_html: false,
        };
        assert!(is_link(&dto(KIND_TEXT, " https://example.org/a?b=1 ")));
        assert!(is_link(&dto(KIND_TEXT, "http://x.de")));
        assert!(is_link(&dto(KIND_TEXT, "HTTPS://X.DE")));
        assert!(!is_link(&dto(KIND_TEXT, "siehe https://x.de")));
        assert!(!is_link(&dto(KIND_TEXT, "ftp://x.de")));
        assert!(!is_link(&dto(KIND_TEXT, "https://")));
        assert!(!is_link(&dto(KIND_IMAGE, "https://x.de")));

        assert!(is_totp(&dto(KIND_TEXT, "otpauth://totp/x?secret=ABC")));
        assert!(is_totp(&dto(KIND_TEXT, "JBSWY3DPEHPK3PXP")));
        assert!(is_totp(&dto(KIND_TEXT, "jbswy3dpehpk3pxp")));
        assert!(is_totp(&dto(KIND_TEXT, "JBSWY3DPEHPK3PXPJBSW====")));
        assert!(!is_totp(&dto(KIND_TEXT, "JBSWY3DP========")));
        assert!(!is_totp(&dto(KIND_TEXT, "JBSWY3DP")));
        assert!(!is_totp(&dto(KIND_TEXT, "JBSWY3DP EHPK3PXP")));
        assert!(!is_totp(&dto(KIND_TEXT, "JBSWY3DPEHPK3PX1")));
        assert!(!is_totp(&dto(KIND_FILES, "JBSWY3DPEHPK3PXP")));
    }

    #[test]
    fn refine_filters_before_paging() {
        let keys = Secret::generate().unwrap().derive_keys();
        let mut pin = row(&keys, "pin", KIND_TEXT, "a", 1);
        pin.pinned = true;
        let rows = vec![
            pin,
            row(&keys, "link", KIND_TEXT, "https://x.de", 2),
            row(&keys, "totp", KIND_TEXT, "JBSWY3DPEHPK3PXP", 3),
            row(&keys, "text", KIND_TEXT, "hallo", 4),
        ];
        let index = SearchIndex::build(&rows, &keys);
        let only = |refine| {
            let p = index.search(&SearchParams {
                refine: Some(refine),
                limit: 1,
                ..Default::default()
            });
            (p.total, p.entries[0].uuid.clone())
        };
        assert_eq!(only(Refine::Pinned), (1, "pin".into()));
        assert_eq!(only(Refine::Links), (1, "link".into()));
        assert_eq!(only(Refine::Totp), (1, "totp".into()));
    }
}

#[cfg(test)]
mod perf {
    use super::tests::row;
    use super::*;
    use crate::storage::crypto::Secret;
    use std::time::{Duration, Instant};

    const WORDS: &[&str] = &[
        "rechnung",
        "kunde",
        "server",
        "passwort",
        "angebot",
        "termin",
        "fn",
        "let",
        "return",
        "https://example.com/pfad",
        "select",
        "from",
        "where",
        "drucker",
        "netzwerk",
        "lizenz",
        "backup",
        "benutzer",
        "fehler",
        "import",
        "export",
        "const",
        "async",
        "await",
    ];

    fn text(seed: u64, words: usize) -> String {
        let mut x = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let mut out = String::new();
        for i in 0..words {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            if i > 0 {
                out.push(if x.is_multiple_of(11) { '\n' } else { ' ' });
            }
            out.push_str(WORDS[(x % WORDS.len() as u64) as usize]);
        }
        out
    }

    /// Synthetische Historie mit 5000 Einträgen über ein Jahr: kurze, mittlere
    /// und lange Texte, jeder 50. ein Bild. Keine echten Nutzerdaten.
    fn history(keys: &CryptoKeys) -> Vec<EntryRow> {
        (0..5000u64)
            .map(|i| {
                let uuid = format!("{i:08}-0000-7000-8000-000000000000");
                let created = 1_758_000_000_000 + i as i64 * 6_300_000;
                if i.is_multiple_of(50) {
                    let mut r = row(keys, &uuid, KIND_TEXT, "", created);
                    r.kind = KIND_IMAGE;
                    r.cipher = None;
                    r.size_bytes = 200_000;
                    r.thumb = Some(vec![0; 8000]);
                    return r;
                }
                let words = match i % 20 {
                    0 => 3000,
                    1..=4 => 200,
                    _ => 3 + (i % 25) as usize,
                };
                let mut r = row(keys, &uuid, KIND_TEXT, &text(i, words), created);
                r.pinned = i.is_multiple_of(500);
                r.snippet = i % 1000 == 1;
                r
            })
            .collect()
    }

    fn median(mut runs: Vec<Duration>) -> Duration {
        runs.sort();
        runs[runs.len() / 2]
    }

    #[test]
    #[ignore = "Messung: cargo test --release --lib perf_ -- --ignored --nocapture"]
    fn perf_5000_entries() {
        let keys = Secret::generate().unwrap().derive_keys();
        let rows = history(&keys);
        let t = Instant::now();
        let index = SearchIndex::build(&rows, &keys);
        println!("build: {:?}", t.elapsed());
        let t = Instant::now();
        for _ in 0..100 {
            std::hint::black_box(Matcher::new(Config::DEFAULT));
        }
        println!("Matcher::new: {:?}", t.elapsed() / 100);
        for (q, limit) in [
            ("", usize::MAX),
            ("", 200),
            ("r", 200),
            ("rech", 200),
            ("rechnung kunde", 200),
            ("zzzq", 200),
        ] {
            let params = SearchParams {
                query: q.into(),
                sort: Some(SortKey::LastCopy),
                limit,
                ..Default::default()
            };
            let runs: Vec<Duration> = (0..21)
                .map(|_| {
                    let t = Instant::now();
                    std::hint::black_box(index.search(&params));
                    t.elapsed()
                })
                .collect();
            let page = index.search(&params);
            let t = Instant::now();
            let json = serde_json::to_string(&page).unwrap();
            let ser = t.elapsed();
            if let Ok(dir) = std::env::var("TIPPIT_PERF_DIR") {
                let name = if limit == usize::MAX { "all" } else { "page" };
                if q.is_empty() {
                    std::fs::write(format!("{dir}/perf-{name}.json"), &json).unwrap();
                }
            }
            println!(
                "query {q:?} limit {limit}: search {:?}, total {}, returned {}, json {} bytes in {ser:?}",
                median(runs),
                page.total,
                page.entries.len(),
                json.len(),
            );
        }
    }
}
