use std::collections::HashMap;

use memchr::memmem::Finder;
use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use serde::{Deserialize, Serialize};

use super::crypto::{self, CryptoKeys};
use super::db::{EntryRow, TouchSource, KIND_FILES, KIND_IMAGE, KIND_TEXT};
use super::vectors::{pick_similar, VectorStore};

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
    /// Nur nach Bedeutung gefunden, kein Suchwort steht im Text.
    pub similar: bool,
}

struct IndexedEntry {
    dto: EntryDto,
    /// Rohtext für den Fuzzy-Fallback.
    haystack: String,
    /// `haystack` gefaltet (`fold_words`).
    words: String,
    /// Name der Quell-App, gefaltet: zählt beim Suchen nur schwach.
    meta: String,
}

/// Treffer einer echten Suche bleiben gedeckelt: die schwächsten Treffer
/// helfen niemandem, und eine gewählte Sortierung soll nur unter den besten laufen.
pub const MAX_HITS: usize = 200;
/// Liefert die Wortsuche weniger Treffer, ergänzt das Fuzzy-Matching von nucleo.
const FUZZY_BELOW: usize = 10;
/// Ab so vielen Kandidaten lohnt es, das Scoring auf Threads zu verteilen.
const PARALLEL_MIN: usize = 1024;
const MAX_THREADS: usize = 8;

/// Gewählte Sortierung der Liste; Bausteine und Angepinntes stehen dabei vorn.
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
    /// `None`: mit Suchtext nach Relevanz, ohne nach Zeit (Bausteine und
    /// Angepinntes vorn).
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
                    e.meta = fold_words(name);
                }
                TouchSource::Clear => {
                    e.dto.source_app_id = None;
                    e.dto.source_app_name = None;
                    e.meta.clear();
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

    /// Texteinträge (uuid, Text) für die Suche nach Bedeutung, neueste zuerst.
    pub fn texts(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries
            .iter()
            .filter(|e| e.dto.kind == KIND_TEXT && !e.haystack.trim().is_empty())
            .map(|e| (e.dto.uuid.as_str(), e.haystack.as_str()))
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

    #[cfg(test)]
    pub fn search(&self, p: &SearchParams) -> SearchPage {
        self.search_with(p, None)
    }

    /// Leere Query → alles, Bausteine vor Angepinntem vor dem Rest, darin nach
    /// Zeit. Sonst die besten `MAX_HITS` Treffer nach Relevanz (`rank_all`),
    /// darunter Treffer nach Bedeutung, wenn `meaning` Vektoren und den Vektor
    /// der Anfrage liefert. Mit `sort` wird danach umsortiert, Bausteine und
    /// Angepinntes vorn.
    pub fn search_with(
        &self,
        p: &SearchParams,
        meaning: Option<(&VectorStore, &[f32])>,
    ) -> SearchPage {
        let query = p.query.trim();
        let candidates: Vec<&IndexedEntry> = self
            .entries
            .iter()
            .filter(|e| p.kind.is_none_or(|k| e.dto.kind == k))
            .filter(|e| p.refine.is_none_or(|r| refine_matches(r, &e.dto)))
            .collect();
        let mut hits = if query.is_empty() {
            let mut all: Vec<_> = candidates
                .iter()
                .map(|e| (Rank::default(), &e.dto))
                .collect();
            all.sort_by(|(_, a), (_, b)| {
                b.snippet
                    .cmp(&a.snippet)
                    .then(b.pinned.cmp(&a.pinned))
                    .then(b.created_at.cmp(&a.created_at))
            });
            all
        } else {
            // Verteilung der Ähnlichkeiten über alle aktiven Einträge, nicht
            // nur die gefilterten.
            let meaning = meaning.map(|(store, v)| {
                let all: Vec<f32> = self
                    .entries
                    .iter()
                    .filter_map(|e| store.cosine(&e.dto.uuid, v))
                    .collect();
                (store, v, all)
            });
            let mut ranked = rank_all(&Query::parse(query), &candidates, meaning.as_ref());
            ranked.sort_by(|(ra, a), (rb, b)| rb.cmp(ra).then(b.created_at.cmp(&a.created_at)));
            ranked.truncate(MAX_HITS);
            ranked
        };
        if let Some(key) = p.sort {
            // Stabil: gleiche Werte behalten die Relevanz-Reihenfolge.
            hits.sort_by(|(_, a), (_, b)| {
                let by_key = sort_value(key, b).cmp(&sort_value(key, a));
                b.snippet
                    .cmp(&a.snippet)
                    .then(b.pinned.cmp(&a.pinned))
                    .then(if p.reverse { by_key.reverse() } else { by_key })
            });
        }
        let total = hits.len();
        let mut end = p.offset.saturating_add(p.limit).min(total);
        if let Some(keep) = p.keep.as_deref() {
            if let Some(pos) = hits.iter().position(|(_, d)| d.uuid == keep) {
                end = end.max(pos + 1);
            }
        }
        let start = p.offset.min(end);
        SearchPage {
            total,
            entries: hits[start..end]
                .iter()
                .map(|(r, d)| EntryDto {
                    similar: r.tier == TIER_SIMILAR,
                    ..(*d).clone()
                })
                .collect(),
        }
    }
}

/// Relevanz eines Treffers: erst die Stufe, darin der Score.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Rank {
    tier: u8,
    score: u32,
}

