//! Suche nach Bedeutung (Opt-in, ab Werk aus).
//!
//! Beim Einschalten lädt ein Hintergrund-Thread das Modell (feste
//! Revision, geprüft, `download`) nach `~/.labi/tippit/models/`, bettet alle
//! Texteinträge ein und legt die Vektoren verschlüsselt in `history.db` ab
//! (`storage::vectors`). Die Vektoren liegen ab dann im RAM (wenige MB); das
//! Modell selbst nur, solange es gebraucht wird: die erste Suche lädt es (rund
//! 1 s), nach `IDLE_UNLOAD` ohne Suche fällt es wieder heraus, und der Heap
//! geht ans System zurück (`platform::release_memory`). Neue Kopien bettet es
//! ein, sobald es das nächste Mal geladen ist.
//!
//! Ausschalten entlädt nur; Modell und Vektoren bleiben, bis „Löschen“ sie
//! entfernt. Suchanfragen warten nie auf das Laden: ohne geladenes Modell
//! bleibt es bei der Wortsuche, und `history-changed` frischt die Liste auf,
//! sobald es bereit ist.

mod download;
pub mod models;
mod modernbert;

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;
use crate::storage::vectors::{self, VectorStats, VectorStore};
use models::{Embedder, MODEL};

/// Ohne Suche so lange geladen lassen; erneutes Laden kostet rund 1 s.
const IDLE_UNLOAD: Duration = Duration::from_secs(3 * 60);
/// So oft schaut der Worker ohne Anstoß nach (Entladen, neue Kopien).
const TICK: Duration = Duration::from_secs(30);
/// Einträge je Einbettungs-Stapel; dazwischen greifen Abbruch und Suche.
const BATCH: usize = 16;
/// Kürzere Suchtexte tragen keine Bedeutung.
const MIN_QUERY_CHARS: usize = 3;

/// Teil von `Settings`; gehört dem Backend (`merge_settings` übernimmt ihn
/// nie aus dem Einstellungsfenster), geändert nur über die Befehle unten.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct SemanticSettings {
    pub enabled: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum Phase {
    #[default]
    Off,
    /// Bytes geladen von gesamt.
    Downloading {
        done: u64,
        total: u64,
    },
    Loading,
    /// Einträge eingebettet von gesamt.
    Indexing {
        done: u64,
        total: u64,
    },
    /// Bereit; ob das Modell gerade im RAM liegt, sagt `SemanticStatus::loaded`.
    Ready,
    Error {
        message: String,
    },
}

#[derive(Default)]
struct Inner {
    phase: Phase,
    embedder: Option<Arc<Embedder>>,
    last_use: Option<Instant>,
    /// Jemand will das Modell (Suche, Einschalten).
    want_load: bool,
    /// Anstoß für den Worker, bevor `TICK` abläuft.
    kicked: bool,
    /// Letzte Suchanfrage und ihr Vektor (Nachladen weiterer Seiten).
    last_query: Option<(String, Vec<f32>)>,
}

pub struct Semantic {
    inner: Mutex<Inner>,
    wake: Condvar,
    pub store: RwLock<VectorStore>,
    /// Download abbrechen (Ausschalten, Löschen).
    cancel: AtomicBool,
    /// Hält der Worker, solange er einen Durchlauf macht. Ausschalten und
    /// Löschen warten darauf, damit kein alter Lauf danach noch das Modell
    /// einsetzt oder Vektoren schreibt. Immer vor allen anderen Sperren nehmen.
    work: Mutex<()>,
}

