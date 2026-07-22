//! Windows-Backend der Plattform-Schicht (Win32/DPAPI/WinRT).

use std::sync::mpsc::Sender;
use std::sync::OnceLock;
use std::time::Duration;

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, GetClipboardSequenceNumber,
};
use windows::Win32::System::Diagnostics::Debug::Beep;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RETURN,
    VK_RWIN, VK_SHIFT, VK_TAB,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetAncestor, GetForegroundWindow,
    GetMessageW, IsWindowVisible, RegisterClassW, SetForegroundWindow, ShowWindow,
    SystemParametersInfoW, TranslateMessage, GA_ROOTOWNER, HWND_MESSAGE, MSG, SPI_GETWORKAREA,
    SW_HIDE, SW_SHOW, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_CLIPBOARDUPDATE, WNDCLASSW,
};

use crate::storage::settings::SyncSettings;
use crate::sync::policy::BlockReason;

use super::SpecialKey;

// ---------------------------------------------------------------------------
// Eingabe-Injektion
// ---------------------------------------------------------------------------

/// UTF-16-Einheiten als Unicode-Tastendrücke injizieren. Ein Aufruf = ein
/// SendInput = atomar; der Aufrufer schneidet nur an Zeichen-Grenzen, damit
/// keine fremde Eingabe ein Surrogatpaar zerreißen kann.
pub fn send_text(units: &[u16]) {
    let mut inputs = Vec::with_capacity(units.len() * 2);
    for &unit in units {
        inputs.push(keyboard_input(VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE));
        inputs.push(keyboard_input(
            VIRTUAL_KEY(0),
            unit,
            KEYEVENTF_UNICODE | KEYEVENTF_KEYUP,
        ));
    }
    send(&inputs);
}

pub fn send_key(key: SpecialKey) {
    let vk = match key {
        SpecialKey::Return => VK_RETURN,
        SpecialKey::Tab => VK_TAB,
    };
    let inputs = [
        keyboard_input(vk, 0, KEYBD_EVENT_FLAGS(0)),
        keyboard_input(vk, 0, KEYEVENTF_KEYUP),
    ];
    send(&inputs);
}

/// true, solange STRG/SHIFT/ALT/WIN physisch gehalten werden.
pub fn modifiers_held() -> bool {
    const MODS: [VIRTUAL_KEY; 5] = [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN];
    MODS.iter()
        .any(|vk| (unsafe { GetAsyncKeyState(vk.0 as i32) } as u16) & 0x8000 != 0)
}

/// Windows braucht keine Berechtigung für SendInput (UIPI drosselt nur
/// elevated Ziele, s. `send`).
pub fn ensure_input_permission() -> bool {
    true
}

fn keyboard_input(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send(inputs: &[INPUT]) {
    let sent = unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        // UIPI: Injektion in elevated Fenster wird ohne eigene Elevation still verworfen.
        tracing::warn!(
            "SendInput: nur {sent}/{} Events injiziert (Ziel elevated?)",
            inputs.len()
        );
    }
}

// ---------------------------------------------------------------------------
// Vordergrund-Ziel (Tipp-Ziel der Historie)
// ---------------------------------------------------------------------------

/// Aktuelles Vordergrund-Fenster als opakes Target (HWND als isize).
pub fn current_foreground() -> isize {
    unsafe { GetForegroundWindow() }.0 as isize
}

pub fn activate_target(target: isize) {
    let _ = unsafe { SetForegroundWindow(HWND(target as *mut core::ffi::c_void)) };
}

/// Warten, bis das Zielfenster wirklich im Vordergrund ist (Poll-Muster wie
/// `typing`-Modifier-Wait). Der GA_ROOTOWNER-Vergleich lässt legitime Fälle
/// durch, in denen das Foreground-HWND ein Dialog/Frame desselben Ziels ist
/// (UWP-ApplicationFrame, owned Dialoge).
pub fn wait_foreground(target: isize, timeout: Duration) -> bool {
    let target = HWND(target as *mut core::ffi::c_void);
    let start = std::time::Instant::now();
    loop {
        let fg = unsafe { GetForegroundWindow() };
        if fg == target
            || unsafe { GetAncestor(fg, GA_ROOTOWNER) == GetAncestor(target, GA_ROOTOWNER) }
        {
            return true;
        }
        if start.elapsed() >= timeout {
            return false;
        }
        std::thread::sleep(Duration::from_millis(15));
    }
}