/// Die ganze Eingabe steht als Wortfolge im Text.
const TIER_PHRASE: u8 = 5;
/// Alle Wörter kommen vor (am Wortanfang, im Wort, mit Tippfehler oder im App-Namen).
const TIER_ALL: u8 = 4;
/// Ab drei Wörtern darf höchstens ein Drittel fehlen.
const TIER_MOST: u8 = 3;
/// Nur die Suche nach Bedeutung findet ihn (Opt-in, `storage::vectors`).
const TIER_SIMILAR: u8 = 2;
/// Nur nucleo findet etwas.
const TIER_FUZZY: u8 = 1;

/// Zerlegte Suche, gefaltet wie der Text (`fold_words`): jedes Wort trägt ein
/// führendes Leerzeichen, ein Fund beginnt so immer am Wortanfang.
struct Query {
    /// Gewertete Wörter, ohne Doppelte, auch die aus `"…"`.
    terms: Vec<String>,
    /// `terms` ohne Leerzeichen als Zeichen, für Längen und Tippfehler.
    chars: Vec<Vec<char>>,
    /// Vorbereitete Suche je Wort: mit und ohne führendes Leerzeichen
    /// (SIMD; `str::find` ist bei langen Logs der teuerste Teil).
    finders: Vec<(Finder<'static>, Finder<'static>)>,
    /// Ganze Anfrage als Wortfolge.
    whole_finder: Finder<'static>,
    /// Alle gewerteten Wörter in Eingabereihenfolge.
    whole: String,
    /// `"…"`: muss so als Wortfolge vorkommen; geschlossen (`true`) bis zum
    /// Wortende, ein offenes beim Tippen auch als Wortanfang.
    phrases: Vec<(String, bool)>,
    /// `-wort`: darf nicht vorkommen.
    excludes: Vec<String>,
    /// Eingabe ohne Ausschlüsse und Anführungszeichen, für nucleo.
    fuzzy: String,
}

