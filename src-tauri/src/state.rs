use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64};
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
    pub index: RwLock<SearchIndex>,
    pub device_id: String,
    pub paused: AtomicBool,
    /// Verhindert parallele Tipp-Vorgänge (zweites STRG+E während des Tippens).
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
    pub own_clip_seq: AtomicU32,
    pub last_clip_seq: AtomicU32,
    /// Fenster, das vor dem Öffnen der Historie fokussiert war (HWND).
    pub prev_hwnd: AtomicIsize,
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
        Self {
            paths,
            settings: RwLock::new(settings),
            db: Mutex::new(db),
            keys: RwLock::new(keys),
            index: RwLock::new(index),
            device_id,
            paused: AtomicBool::new(false),
            typing_lock: Mutex::new(()),
            typing_gen: AtomicU64::new(0),
            blink_gen: AtomicU64::new(0),
            own_clip_seq: AtomicU32::new(0),
            last_clip_seq: AtomicU32::new(0),
            prev_hwnd: AtomicIsize::new(0),
            sync_gen: AtomicU64::new(0),
            settings_dirty: AtomicBool::new(false),
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
