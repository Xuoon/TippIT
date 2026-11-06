//! Suche nach Bedeutung (Opt-in, ab Werk aus).
//!
//! Beim Einschalten lädt ein Hintergrund-Thread das Modell (feste
//! Revision, geprüft, `download`) nach `~/.labi/tippit/models/`, bettet alle
//! Texteinträge ein und legt die Vektoren verschlüsselt in `history.db` ab
//! (`storage::vectors`). Die Vektoren liegen ab dann im RAM (wenige MB); das
//! Modell selbst nur, solange es gebraucht wird: es lädt beim Öffnen der
//! Historie und fällt nach `IDLE_UNLOAD` ohne Suche wieder heraus. Neue Kopien
//! bettet es ein, sobald es das nächste Mal geladen ist.
//!
//! Ausschalten entlädt nur; Modell und Vektoren bleiben, bis „Löschen“ sie
//! entfernt. Suchanfragen warten nie auf das Laden: ohne geladenes Modell
//! bleibt es bei der Wortsuche, und `history-changed` frischt die Liste auf,
//! sobald es bereit ist.

mod download;
pub mod models;
mod modernbert;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;
use crate::storage::vectors::{self, VectorStats, VectorStore};
use models::{Embedder, MODEL};

/// Ohne Suche so lange geladen lassen.
const IDLE_UNLOAD: Duration = Duration::from_secs(10 * 60);
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
    /// Jemand will das Modell (Historie offen, Suche, Einschalten).
    want_load: bool,
    /// Anstoß für den Worker, bevor `TICK` abläuft.
    kicked: bool,
    /// Letzte Suchanfrage und ihr Vektor (Nachladen weiterer Seiten).
    last_query: Option<(String, Vec<f32>)>,
}

pub struct Semantic {
    inner: Mutex<Inner>,
    wake: Condvar,
    store: RwLock<VectorStore>,
    /// Download abbrechen (Ausschalten, Löschen).
    cancel: AtomicBool,
}

impl Semantic {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            wake: Condvar::new(),
            store: RwLock::new(VectorStore::default()),
            cancel: AtomicBool::new(false),
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