impl Query {
    fn parse(raw: &str) -> Self {
        let mut phrases = Vec::new();
        let mut excludes = Vec::new();
        let mut kept: Vec<&str> = Vec::new();
        // Ungerade Abschnitte stehen in Anführungszeichen; ein offenes gilt bis zum Ende.
        let parts = raw.split('"').count();
        for (i, part) in raw.split('"').enumerate() {
            if i % 2 == 1 {
                let phrase = fold_words(part);
                if !phrase.is_empty() {
                    phrases.push((phrase, i + 1 < parts));
                    kept.push(part);
                }
                continue;
            }
            for word in part.split_whitespace() {
                match word.strip_prefix('-') {
                    Some(rest) => {
                        let rest = fold_words(rest);
                        if !rest.is_empty() {
                            excludes.push(rest);
                        }
                    }
                    None => kept.push(word),
                }
            }
        }
        let fuzzy = kept.join(" ");
        let whole = fold_words(&fuzzy);
        let mut terms: Vec<String> = Vec::new();
        for t in whole.split(' ').skip(1) {
            let t = format!(" {t}");
            if !terms.contains(&t) {
                terms.push(t);
            }
        }
        let chars = terms.iter().map(|t| t[1..].chars().collect()).collect();
        let finders = terms
            .iter()
            .map(|t| {
                (
                    Finder::new(t.as_str()).into_owned(),
                    Finder::new(&t[1..]).into_owned(),
                )
            })
            .collect();
        let whole_finder = Finder::new(whole.as_str()).into_owned();
        Self {
            terms,
            chars,
            finders,
            whole_finder,
            whole,
            phrases,
            excludes,
            fuzzy,
        }
    }

    /// Harte Bedingungen aus `"…"` und `-wort`, auch für den Fuzzy-Fallback.
    fn admits(&self, e: &IndexedEntry) -> bool {
        self.phrases.iter().all(|(p, closed)| {
            if *closed {
                contains_word_run(&e.words, p)
            } else {
                e.words.contains(p.as_str())
            }
        }) && !self.excludes.iter().any(|x| e.words.contains(x.as_str()))
    }
}

/// `p` (gefaltet, mit führendem Leerzeichen) steht in `words` und endet an
/// einer Wortgrenze.
fn contains_word_run(words: &str, p: &str) -> bool {
    words.match_indices(p).any(|(i, _)| {
        let end = i + p.len();
        end == words.len() || words.as_bytes()[end] == b' '
    })
}

