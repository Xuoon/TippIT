use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::OnceLock;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, GetClipboardSequenceNumber,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW,
    TranslateMessage, HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLIPBOARDUPDATE,
    WNDCLASSW,
};

use crate::state::AppState;
use crate::storage::crypto;
use crate::storage::db::{self, EntryRow, KIND_FILES, KIND_IMAGE, KIND_TEXT};

use super::read::{read_clipboard, ClipContent};

static UPDATE_TX: OnceLock<Sender<()>> = OnceLock::new();

/// Startet den eventbasierten Clipboard-Monitor:
/// Message-Only-Window + AddClipboardFormatListener (kein Polling).
pub fn start(app: AppHandle) {
    let (tx, rx) = mpsc::channel::<()>();
    UPDATE_TX.set(tx).ok();
    std::thread::spawn(|| unsafe { message_pump() });
    std::thread::spawn(move || capture_worker(app, rx));
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    if msg == WM_CLIPBOARDUPDATE {
        if let Some(tx) = UPDATE_TX.get() {
            let _ = tx.send(());
        }
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}

unsafe fn message_pump() {
    unsafe {
        let class_name = w!("TippITClipboardMonitor");
        let hinstance = GetModuleHandleW(None).expect("GetModuleHandleW");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };
        if RegisterClassW(&wc) == 0 {
            tracing::error!("RegisterClassW für Clipboard-Monitor fehlgeschlagen");
            return;
        }
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class_name,
            None,
            WINDOW_STYLE(0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(hinstance.into()),
            None,
        )
        .expect("Clipboard-Monitor-Fenster");
        AddClipboardFormatListener(hwnd).expect("AddClipboardFormatListener");

        let mut msg = MSG::default();
        loop {
            let ret = GetMessageW(&mut msg, None, 0, 0);
            // 0 = WM_QUIT, -1 = Fehler — beides beendet die Pump (kein Busy-Loop).
            if ret.0 <= 0 {
                if ret.0 == -1 {
                    tracing::error!("GetMessageW-Fehler im Clipboard-Monitor");
                }
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn capture_worker(app: AppHandle, rx: Receiver<()>) {
    while rx.recv().is_ok() {
        // Debounce: manche Apps feuern eine Kopie als mehrere Updates.
        while rx.recv_timeout(Duration::from_millis(150)).is_ok() {}
        if let Err(e) = capture(&app) {
            tracing::warn!("Clipboard-Capture fehlgeschlagen: {e}");
        }
    }
}

fn capture(app: &AppHandle) -> anyhow::Result<()> {
    let state = app.state::<AppState>();

    // Pause stoppt auch die Erfassung — sonst landet z. B. ein bewusst
    // "unbeobachtet" kopiertes Passwort doch in Historie und Sync.
    if state.paused.load(Ordering::SeqCst) {
        return Ok(());
    }

    let seq = unsafe { GetClipboardSequenceNumber() };

    // Eigener Write (Copy aus der Historie / Kopplungscode): genau diese Sequenz
    // überspringen. Hat der Nutzer danach schon wieder kopiert, ist seq neuer
    // und die Kopie wird normal erfasst.
    if seq == state.own_clip_seq.load(Ordering::SeqCst) {
        state.last_clip_seq.store(seq, Ordering::SeqCst);
        return Ok(());
    }

    // Sequence-Number-Dedupe: gleiche Sequenz = schon verarbeitet.
    if state.last_clip_seq.swap(seq, Ordering::SeqCst) == seq {
        return Ok(());
    }

    let (capture_images, capture_files, max_entries) = {
        let s = state.settings.read().unwrap();
        (
            s.history.capture_images,
            s.history.capture_files,
            s.history.max_entries,
        )
    };

    let Some(content) = read_clipboard(capture_images, capture_files) else {
        return Ok(());
    };

    let (kind, plain, thumb) = match content {
        ClipContent::Text(t) => (KIND_TEXT, t.into_bytes(), None),
        ClipContent::Files(files) => (KIND_FILES, serde_json::to_vec(&files)?, None),
        ClipContent::Image { png, thumb_png } => (KIND_IMAGE, png, Some(thumb_png)),
    };
    let hash = crypto::sha256(&plain);

    let db = state.db.lock().unwrap();
    let now_ms = now_ms();

    if let Some(existing) = db::find_by_hash(&db, &hash)? {
        // Duplikat: nach oben schieben (Parität zur AutoIt-Version).
        let lamport = db::next_lamport(&db)?;
        db::touch(&db, &existing, now_ms, lamport)?;
        state.index.write().unwrap().touch(&existing, now_ms);
    } else {
        let uuid = uuid::Uuid::now_v7().to_string();
        let keys = state.keys.read().unwrap().clone();
        let cipher = crypto::encrypt(&keys, &uuid, kind, &plain)?;
        let thumb_cipher = thumb
            .map(|t| crypto::encrypt(&keys, &uuid, kind, &t))
            .transpose()?;
        let row = EntryRow {
            uuid,
            kind,
            cipher: Some(cipher),
            thumb: thumb_cipher,
            size_bytes: plain.len() as i64,
            hash: hash.to_vec(),
            created_at: now_ms,
            pinned: false,
            deleted: false,
            device_id: state.device_id.clone(),
            lamport: db::next_lamport(&db)?,
        };
        db::insert(&db, &row)?;
        state.index.write().unwrap().upsert(&row, &keys);

        for pruned in db::prune(&db, max_entries)? {
            state.index.write().unwrap().remove(&pruned);
        }
    }
    drop(db);

    state.notify_push();
    let _ = app.emit("history-changed", ());
    Ok(())
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