impl Semantic {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            wake: Condvar::new(),
            store: RwLock::new(VectorStore::default()),
            cancel: AtomicBool::new(false),
            work: Mutex::new(()),
        }
    }

    fn kick(&self, want_load: bool) {
        let mut inner = self.inner.lock().unwrap();
        inner.kicked = true;
        if want_load {
            inner.want_load = true;
            inner.last_use = Some(Instant::now());
        }
        self.wake.notify_all();
    }

    /// Vektor der Suchanfrage; `None` ohne geladenes Modell (nie blockierend
    /// auf das Laden). Welche Einträge ähnlich sind, entscheidet der Index
    /// (`SearchIndex::search_with`), weil nur er Filter und Papierkorb kennt.
    pub fn query_vector(&self, app: &AppHandle, query: &str) -> Option<Vec<f32>> {
        let query = meaning_text(query);
        if query.chars().count() < MIN_QUERY_CHARS || !enabled(app) {
            return None;
        }
        let embedder = {
            let mut inner = self.inner.lock().unwrap();
            inner.last_use = Some(Instant::now());
            if let Some((q, v)) = &inner.last_query {
                if *q == query {
                    return Some(v.clone());
                }
            }
            match &inner.embedder {
                Some(e) => e.clone(),
                None => {
                    drop(inner);
                    self.kick(true);
                    return None;
                }
            }
        };
        let result = embedder.embed(&[query.as_str()]);
        // Inzwischen entladen: dann fällt das Modell erst hier heraus.
        release(embedder);
        let v = match result {
            Ok(mut v) => v.remove(0),
            Err(e) => {
                tracing::warn!("Suchanfrage nicht einbettbar: {e}");
                return None;
            }
        };
        self.inner.lock().unwrap().last_query = Some((query, v.clone()));
        Some(v)
    }
}

/// Text der Anfrage ohne Operatoren: `-wort` fällt weg, Anführungszeichen auch.
fn meaning_text(query: &str) -> String {
    query
        .split_whitespace()
        .filter(|w| !w.starts_with('-'))
        .collect::<Vec<_>>()
        .join(" ")
        .replace('"', "")
}

fn enabled(app: &AppHandle) -> bool {
    app.state::<AppState>()
        .settings
        .read()
        .unwrap()
        .semantic
        .enabled
}

fn model_dir(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().paths.models_dir().join(MODEL.id())
}

/// Worker-Thread starten (einmal in `setup`).
pub fn start(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("tippit-semantic".into())
        .spawn(move || worker(&app));
    if let Err(e) = spawned {
        tracing::error!("Worker der Suche nach Bedeutung startet nicht: {e}");
    }
}

fn worker(app: &AppHandle) {
    let sem = app.state::<Semantic>();
    loop {
        {
            let inner = sem.inner.lock().unwrap();
            let (mut inner, _) = sem
                .wake
                .wait_timeout_while(inner, TICK, |i| !i.kicked)
                .unwrap();
            inner.kicked = false;
        }
        let result = {
            let _work = sem.work.lock().unwrap();
            step(app, &sem)
        };
        if let Err(e) = result {
            // Abgebrochen (Ausschalten, Löschen): der nächste Lauf folgt der
            // neuen Einstellung, das ist kein Fehler.
            if cancelled(app, &sem) {
                continue;
            }
            tracing::warn!("Suche nach Bedeutung: {e:#}");
            set_phase(
                app,
                Phase::Error {
                    message: format!("{e:#}"),
                },
            );
            // Erst auf den nächsten Anstoß wieder versuchen, nicht im Takt.
            sem.inner.lock().unwrap().want_load = false;
        }
    }
}

/// Laufende Arbeit soll aufhören: ausgeschaltet oder Löschen angefordert.
fn cancelled(app: &AppHandle, sem: &Semantic) -> bool {
    sem.cancel.load(Ordering::SeqCst) || !enabled(app)
}

fn set_phase(app: &AppHandle, phase: Phase) {
    let sem = app.state::<Semantic>();
    let changed = {
        let mut inner = sem.inner.lock().unwrap();
        let changed = inner.phase != phase;
        inner.phase = phase;
        changed
    };
    if changed {
        let _ = app.emit("semantic-status", status_of(app));
    }
}

fn unload(sem: &Semantic) {
    let embedder = {
        let mut inner = sem.inner.lock().unwrap();
        inner.last_query = None;
        inner.want_load = false;
        inner.embedder.take()
    };
    if let Some(embedder) = embedder {
        release(embedder);
    }
}

/// Einen Besitzer des Modells abgeben. War es der letzte (eine laufende Suche
/// hält ihren eigenen), fällt das Modell jetzt heraus, und der freie Heap geht
/// ans System zurück.
fn release(embedder: Arc<Embedder>) {
    if let Some(embedder) = Arc::into_inner(embedder) {
        drop(embedder);
        crate::platform::release_memory();
    }
}