    /// Treffer nach Bedeutung für die Suche; nie blockierend auf das Laden.
    pub fn similar(&self, app: &AppHandle, query: &str) -> Vec<(String, f32)> {
        let query = meaning_text(query);
        if query.chars().count() < MIN_QUERY_CHARS || !enabled(app) {
            return Vec::new();
        }
        let embedder = {
            let mut inner = self.inner.lock().unwrap();
            inner.last_use = Some(Instant::now());
            if let Some((q, v)) = &inner.last_query {
                if *q == query {
                    return self.store.read().unwrap().similar(v);
                }
            }
            match &inner.embedder {
                Some(e) => e.clone(),
                None => {
                    drop(inner);
                    self.kick(true);
                    return Vec::new();
                }
            }
        };
        let v = match embedder.embed(&[query.as_str()]) {
            Ok(mut v) => v.remove(0),
            Err(e) => {
                tracing::warn!("Suchanfrage nicht einbettbar: {e}");
                return Vec::new();
            }
        };
        let hits = self.store.read().unwrap().similar(&v);
        self.inner.lock().unwrap().last_query = Some((query, v));
        hits
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

/// Historie geöffnet: Modell vorab laden, damit die erste Suche es schon hat.
pub fn touch(app: &AppHandle) {
    if enabled(app) {
        app.state::<Semantic>().kick(true);
    }
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
        if let Err(e) = step(app, &sem) {
            // Abgebrochen (Ausschalten, Modellwechsel): der nächste Lauf
            // folgt der neuen Einstellung, das ist kein Fehler.
            if sem.cancel.load(Ordering::SeqCst) {
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
    let mut inner = sem.inner.lock().unwrap();
    inner.embedder = None;
    inner.last_query = None;
    inner.want_load = false;
}

/// Ein Durchlauf: Zustand an die Einstellung angleichen, dann arbeiten.
fn step(app: &AppHandle, sem: &Semantic) -> anyhow::Result<()> {
    if !enabled(app) {
        unload(sem);
        set_phase(app, Phase::Off);
        return Ok(());
    }
    sem.cancel.store(false, Ordering::SeqCst);
    let dir = model_dir(app);
    if !download::is_complete(&dir) {
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
            &sem.cancel,
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
    let mut fresh = false;
    if !loaded {
        if !want {
            set_phase(app, Phase::Ready);
            return Ok(());
        }
        set_phase(app, Phase::Loading);
        let started = Instant::now();
        let embedder = Arc::new(Embedder::load(&dir)?);
        tracing::info!("Modell {id} geladen in {:?}", started.elapsed());
        let mut inner = sem.inner.lock().unwrap();
        inner.embedder = Some(embedder);
        inner.want_load = false;
        inner.last_use = Some(Instant::now());
        fresh = true;
    }
    let embedded = index_missing(app, sem)?;
    set_phase(app, Phase::Ready);
    if fresh || embedded > 0 {
        // Liste neu laden: jetzt gibt es (mehr) Treffer nach Bedeutung.
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
        if !enabled(app) {
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
    remove_model_dirs(app, &keep);
    let state = app.state::<AppState>();
    let deleted = vectors::delete_stale(&state.db.lock().unwrap(), &keep);
    if let Err(e) = deleted {
        tracing::warn!("Alte Vektoren nicht gelöscht: {e}");
    }
}

fn remove_model_dirs(app: &AppHandle, keep: &[String]) {
    let root = app.state::<AppState>().paths.models_dir();
    let Ok(dirs) = std::fs::read_dir(&root) else {
        return;
    };
    for dir in dirs.flatten() {
        let name = dir.file_name().to_string_lossy().into_owned();
        if !keep.contains(&name) {
            if let Err(e) = std::fs::remove_dir_all(dir.path()) {
                tracing::warn!("Modellordner {name} nicht gelöscht: {e}");
            }
        }
    }
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
    SemanticStatus {
        enabled: enabled(app),
        phase,
        loaded,
        ram_bytes: store_bytes + if loaded { MODEL.ram_bytes } else { 0 },
        model_ram_bytes: MODEL.ram_bytes,
        download_bytes: MODEL.download_bytes(),
        model_bytes: dir_size(&state.paths.models_dir()),
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
    set_enabled(&app, true)?;
    app.state::<Semantic>().kick(true);
    Ok(status_of(&app))
}

/// Ausschalten: Modell aus dem RAM, Download abbrechen. Dateien bleiben.
#[tauri::command(async)]
pub fn semantic_disable(app: AppHandle) -> Result<SemanticStatus, String> {
    set_enabled(&app, false)?;
    let sem = app.state::<Semantic>();
    sem.cancel.store(true, Ordering::SeqCst);
    unload(&sem);
    sem.kick(false);
    set_phase(&app, Phase::Off);
    Ok(status_of(&app))
}

/// Ausschalten und alles entfernen: Modellordner und Vektoren. Die Historie
/// selbst bleibt unberührt.
#[tauri::command(async)]
pub fn semantic_delete(app: AppHandle) -> Result<SemanticStatus, String> {
    semantic_disable(app.clone())?;
    remove_model_dirs(&app, &[]);
    let state = app.state::<AppState>();
    vectors::delete_all(&state.db.lock().unwrap()).map_err(|e| e.to_string())?;
    *app.state::<Semantic>().store.write().unwrap() = VectorStore::default();
    tracing::info!("Modell und Vektoren der Suche nach Bedeutung gelöscht");
    Ok(status_of(&app))
}

/// Bereinigen: Vektoren und Modellordner älterer Revisionen und verwaiste
/// Vektoren entfernen.
#[tauri::command(async)]
pub fn semantic_cleanup(app: AppHandle) -> Result<SemanticStatus, String> {
    let keep = [MODEL.id()];
    remove_model_dirs(&app, &keep);
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