/// Wortsuche über alle Kandidaten, darunter die Treffer nach Bedeutung; findet
/// die Wortsuche weniger als `FUZZY_BELOW`, ergänzt nucleo die übrigen ganz unten.
fn rank_all<'a>(
    q: &Query,
    candidates: &[&'a IndexedEntry],
    meaning: Option<&(&VectorStore, &[f32], Vec<f32>)>,
) -> Vec<(Rank, &'a EntryDto)> {
    let newest = candidates
        .iter()
        .map(|e| e.dto.created_at)
        .max()
        .unwrap_or(0);
    let lexical: Vec<Option<Rank>> = parallel(candidates, |chunk| {
        let mut typos = vec![HashMap::new(); q.terms.len()];
        chunk
            .iter()
            .map(|e| rank_words(q, e, newest, &mut typos))
            .collect()
    });
    let mut hits: Vec<(Rank, &EntryDto)> = candidates
        .iter()
        .zip(&lexical)
        .filter_map(|(e, r)| r.map(|r| (r, &e.dto)))
        .collect();
    let lexical_hits = hits.len();
    let others: Vec<&IndexedEntry> = candidates
        .iter()
        .zip(&lexical)
        .filter(|(e, r)| r.is_none() && q.admits(e))
        .map(|(e, _)| *e)
        .collect();
    // Nur was hier noch infrage kommt, konkurriert um die Plätze nach Bedeutung.
    let similar = meaning.map_or_else(Vec::new, |(store, v, all)| {
        let eligible = others
            .iter()
            .filter_map(|e| Some((e.dto.uuid.as_str(), store.cosine(&e.dto.uuid, v)?)))
            .collect();
        pick_similar(all, eligible)
    });
    let mut rest: Vec<&IndexedEntry> = Vec::new();
    for e in others {
        match similar.iter().find(|(u, _)| *u == e.dto.uuid) {
            Some((_, sim)) => {
                let score = (sim.clamp(0.0, 1.0) * 1000.0) as u32;
                hits.push((
                    Rank {
                        tier: TIER_SIMILAR,
                        score,
                    },
                    &e.dto,
                ));
            }
            None => rest.push(e),
        }
    }
    if lexical_hits >= FUZZY_BELOW || q.fuzzy.is_empty() {
        return hits;
    }
    let pattern = Pattern::new(
        &q.fuzzy,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    hits.extend(parallel(&rest, |chunk| {
        let mut matcher = Matcher::new(Config::DEFAULT);
        let mut buf = Vec::new();
        chunk
            .iter()
            .filter_map(|e| {
                let score = pattern.score(Utf32Str::new(&e.haystack, &mut buf), &mut matcher)?;
                Some((
                    Rank {
                        tier: TIER_FUZZY,
                        score,
                    },
                    &e.dto,
                ))
            })
            .collect()
    }));
    hits
}

/// Stufe und Score der Wortsuche; `None`, wenn zu viele Wörter fehlen.
fn rank_words<'a>(
    q: &Query,
    e: &'a IndexedEntry,
    newest: i64,
    typos: &mut [Typos<'a>],
) -> Option<Rank> {
    if !q.admits(e) {
        return None;
    }
    let mut score = 0;
    let mut matched = 0;
    // Erste Fundstelle je Wort im Text (Byte-Offset, Ende).
    let mut spans: Vec<(usize, usize)> = Vec::new();
    let mut in_order = true;
    for ((t, chars), (finders, typos)) in q
        .terms
        .iter()
        .zip(&q.chars)
        .zip(q.finders.iter().zip(typos))
    {
        let (s, pos) = term_score(t, chars, finders, e, typos);
        if s == 0 {
            continue;
        }
        matched += 1;
        score += s;
        if let Some(span) = pos {
            in_order &= spans.last().is_none_or(|last| last.0 < span.0);
            spans.push(span);
        }
    }
    let n = q.terms.len();
    let tier = if n >= 2 && q.whole_finder.find(e.words.as_bytes()).is_some() {
        TIER_PHRASE
    } else if matched == n {
        TIER_ALL
    } else if n >= 3 && n - matched <= n / 3 {
        TIER_MOST
    } else {
        return None;
    };
    let whole = q.whole.len() as u64;
    if !spans.is_empty() {
        // Kurze Texte, die vor allem das Gesuchte enthalten, vor langen Logs.
        score += (60 * whole / (whole + e.words.len() as u64 / 8 + 1)) as u32;
        if e.words.starts_with(q.terms[0].as_str()) {
            score += 30;
        }
    }
    if spans.len() >= 2 {
        if in_order {
            score += 30;
        }
        // Dicht beieinander statt über den Text verstreut.
        let from = spans.iter().map(|s| s.0).min().unwrap_or(0);
        let to = spans.iter().map(|s| s.1).max().unwrap_or(0);
        score += (50 * whole / (to - from).max(whole as usize) as u64) as u32;
    }
    Some(Rank {
        tier,
        score: score + bonus(&e.dto, newest),
    })
}

/// Wert eines Suchworts und seine erste Fundstelle im Text: Wortanfang (als
/// ganzes Wort etwas mehr) vor Wortinnerem vor Tippfehler vor App-Name.
fn term_score<'a>(
    t: &str,
    chars: &[char],
    (word_start, inner): &(Finder<'static>, Finder<'static>),
    e: &'a IndexedEntry,
    typos: &mut Typos<'a>,
) -> (u32, Option<(usize, usize)>) {
    if let Some(pos) = word_start.find(e.words.as_bytes()) {
        let end = pos + t.len();
        let whole_word = e.words[end..].starts_with(' ') || end == e.words.len();
        return (if whole_word { 110 } else { 100 }, Some((pos, end)));
    }
    let bare = &t[1..];
    if chars.len() >= 3 {
        if let Some(pos) = inner.find(e.words.as_bytes()) {
            return (60, Some((pos, pos + bare.len())));
        }
    }
    if chars.len() >= 5 && typo_match(chars, &e.words, typos) {
        return (50, None);
    }
    if e.meta.contains(t) {
        return (40, None);
    }
    (0, None)
}

const TYPO_MAX_LEN: usize = 48;