/// Ein Durchlauf: Zustand an die Einstellung angleichen, dann arbeiten.
fn step(app: &AppHandle, sem: &Semantic) -> anyhow::Result<()> {
    if !enabled(app) {
        unload(sem);
        set_phase(app, Phase::Off);
        return Ok(());
    }
    let dir = model_dir(app);
    if !download::is_complete(&MODEL, &dir) {
        set_phase(
            app,
            Phase::Downloading {
                done: 0,
                total: MODEL.download_bytes(),
            },
        );
        let last = std::cell::Cell::new(Instant::now());
        tauri::async_runtime::block_on(download::fetch(
            &MODEL,
            &dir,
            &|| cancelled(app, sem),
            |done, total| {
                if last.get().elapsed() >= Duration::from_millis(250) || done == total {
                    last.set(Instant::now());
                    set_phase(app, Phase::Downloading { done, total });
                }
            },
        ))?;
        remove_outdated(app);
    }
    let state = app.state::<AppState>();
    let id = MODEL.id();
    if sem.store.read().unwrap().model != id {
        let rows = vectors::load(&state.db.lock().unwrap(), &state.keys, &id)?;
        *sem.store.write().unwrap() = VectorStore::new(&id, rows);
        sem.inner.lock().unwrap().last_query = None;
    }
    // Gelöschtes und Papierkorb auch aus dem RAM (die DB räumt endgültig
    // Gelöschtes per Trigger); ein wiederhergestellter Eintrag wird neu
    // eingebettet.
    let active: Vec<String> = {
        let index = state.index.read().unwrap();
        index.texts().map(|(u, _)| u.to_owned()).collect()
    };
    let active: HashSet<&str> = active.iter().map(String::as_str).collect();
    sem.store.write().unwrap().retain(&active);

    let (want, loaded, idle) = {
        let inner = sem.inner.lock().unwrap();
        let idle = inner.last_use.is_none_or(|t| t.elapsed() >= IDLE_UNLOAD);
        (inner.want_load, inner.embedder.is_some(), idle)
    };
    if loaded && idle {
        unload(sem);
        tracing::info!("Modell der Suche nach Bedeutung entladen (unbenutzt)");
        set_phase(app, Phase::Ready);
        return Ok(());
    }
    if !loaded {
        if !want {
            set_phase(app, Phase::Ready);
            return Ok(());
        }
        set_phase(app, Phase::Loading);
        let started = Instant::now();
        let embedder = match Embedder::load(&dir) {
            Ok(e) => Arc::new(e),
            Err(e) => {
                // Beschädigt oder ersetzt: der nächste Lauf prüft jede Datei
                // und lädt neu, was nicht stimmt.
                download::invalidate(&dir);
                return Err(e);
            }
        };
        // Reste vom Einlesen des Tokenizer-JSON (rund 150 MB in vielen kleinen
        // Blöcken) sind frei, liegen aber noch beim Prozess.
        crate::platform::release_memory();
        tracing::info!("Modell {id} geladen in {:?}", started.elapsed());
        if cancelled(app, sem) {
            release(embedder);
            return Ok(());
        }
        {
            let mut inner = sem.inner.lock().unwrap();
            inner.embedder = Some(embedder);
            inner.want_load = false;
            inner.last_use = Some(Instant::now());
        }
        // Liste neu laden, noch vor dem Einbetten neuer Kopien: die Suche, die
        // das Laden angestoßen hat, bekommt so gleich ihre Treffer nach
        // Bedeutung.
        let _ = app.emit("history-changed", ());
    }
    let embedded = index_missing(app, sem)?;
    set_phase(app, Phase::Ready);
    if embedded > 0 {
        // Jetzt gibt es mehr Treffer nach Bedeutung.
        let _ = app.emit("history-changed", ());
    }
    Ok(())
}

