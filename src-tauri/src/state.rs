use std::sync::atomic::{AtomicBool, AtomicI64, AtomicIsize, AtomicU64};
use std::sync::{Mutex, RwLock};

use rusqlite::Connection;
use tokio::sync::mpsc::UnboundedSender;

use crate::storage::crypto::CryptoKeys;
use crate::storage::index::SearchIndex;
use crate::storage::paths::AppPaths;
use crate::storage::settings::Settings;

pub struct AppState {
    pub paths: AppPaths,
    pub settings: RwLock<Settings>,
    pub db: Mutex<Connection>,
    /// RwLock: beim Koppeln/Verlassen einer Sync-Gruppe rotieren die Schlüssel.
    pub keys: RwLock<CryptoKeys>,
    /// Barriere über DB-Umschlüsselung UND Key-Swap. Jeder Pfad, der Ciphertexte
    /// zusammen mit `keys` liest oder schreibt, hält sie für seinen Snapshot.
    pub rotation_lock: Mutex<()>,
    pub index: RwLock<SearchIndex>,
    pub device_id: String,
    pub paused: AtomicBool,
    /// Verhindert parallele Tipp-Vorgänge (zweiter Tipp-Hotkey während des Tippens).
    pub typing_lock: Mutex<()>,
    /// Generation-Counter fürs Tippen: Bump bricht einen laufenden Vorgang ab
    /// (Abbruch-Hotkey) und beendet den Tray-Blink-Task.
    /// INVARIANTE: Jeder Tipp-Vorgang zieht seine Generation ERST NACHDEM er
    /// `typing_lock` hält (`paste_clipboard`, `history::type_entry`). Nur so kann
    /// der terminale Bump am Vorgangsende keinen bereits neu gestarteten Vorgang
    /// entwerten. Ein neuer Einstiegspunkt darf diese Reihenfolge nie umkehren.
    pub typing_gen: AtomicU64,
    /// Generation-Counter: Bump beendet einen laufenden Blink-Task.
    pub blink_gen: AtomicU64,
    /// Clipboard-Sequenznummer des letzten EIGENEN Writes (Copy aus der Historie,
    /// Kopplungscode). Der Monitor überspringt exakt diese Sequenz — robuster als
    /// ein Zähler: kein Leak bei fehlgeschlagenem Write, und eine echte User-Kopie
    /// direkt nach unserem Write (neue Sequenz) wird trotzdem erfasst.
    /// (i64: Win32-Sequenznummer u32, macOS-changeCount isize — beides passt.)
    pub own_clip_seq: AtomicI64,
    pub last_clip_seq: AtomicI64,
    /// Tipp-Ziel, das vor dem Öffnen der Historie im Vordergrund war
    /// (Windows: HWND, macOS: PID — opak, nur platform::* interpretiert es).
    pub prev_target: AtomicIsize,
    /// Anzeigename + id der Ziel-App (für Footer „In {App} einfügen").
    pub prev_target_app: Mutex<Option<(String, String)>>,
    /// Generation-Counter für den Sync-Task (Bump beendet die laufende Loop).
    pub sync_gen: AtomicU64,
    /// Settings geändert und noch nicht gesynct.
    pub settings_dirty: AtomicBool,
    /// Weckt die Sync-Loop für einen Push.
    pub push_notify: Mutex<Option<UnboundedSender<()>>>,
}

impl AppState {
    pub fn new(
        paths: AppPaths,
        settings: Settings,
        db: Connection,
        keys: CryptoKeys,
        index: SearchIndex,
        device_id: String,
    ) -> Self {
        let settings_dirty = crate::sync::SyncState::load(&paths)
            .is_some_and(|sync_state| sync_state.settings_dirty);
        Self {
            paths,
            settings: RwLock::new(settings),
            db: Mutex::new(db),
            keys: RwLock::new(keys),
            rotation_lock: Mutex::new(()),
            index: RwLock::new(index),
            device_id,
            paused: AtomicBool::new(false),
            typing_lock: Mutex::new(()),
            typing_gen: AtomicU64::new(0),
            blink_gen: AtomicU64::new(0),
            own_clip_seq: AtomicI64::new(-1),
            last_clip_seq: AtomicI64::new(-1),
            prev_target: AtomicIsize::new(0),
            prev_target_app: Mutex::new(None),
            sync_gen: AtomicU64::new(0),
            settings_dirty: AtomicBool::new(settings_dirty),
            push_notify: Mutex::new(None),
        }
    }

    /// Sync-Loop anstoßen (no-op, wenn kein Sync läuft).
    pub fn notify_push(&self) {
        if let Some(tx) = self.push_notify.lock().unwrap().as_ref() {
            let _ = tx.send(());
        }
    }
}