/// Ein Wort im Text ist höchstens einen Tippfehler entfernt (ab 9 Zeichen
/// zwei; Vertauschen zählt als einer). Der erste Buchstabe muss stimmen: so
/// bleibt der Vergleich auch bei langen Logs billig. Gezählt wird in
/// Zeichen; `typos` merkt sich das Ergebnis je Wort über alle Einträge einer
/// Suche, denn Texte wiederholen ihre Wörter.
fn typo_match<'a>(t: &[char], words: &'a str, typos: &mut Typos<'a>) -> bool {
    let max = if t.len() >= 9 { 2 } else { 1 };
    let mut w: [char; TYPO_MAX_LEN] = ['\0'; TYPO_MAX_LEN];
    words.split(' ').any(|word| {
        // Billige Vorprüfung: erster Buchstabe und Länge in Bytes (ein
        // Zeichen hat 1 bis 4 Bytes).
        if word.chars().next() != t.first().copied()
            || word.len() + max < t.len()
            || word.len() > (t.len() + max) * 4
        {
            return false;
        }
        *typos.entry(word).or_insert_with(|| {
            let mut n = 0;
            for c in word.chars() {
                if n == TYPO_MAX_LEN {
                    return false;
                }
                w[n] = c;
                n += 1;
            }
            n.abs_diff(t.len()) <= max && edits_within(t, &w[..n], max)
        })
    })
}

/// Tippfehler-Ergebnis je Wort des Textes, für ein Suchwort.
type Typos<'a> = HashMap<&'a str, bool>;

/// Optimal String Alignment (Levenshtein plus Vertauschung) mit Abbruch,
/// sobald eine Zeile `max` überschreitet. Längere Wörter als `TYPO_MAX_LEN`
/// gelten nicht als Tippfehler; so bleiben die Zeilen auf dem Stack.
fn edits_within(a: &[char], b: &[char], max: usize) -> bool {
    if a.len() >= TYPO_MAX_LEN || b.len() >= TYPO_MAX_LEN {
        return false;
    }
    let mut before = [0usize; TYPO_MAX_LEN];
    let mut prev = [0usize; TYPO_MAX_LEN];
    let mut cur = [0usize; TYPO_MAX_LEN];
    for (j, p) in prev.iter_mut().enumerate().take(b.len() + 1) {
        *p = j;
    }
    for i in 1..=a.len() {
        cur[0] = i;
        let mut row_min = i;
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut d = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d = d.min(before[j - 2] + 1);
            }
            cur[j] = d;
            row_min = row_min.min(d);
        }
        if row_min > max {
            return false;
        }
        std::mem::swap(&mut before, &mut prev);
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()] <= max
}

/// Leichter Vorsprung innerhalb einer Stufe: frisch, oft kopiert, angepinnt
/// oder Baustein. Nie so groß, dass er eine Stufe überspringt.
fn bonus(d: &EntryDto, newest: i64) -> u32 {
    let days =
        u32::try_from(newest.saturating_sub(d.created_at).max(0) / 86_400_000).unwrap_or(u32::MAX);
    let fresh = 30 * 7 / days.saturating_add(7);
    let often = (d.copy_count.clamp(1, 6) - 1) as u32 * 4;
    fresh + often + if d.pinned { 15 } else { 0 } + if d.snippet { 20 } else { 0 }
}