/// Texteinträge ohne Vektor einbetten, neueste zuerst, stapelweise; liefert
/// die Anzahl.
fn index_missing(app: &AppHandle, sem: &Semantic) -> anyhow::Result<u64> {
    let state = app.state::<AppState>();
    let todo: Vec<(String, String)> = {
        let store = sem.store.read().unwrap();
        state
            .index
            .read()
            .unwrap()
            .texts()
            .filter(|(u, _)| !store.contains(u))
            .map(|(u, t)| (u.to_owned(), t.to_owned()))
            .collect()
    };
    let total = todo.len() as u64;
    if total == 0 {
        return Ok(0);
    }
    let id = MODEL.id();
    let mut done = 0;
    for batch in todo.chunks(BATCH) {
        if cancelled(app, sem) {
            return Ok(done);
        }
        set_phase(app, Phase::Indexing { done, total });
        let Some(embedder) = sem.inner.lock().unwrap().embedder.clone() else {
            return Ok(done);
        };
        let texts: Vec<&str> = batch.iter().map(|(_, t)| t.as_str()).collect();
        let vecs = embedder.embed(&texts)?;
        {
            let db = state.db.lock().unwrap();
            let mut store = sem.store.write().unwrap();
            for ((uuid, _), v) in batch.iter().zip(vecs) {
                // Inzwischen gelöscht: kein Vektor ohne Eintrag.
                if crate::storage::db::get(&db, uuid)?.is_none() {
                    continue;
                }
                vectors::put(&db, &state.keys, uuid, &id, &v)?;
                store.insert(uuid.clone(), v);
            }
        }
        done += batch.len() as u64;
        // Laufendes Einbetten zählt als Nutzung, sonst entlüde der nächste Takt.
        sem.inner.lock().unwrap().last_use = Some(Instant::now());
    }
    tracing::info!("{total} Einträge für die Suche nach Bedeutung eingebettet");
    Ok(done)
}

/// Ordner und Vektoren älterer Revisionen löschen (nach einem Update mit
/// neuem Modell).
fn remove_outdated(app: &AppHandle) {
    let keep = [MODEL.id()];
    if let Err(e) = remove_model_dirs(app, &keep) {
        tracing::warn!("{e:#}");
    }
    let state = app.state::<AppState>();
    let deleted = vectors::delete_stale(&state.db.lock().unwrap(), &keep);
    if let Err(e) = deleted {
        tracing::warn!("Alte Vektoren nicht gelöscht: {e}");
    }
}

/// Modellordner außer `keep` löschen; meldet den ersten, der bleibt.
fn remove_model_dirs(app: &AppHandle, keep: &[String]) -> anyhow::Result<()> {
    let root = app.state::<AppState>().paths.models_dir();
    let Ok(dirs) = std::fs::read_dir(&root) else {
        return Ok(());
    };
    let mut failed = None;
    for dir in dirs.flatten() {
        let name = dir.file_name().to_string_lossy().into_owned();
        if !keep.contains(&name) {
            if let Err(e) = std::fs::remove_dir_all(dir.path()) {
                tracing::warn!("Modellordner {name} nicht gelöscht: {e}");
                failed.get_or_insert(anyhow::anyhow!("Modellordner {name} nicht gelöscht: {e}"));
            }
        }
    }
    failed.map_or(Ok(()), Err)
}

fn dir_size(path: &std::path::Path) -> u64 {
    std::fs::read_dir(path).map_or(0, |entries| {
        entries
            .flatten()
            .map(|e| match e.file_type() {
                Ok(t) if t.is_dir() => dir_size(&e.path()),
                Ok(_) => e.metadata().map_or(0, |m| m.len()),
                Err(_) => 0,
            })
            .sum()
    })
}

#[derive(Clone, Serialize)]
pub struct SemanticStatus {
    pub enabled: bool,
    #[serde(flatten)]
    pub phase: Phase,
    /// Modell liegt gerade im RAM.
    pub loaded: bool,
    /// Geschätzter RAM: Modell (falls geladen) plus Vektoren.
    pub ram_bytes: u64,
    /// RAM des Modells, solange es geladen ist.
    pub model_ram_bytes: u64,
    /// Download-Größe des Modells.
    pub download_bytes: u64,
    /// Modellordner auf der Platte (auch ältere Revisionen).
    pub model_bytes: u64,
    /// Davon alles außer dem Ordner des aktuellen Modells: „Bereinigen“
    /// entfernt es.
    pub outdated_model_bytes: u64,
    pub vectors: VectorStats,
    /// Texteinträge, die einen Vektor bekommen.
    pub embeddable: usize,
}