// ---------------------------------------------------------------------------
// Fenster-Sichtbarkeit & Arbeitsbereich
// ---------------------------------------------------------------------------

// WICHTIG: Sichtbarkeit läuft komplett über Win32 (ShowWindow/IsWindowVisible),
// nicht über Tauris show()/hide()/is_visible(): historisch wurde das Fenster
// auch ohne Aktivierung gezeigt, wovon Tauris interner Visible-Zustand nichts
// mitbekommt — dessen hide() würde als No-op verpuffen (Fenster wäre "stuck").

fn hwnd_of(window: &tauri::WebviewWindow) -> Option<HWND> {
    window.hwnd().ok().map(|h| HWND(h.0))
}

pub fn window_visible(window: &tauri::WebviewWindow) -> bool {
    hwnd_of(window)
        .map(|hwnd| unsafe { IsWindowVisible(hwnd) }.as_bool())
        .unwrap_or(false)
}

pub fn hide_window(window: &tauri::WebviewWindow) {
    if let Some(hwnd) = hwnd_of(window) {
        let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
    } else {
        let _ = window.hide();
    }
}

/// Fenster MIT Aktivierung zeigen (Pfeiltasten/Sofort-Suche funktionieren direkt).
pub fn show_window_activated(window: &tauri::WebviewWindow) {
    match hwnd_of(window) {
        Some(hwnd) => {
            let _ = unsafe { ShowWindow(hwnd, SW_SHOW) };
            // Aus dem Hotkey-Kontext heraus haben wir Foreground-Rechte.
            let _ = unsafe { SetForegroundWindow(hwnd) };
        }
        None => {
            tracing::warn!("HWND nicht ermittelbar, zeige über Tauri");
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}

/// Arbeitsbereich des primären Monitors (ohne Taskbar) als
/// (links, oben, rechts, unten) in physischen Pixeln.
pub fn work_area(_window: &tauri::WebviewWindow) -> (f64, f64, f64, f64) {
    let mut rect = RECT::default();
    let _ = unsafe {
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut rect as *mut _ as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    (
        f64::from(rect.left),
        f64::from(rect.top),
        f64::from(rect.right),
        f64::from(rect.bottom),
    )
}

// ---------------------------------------------------------------------------
// Zwischenablage
// ---------------------------------------------------------------------------

/// Änderungszähler der Zwischenablage (für Dedupe + Eigen-Write-Erkennung).
pub fn clipboard_seq() -> i64 {
    i64::from(unsafe { GetClipboardSequenceNumber() })
}

/// Kopierte Dateien (CF_HDROP) — Priorität vor Text/Bild, s. clipboard::read.
pub fn clipboard_file_list() -> Option<Vec<String>> {
    clipboard_win::get_clipboard::<Vec<String>, _>(clipboard_win::formats::FileList)
        .ok()
        .filter(|files| !files.is_empty())
}

static UPDATE_TX: OnceLock<Sender<()>> = OnceLock::new();

/// Eventbasierter Clipboard-Monitor: Message-Only-Window +
/// AddClipboardFormatListener (kein Polling). Sendet pro Update ein Signal.
pub fn watch_clipboard(tx: Sender<()>) {
    UPDATE_TX.set(tx).ok();
    std::thread::spawn(|| unsafe { message_pump() });
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

// ---------------------------------------------------------------------------
// Sound
// ---------------------------------------------------------------------------

/// Blockierender kernel32-Beep (blockiert für die volle Dauer).
pub fn beep_blocking(freq: u32, duration_ms: u32) {
    let _ = unsafe { Beep(freq, duration_ms) };
}

// ---------------------------------------------------------------------------
// Schlüsselschutz at-rest (DPAPI, User-Scope — ohne Passwortabfrage)
// ---------------------------------------------------------------------------

const ENTROPY: &[u8] = b"TippIT.v1";

pub fn protect(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    dpapi::protect(data)
}

pub fn unprotect(data: &[u8]) -> anyhow::Result<Vec<u8>> {
    dpapi::unprotect(data)
}

mod dpapi {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::LocalFree;
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };

    fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB {
            cbData: data.len() as u32,
            pbData: data.as_ptr() as *mut u8,
        }
    }

    pub fn protect(data: &[u8]) -> anyhow::Result<Vec<u8>> {
        let input = blob(data);
        let entropy = blob(super::ENTROPY);
        let mut output = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptProtectData(
                &input,
                PWSTR::null(),
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
            .map_err(|e| anyhow::anyhow!("CryptProtectData: {e}"))?;
            let out = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
            let _ = LocalFree(Some(windows::Win32::Foundation::HLOCAL(
                output.pbData as *mut core::ffi::c_void,
            )));
            Ok(out)
        }
    }

    pub fn unprotect(data: &[u8]) -> anyhow::Result<Vec<u8>> {
        let input = blob(data);
        let entropy = blob(super::ENTROPY);
        let mut output = CRYPT_INTEGER_BLOB::default();
        unsafe {
            CryptUnprotectData(
                &input,
                None,
                Some(&entropy),
                None,
                None,
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
            .map_err(|e| anyhow::anyhow!("CryptUnprotectData: {e}"))?;
            let out = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
            let _ = LocalFree(Some(windows::Win32::Foundation::HLOCAL(
                output.pbData as *mut core::ffi::c_void,
            )));
            Ok(out)
        }
    }
}

// ---------------------------------------------------------------------------
// Dateisystem & Sync-Richtlinien
// ---------------------------------------------------------------------------

/// Hidden-Attribut direkt per Win32 setzen — kein `attrib`-Kindprozess, der im
/// GUI-Subsystem ein Konsolenfenster aufblitzen ließe. Bestehende Attribute
/// bleiben erhalten (Semantik von `attrib +H`).
pub fn hide_directory(path: &std::path::Path) -> anyhow::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        GetFileAttributesW, SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN, FILE_FLAGS_AND_ATTRIBUTES,
        INVALID_FILE_ATTRIBUTES,
    };

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let path = PCWSTR(wide.as_ptr());
    let attrs = match unsafe { GetFileAttributesW(path) } {
        INVALID_FILE_ATTRIBUTES => FILE_ATTRIBUTE_HIDDEN,
        attrs => FILE_FLAGS_AND_ATTRIBUTES(attrs) | FILE_ATTRIBUTE_HIDDEN,
    };
    unsafe { SetFileAttributesW(path, attrs) }.map_err(|e| anyhow::anyhow!("{e}"))
}

/// Prüft die aktuellen Windows-Richtlinien direkt vor Hintergrundtransfers.
/// Nicht verfügbare Systeminformationen blockieren den Sync nicht.
pub fn block_reason(settings: &SyncSettings) -> Option<BlockReason> {
    use windows::Networking::Connectivity::NetworkInformation;
    use windows::System::Power::{EnergySaverStatus, PowerManager};

    if !settings.allow_energy_saver
        && matches!(PowerManager::EnergySaverStatus(), Ok(EnergySaverStatus::On))
    {
        return Some(BlockReason::EnergySaver);
    }

    let Ok(profile) = NetworkInformation::GetInternetConnectionProfile() else {
        return None;
    };
    if !settings.allow_mobile_data && profile.IsWwanConnectionProfile().unwrap_or(false) {
        return Some(BlockReason::MobileData);
    }

    if !settings.allow_data_saver {
        if let Ok(cost) = profile.GetConnectionCost() {
            if cost.BackgroundDataUsageRestricted().unwrap_or(false)
                || cost.OverDataLimit().unwrap_or(false)
            {
                return Some(BlockReason::DataSaver);
            }
        }
    }

    None
}