/// Text für die Wortsuche: Kleinbuchstaben ohne Akzente, ä/ae → a, ö/oe → o,
/// ü/ue → u, ß → ss; jedes Wort mit führendem Leerzeichen, alles außer
/// Buchstaben und Ziffern trennt. Spiegel: `foldTerm` in `src/lib/preview.ts`.
fn fold_words(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 1);
    let mut in_word = false;
    // Nach a/o/u entfällt ein schlichtes e: „Maerz“ wird wie „März“ gesucht.
    let mut after_vowel = false;
    // Ein solches e wartet auf das nächste Zeichen: trägt es einen
    // kombinierenden Akzent, ist es ein é und bleibt (wie das fertige é).
    let mut held_e = false;
    for c in text.chars().flat_map(char::to_lowercase) {
        // Kombinierende Akzente (zerlegtes Unicode, etwa macOS-Dateinamen).
        if ('\u{300}'..='\u{36f}').contains(&c) {
            if held_e {
                out.push('e');
                held_e = false;
            }
            continue;
        }
        held_e = false;
        if !c.is_alphanumeric() {
            in_word = false;
            after_vowel = false;
            continue;
        }
        if !in_word {
            out.push(' ');
            in_word = true;
        }
        if c == 'e' && after_vowel {
            after_vowel = false;
            held_e = true;
            continue;
        }
        after_vowel = matches!(c, 'a' | 'o' | 'u');
        match c {
            'ä' | 'à' | 'á' | 'â' | 'ã' | 'å' => out.push('a'),
            'ö' | 'ò' | 'ó' | 'ô' | 'õ' | 'ø' => out.push('o'),
            'ü' | 'ù' | 'ú' | 'û' => out.push('u'),
            'è' | 'é' | 'ê' | 'ë' => out.push('e'),
            'ì' | 'í' | 'î' | 'ï' => out.push('i'),
            'ç' => out.push('c'),
            'ñ' => out.push('n'),
            'ý' | 'ÿ' => out.push('y'),
            'ß' => out.push_str("ss"),
            _ => out.push(c),
        }
    }
    out
}