fn status_of(app: &AppHandle) -> SemanticStatus {
    let state = app.state::<AppState>();
    let sem = app.state::<Semantic>();
    let (phase, loaded) = {
        let inner = sem.inner.lock().unwrap();
        (inner.phase.clone(), inner.embedder.is_some())
    };
    let vectors = vectors::stats(&state.db.lock().unwrap(), &MODEL.id()).unwrap_or_default();
    let store_bytes = sem.store.read().unwrap().bytes() as u64;
    let embeddable = state.index.read().unwrap().texts().count();
    let model_bytes = dir_size(&state.paths.models_dir());
    SemanticStatus {
        enabled: enabled(app),
        phase,
        loaded,
        ram_bytes: store_bytes + if loaded { MODEL.ram_bytes } else { 0 },
        model_ram_bytes: MODEL.ram_bytes,
        download_bytes: MODEL.download_bytes(),
        model_bytes,
        outdated_model_bytes: model_bytes - dir_size(&model_dir(app)).min(model_bytes),
        vectors,
        embeddable,
    }
}

fn set_enabled(app: &AppHandle, on: bool) -> Result<(), String> {
    let state = app.state::<AppState>();
    let settings = {
        let mut current = state.settings.write().unwrap();
        let mut next = current.clone();
        next.semantic.enabled = on;
        next.save(&state.paths).map_err(|e| e.to_string())?;
        *current = next.clone();
        next
    };
    let _ = app.emit("settings-changed", settings);
    Ok(())
}

#[tauri::command(async)]
pub fn semantic_status(app: AppHandle) -> SemanticStatus {
    status_of(&app)
}

/// Einschalten; lädt das Modell und bettet im Hintergrund ein.
#[tauri::command(async)]
pub fn semantic_enable(app: AppHandle) -> Result<SemanticStatus, String> {
    let sem = app.state::<Semantic>();
    set_enabled(&app, true)?;
    sem.cancel.store(false, Ordering::SeqCst);
    sem.kick(true);
    Ok(status_of(&app))
}

/// Ausschalten und abwarten, bis ein laufender Durchlauf aufgehört hat
/// (Download sofort, Einbetten nach dem laufenden Stapel). Danach setzt kein
/// alter Lauf mehr das Modell ein. Liefert die Sperre, solange der Aufrufer
/// noch aufräumt.
fn stop(app: &AppHandle) -> Result<std::sync::MutexGuard<'_, ()>, String> {
    set_enabled(app, false)?;
    let sem = app.state::<Semantic>().inner();
    sem.cancel.store(true, Ordering::SeqCst);
    let work = sem.work.lock().unwrap();
    unload(sem);
    set_phase(app, Phase::Off);
    Ok(work)
}

/// Ausschalten: Modell aus dem RAM, Download abbrechen. Dateien bleiben.
#[tauri::command(async)]
pub fn semantic_disable(app: AppHandle) -> Result<SemanticStatus, String> {
    drop(stop(&app)?);
    Ok(status_of(&app))
}

/// Ausschalten und alles entfernen: Modellordner und Vektoren. Die Historie
/// selbst bleibt unberührt.
#[tauri::command(async)]
pub fn semantic_delete(app: AppHandle) -> Result<SemanticStatus, String> {
    let work = stop(&app)?;
    let state = app.state::<AppState>();
    vectors::delete_all(&state.db.lock().unwrap()).map_err(|e| e.to_string())?;
    *app.state::<Semantic>().store.write().unwrap() = VectorStore::default();
    let removed = remove_model_dirs(&app, &[]).map_err(|e| format!("{e:#}"));
    drop(work);
    removed?;
    tracing::info!("Modell und Vektoren der Suche nach Bedeutung gelöscht");
    Ok(status_of(&app))
}

/// Bereinigen: Vektoren und Modellordner älterer Revisionen und verwaiste
/// Vektoren entfernen.
#[tauri::command(async)]
pub fn semantic_cleanup(app: AppHandle) -> Result<SemanticStatus, String> {
    let keep = [MODEL.id()];
    // Ohne auf den Worker zu warten: er schreibt nur in den Ordner und die
    // Vektoren des aktuellen Modells, und die bleiben.
    remove_model_dirs(&app, &keep).map_err(|e| format!("{e:#}"))?;
    let state = app.state::<AppState>();
    let n = vectors::delete_stale(&state.db.lock().unwrap(), &keep).map_err(|e| e.to_string())?;
    tracing::info!("{n} veraltete Vektoren entfernt");
    Ok(status_of(&app))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meaning_text_drops_operators() {
        assert_eq!(
            meaning_text("\"noch einmal\" rechnung -log"),
            "noch einmal rechnung"
        );
        assert_eq!(meaning_text("  wlan   passwort "), "wlan passwort");
    }
}