/// Arbeit über die Kandidaten; ab `PARALLEL_MIN` auf mehrere Threads
/// verteilt, die Reihenfolge der Ergebnisse bleibt die der Kandidaten.
fn parallel<'a, T: Send>(
    candidates: &[&'a IndexedEntry],
    work: impl Fn(&[&'a IndexedEntry]) -> Vec<T> + Sync,
) -> Vec<T> {
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(MAX_THREADS);
    if candidates.len() < PARALLEL_MIN || threads < 2 {
        return work(candidates);
    }
    let chunk_len = candidates.len().div_ceil(threads);
    let work = &work;
    std::thread::scope(|s| {
        let handles: Vec<_> = candidates
            .chunks(chunk_len)
            .map(|chunk| s.spawn(move || work(chunk)))
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
        words: fold_words(&haystack),
        meta: row
            .source_app_name
            .as_deref()
            .map(fold_words)
            .unwrap_or_default(),
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
            similar: false,
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

    /// Ohne Suchtext Bausteine vor Angepinntem vor dem Rest, darin nach Datum.
    /// Beim Suchen gewinnt der bessere Treffer, angepinnt oder nicht.
    #[test]
    fn search_puts_snippets_and_pins_first_only_without_query() {
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
        assert!(!order.contains(&"anderes".to_string()));
        // Gleicher Text: das Neuere zuerst. Das gespreizte „r e c h n u n g“
        // findet nur noch nucleo, eine Stufe tiefer, der Pin hilft nicht.
        let pos = |u: &str| order.iter().position(|x| x == u).unwrap();
        assert!(pos("neu") < pos("alt"));
        assert!(pos("alt") < pos("pin"));
        assert!(pos("alt") < pos("unscharf"));

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

    /// Mit Suchtext und gewählter Sortierung gewinnt die Sortierung; gedeckelt
    /// wird vorher nach Relevanz.
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
        // Bei so vielen Worttreffern bleibt der nur unscharfe draußen.
        assert!(!by_time.contains(&"unscharf-neu".to_string()));
        assert_eq!(by_time[0], format!("fuell-{}", MAX_HITS + 4));
    }

    const DAY: i64 = 86_400_000;
    const SATZ: &str = "Könnten Sie mir bitte die Rechnung für März noch einmal schicken?";

    fn first(index: &SearchIndex, query: &str) -> String {
        find(index, query, None, 1).remove(0)
    }

    /// Erinnerte Sätze, auch unvollständig, umformuliert oder mit Tippfehlern,
    /// schlagen neuere Streutreffer in einem Log.
    #[test]
    fn search_ranks_remembered_sentences_first() {
        let keys = Secret::generate().unwrap().derive_keys();
        let filler = "2026-03-02 INFO worker heartbeat ok\n".repeat(40);
        let log = format!(
            "{filler}2026-03-02 ERROR rechnung-service: bitte retry\n{filler}queue maerz voll, schicken abgebrochen\n{filler}"
        );
        let rows = vec![
            row(&keys, "satz", KIND_TEXT, SATZ, 1),
            row(&keys, "log", KIND_TEXT, &log, 200 * DAY),
            row(&keys, "kurz", KIND_TEXT, "Rechnung", 100 * DAY),
            row(&keys, "anderes", KIND_TEXT, "Termin am Montag", 300 * DAY),
        ];
        let index = SearchIndex::build(&rows, &keys);

        // Zwei Wörter fehlen, „märz“ ist der Text, „vom“ und „nochmal“ nicht.
        assert_eq!(
            first(&index, "rechnung vom märz bitte nochmal schicken"),
            "satz"
        );
        // Tippfehler und „ae“ statt „ä“; im Log steht „maerz“ wörtlich.
        assert_eq!(first(&index, "rechnug maerz bitte schicken"), "satz");
        assert_eq!(first(&index, "Könnten Sie mir bitte die Rechnung"), "satz");
        assert_eq!(first(&index, "bitte die rechnung fur marz"), "satz");
        // Ein einzelnes Wort: der kurze, genaue Treffer vor Satz und Log.
        assert_eq!(find(&index, "rechnung", None, 9), ["kurz", "satz", "log"]);
        // Eine gewählte Sortierung gilt weiter.
        assert_eq!(
            sorted(&index, "rechnung", SortKey::LastCopy, false)[0],
            "log"
        );
    }

    #[test]
    fn search_operators_and_source_app() {
        let keys = Secret::generate().unwrap().derive_keys();
        let mut a = row(&keys, "a", KIND_TEXT, "Rechnung noch einmal schicken", 3);
        a.source_app_name = Some("Outlook".into());
        let mut b = row(&keys, "b", KIND_TEXT, "Rechnung, einmal noch", 2);
        b.source_app_name = Some("Teams".into());
        let mut c = row(&keys, "c", KIND_TEXT, "Termin", 1);
        c.source_app_name = Some("Outlook".into());
        let index = SearchIndex::build(&[a, b, c], &keys);
        let all = |q| find(&index, q, None, usize::MAX);
        let found = |q| {
            let mut ids = all(q);
            ids.sort();
            ids
        };

        assert_eq!(all("\"noch einmal\""), ["a"]);
        // Offenes Anführungszeichen beim Tippen gilt bis zum Ende.
        assert_eq!(all("\"einmal no"), ["b"]);
        // Geschlossen gilt die Wortgrenze: „noc“ ist nicht „noch“.
        assert!(all("\"einmal noc\"").is_empty());
        assert_eq!(all("rechnung -schicken"), ["b"]);
        // Die nucleo-Zeichen sind keine Operatoren mehr.
        assert_eq!(found("^rechnung"), ["a", "b"]);
        assert_eq!(all("!schicken"), ["a"]);
        // Vertauschte Buchstaben gelten als ein Tippfehler.
        assert_eq!(found("rehcnung"), ["a", "b"]);
        // Der App-Name zählt, aber schwächer als der Text.
        assert_eq!(found("outlook"), ["a", "c"]);
        assert_eq!(all("outlook termin")[0], "c");
        assert_eq!(all("teams rechnung")[0], "b");
    }

    /// Treffer nach Bedeutung stehen unter allen Worttreffern, über nucleo,
    /// sind gekennzeichnet und halten sich an Filter und `-wort`. Vektoren
    /// gelöschter Einträge belegen keinen Platz.
    #[test]
    fn search_places_similar_between_words_and_fuzzy() {
        let keys = Secret::generate().unwrap().derive_keys();
        let mut rows = vec![
            row(&keys, "wort", KIND_TEXT, "Bankverbindung siehe unten", 1),
            row(&keys, "iban", KIND_TEXT, "IBAN DE89 3704 0044", 2),
            row(&keys, "iban2", KIND_TEXT, "IBAN DE12 extra", 3),
            row(
                &keys,
                "unscharf",
                KIND_TEXT,
                "b-a-n-k-v-e-r-b-i-n-d-u-n-g",
                4,
            ),
            row(&keys, "datei", KIND_FILES, "/tmp/konto.pdf", 5),
        ];
        let unit = |a: f32| vec![a.cos(), a.sin()];
        let mut vectors: std::collections::HashMap<String, Vec<f32>> = [
            ("wort", unit(0.3)),
            ("iban", unit(0.05)),
            ("iban2", unit(0.02)),
            ("datei", unit(0.0)),
            ("geloescht", unit(0.0)),
        ]
        .into_iter()
        .map(|(u, v)| (u.to_string(), v))
        .collect();
        for i in 0..60 {
            let uuid = format!("fueller{i}");
            rows.push(row(&keys, &uuid, KIND_TEXT, "Termin am Montag", 10 + i));
            vectors.insert(uuid, unit(1.2 + (i % 5) as f32 * 0.05));
        }
        let index = SearchIndex::build(&rows, &keys);
        let store = VectorStore::new("m", vectors);
        let q = unit(0.0);
        let page = |query: &str, kind| {
            index.search_with(
                &SearchParams {
                    query: query.into(),
                    kind,
                    limit: 9,
                    ..Default::default()
                },
                Some((&store, &q)),
            )
        };
        let hits = page("bankverbindung", Some(KIND_TEXT)).entries;
        let ids: Vec<&str> = hits.iter().map(|e| e.uuid.as_str()).collect();
        assert_eq!(ids, ["wort", "iban2", "iban", "unscharf"]);
        let flags: Vec<bool> = hits.iter().map(|e| e.similar).collect();
        assert_eq!(flags, [false, true, true, false]);
        let ids: Vec<String> = page("bankverbindung -extra", None)
            .entries
            .into_iter()
            .map(|e| e.uuid)
            .collect();
        assert_eq!(ids, ["wort", "datei", "iban", "unscharf"]);
    }

    /// Tippfehler zählen in Zeichen, nicht in UTF-8-Bytes.
    #[test]
    fn typo_counts_chars() {
        let typo = |t: &str, words: &str| {
            let t: Vec<char> = t.chars().collect();
            typo_match(&t, words, &mut HashMap::new())
        };
        assert!(typo("東大京阪京都", " 東京大阪京都"));
        assert!(typo("rechnug", " die rechnung"));
        assert!(!typo("rechnug", " die zeichnung"));
    }

    /// Extreme Zeitstempel (etwa aus einem Import) laufen nicht über.
    #[test]
    fn bonus_saturates_timestamps() {
        let keys = Secret::generate().unwrap().derive_keys();
        let index = SearchIndex::build(&[row(&keys, "a", KIND_TEXT, "x", i64::MIN)], &keys);
        assert_eq!(bonus(&index.entries[0].dto, i64::MAX), 0);
    }

    /// Dieselben Fälle wie `foldTerm` in `src/lib/preview.test.ts`.
    #[test]
    fn fold_words_mirrors_frontend() {
        assert_eq!(fold_words("Straße"), " strasse");
        assert_eq!(fold_words("März"), " marz");
        assert_eq!(fold_words("MAERZ"), " marz");
        assert_eq!(fold_words("Ma\u{308}rz"), " marz");
        assert_eq!(fold_words("Grüße, Öl"), " grusse ol");
        assert_eq!(fold_words("Café-Bar 3"), " cafe bar 3");
        assert_eq!(fold_words("poésie"), " poesie");
        assert_eq!(fold_words("poe\u{301}sie"), " poesie");
        assert_eq!(fold_words("Poesie"), " posie");
        assert_eq!(fold_words("  "), "");
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
            similar: false,
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
            ("rechnug kunde servr", 200),
            ("rechnug kudne servr passwrot angeobt temrin bakcup", 200),
            ("rechnung kunde server passwort angebot termin backup", 200),
            ("\"rechnung kunde\" -drucker", 200),
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
