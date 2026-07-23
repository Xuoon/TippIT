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

pub fn configure_app(_app: &mut tauri::App) {}

/// Windows zeigt das Einstellungsfenster regulär mit EXE-Icon in der
/// Alt-Tab-Liste — nichts umzuschalten (macOS: ActivationPolicy-Wechsel).
pub fn set_app_switcher_visible(_app: &tauri::AppHandle, _visible: bool) {}

pub fn default_hotkeys() -> (&'static str, &'static str) {
    ("ctrl+y", "ctrl+shift+y")
}

/// PARITÄT: Token müssen deckungsgleich mit `formatHotkey` in
/// `src/lib/platform.ts` bleiben.
pub fn display_hotkey(value: &str) -> String {
    value
        .to_uppercase()
        .replace("CTRL", "STRG")
        .replace('+', " + ")
}

/// Windows registriert Hotkeys über virtuelle Keys — bereits layoutbewusst
/// (Pendant zur UCKeyTranslate-Übersetzung in mac.rs, hier Identität).
pub fn resolve_hotkey(value: &str) -> String {
    value.to_owned()
}

/// DPAPI übernimmt unter Windows den Schutz; zusätzliche Unix-Rechte entfallen.
pub fn secure_key_file(_path: &std::path::Path) -> anyhow::Result<()> {
    Ok(())
}

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

/// Best-effort Vordergrund-App für Source-Meta. Nie panic.
/// `None` = transient/unbekannt; Self mit `is_self: true`.
pub fn foreground_app_info() -> Option<super::ForegroundApp> {
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return None;
    }
    let mut pid: u32 = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()? };
    let mut buf = [0u16; 512];
    let mut size = buf.len() as u32;
    let path = unsafe {
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);
        if ok.is_err() || size == 0 {
            return None;
        }
        let os = std::ffi::OsString::from_wide(&buf[..size as usize]);
        os.to_string_lossy().replace('/', "\\")
    };
    let path_lower = path.to_ascii_lowercase();
    let self_path = std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().replace('/', "\\").to_ascii_lowercase());
    let is_self = self_path.as_ref().is_some_and(|s| s == &path_lower)
        || path_lower.ends_with("\\tippit.exe");
    let name = file_description(&path)
        .or_else(|| {
            std::path::Path::new(&path)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "Unbekannt".into());
    let icon_png = if is_self { None } else { exe_icon_png(&path) };
    Some(super::ForegroundApp {
        id: path_lower,
        name,
        icon_png,
        is_self,
    })
}

/// Shell-Icon der EXE als 32×32-PNG (Pendant zu `ns_image_to_png32` in mac.rs).
fn exe_icon_png(path: &str) -> Option<Vec<u8>> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
    use windows::Win32::UI::WindowsAndMessaging::DestroyIcon;

    // SHGetFileInfoW verlangt initialisiertes COM; Mehrfach-Init auf demselben
    // Thread ist harmlos (S_FALSE) und wird bewusst nicht wieder abgebaut.
    let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    let wide: Vec<u16> = std::ffi::OsStr::new(path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut info = SHFILEINFOW::default();
    let ok = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        )
    };
    if ok == 0 || info.hIcon.is_invalid() {
        return None;
    }
    let png = icon_to_png32(info.hIcon);
    unsafe {
        let _ = DestroyIcon(info.hIcon);
    }
    png
}

/// HICON → 32×32-RGBA-PNG (Farb-Bitmap als 32-bpp-DIB auslesen).
fn icon_to_png32(hicon: windows::Win32::UI::WindowsAndMessaging::HICON) -> Option<Vec<u8>> {
    use windows::Win32::Graphics::Gdi::{
        DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO,
        BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetIconInfo, ICONINFO};

    let mut info = ICONINFO::default();
    unsafe { GetIconInfo(hicon, &mut info) }.ok()?;
    let color = info.hbmColor;
    let mask = info.hbmMask;
    let result = (|| {
        if color.is_invalid() {
            return None; // monochromes Icon — generischer Fallback reicht
        }
        let mut bm = BITMAP::default();
        let got = unsafe {
            GetObjectW(
                color.into(),
                std::mem::size_of::<BITMAP>() as i32,
                Some(&mut bm as *mut _ as *mut core::ffi::c_void),
            )
        };
        if got == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 {
            return None;
        }
        let (w, h) = (bm.bmWidth, bm.bmHeight);
        let mut bi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                // Negativ = top-down, passend zur Pixelreihenfolge von image.
                biHeight: -h,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
        let hdc = unsafe { GetDC(None) };
        let lines = unsafe {
            GetDIBits(
                hdc,
                color,
                0,
                h as u32,
                Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
                &mut bi,
                DIB_RGB_COLORS,
            )
        };
        unsafe { ReleaseDC(None, hdc) };
        if lines == 0 {
            return None;
        }
        // BGRA → RGBA; Icons ohne Alphakanal (Alt-Format) opak stellen.
        let mut has_alpha = false;
        for px in buf.chunks_exact_mut(4) {
            px.swap(0, 2);
            has_alpha |= px[3] != 0;
        }
        if !has_alpha {
            for px in buf.chunks_exact_mut(4) {
                px[3] = 255;
            }
        }
        let img = image::RgbaImage::from_raw(w as u32, h as u32, buf)?;
        let resized = image::DynamicImage::ImageRgba8(img).resize_exact(
            32,
            32,
            image::imageops::FilterType::Triangle,
        );
        let mut out = std::io::Cursor::new(Vec::new());
        resized.write_to(&mut out, image::ImageFormat::Png).ok()?;
        Some(out.into_inner())
    })();
    unsafe {
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mask.into());
    }
    result
}

/// OCR über die Windows-Engine (WinRT `Windows.Media.Ocr`). Die Erkennung nutzt
/// die installierten Sprachpakete; Zeilen-Boxen entstehen als Vereinigung der
/// Wort-Rechtecke (die API liefert nur Wort-Koordinaten), normalisiert mit
/// Ursprung oben-links — Parität zu mac.rs::ocr_png.
pub fn ocr_png(png: &[u8]) -> anyhow::Result<Vec<super::OcrLine>> {
    use windows::Graphics::Imaging::BitmapDecoder;
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};

    let engine = OcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| {
        anyhow::anyhow!("Kein OCR-Sprachpaket installiert (Windows-Einstellungen → Sprache)")
    })?;

    // Die Engine deckelt die Bildkante — größere Bilder vorab herunterskalieren
    // (Koordinaten bleiben korrekt, sie werden ohnehin normalisiert).
    let max = OcrEngine::MaxImageDimension().unwrap_or(2600);
    let decoded = image::load_from_memory(png)?;
    let png_owned;
    let png_bytes: &[u8] = if decoded.width().max(decoded.height()) > max {
        let scaled = decoded.resize(max, max, image::imageops::FilterType::Triangle);
        let mut out = std::io::Cursor::new(Vec::new());
        scaled.write_to(&mut out, image::ImageFormat::Png)?;
        png_owned = out.into_inner();
        &png_owned
    } else {
        png
    };

    let stream = InMemoryRandomAccessStream::new()?;
    let writer = DataWriter::CreateDataWriter(&stream)?;
    writer.WriteBytes(png_bytes)?;
    writer.StoreAsync()?.join()?;
    writer.FlushAsync()?.join()?;
    writer.DetachStream()?;
    stream.Seek(0)?;

    let decoder = BitmapDecoder::CreateAsync(&stream)?.join()?;
    let bitmap = decoder.GetSoftwareBitmapAsync()?.join()?;
    let (bw, bh) = (
        f64::from(bitmap.PixelWidth()?),
        f64::from(bitmap.PixelHeight()?),
    );
    if bw <= 0.0 || bh <= 0.0 {
        return Ok(Vec::new());
    }

    let result = engine.RecognizeAsync(&bitmap)?.join()?;
    let mut lines = Vec::new();
    for line in result.Lines()? {
        let text = line.Text()?.to_string();
        if text.trim().is_empty() {
            continue;
        }
        // Zeilen-Box = Vereinigung der Wort-Boxen.
        let mut min_x = f64::MAX;
        let mut min_y = f64::MAX;
        let mut max_x = f64::MIN;
        let mut max_y = f64::MIN;
        for word in line.Words()? {
            let rect = word.BoundingRect()?;
            min_x = min_x.min(f64::from(rect.X));
            min_y = min_y.min(f64::from(rect.Y));
            max_x = max_x.max(f64::from(rect.X + rect.Width));
            max_y = max_y.max(f64::from(rect.Y + rect.Height));
        }
        if min_x >= max_x || min_y >= max_y {
            continue;
        }
        lines.push(super::OcrLine {
            text,
            x: min_x / bw,
            y: min_y / bh,
            w: (max_x - min_x) / bw,
            h: (max_y - min_y) / bh,
        });
    }
    Ok(lines)
}

/// Datei-Pfad oder URL im Standard-Handler öffnen (`ShellExecuteW`). Die
/// Scheme-/Typ-Prüfung macht der Aufrufer (`history::open_entry`).
pub fn open_external(target: &str) -> anyhow::Result<()> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let file = HSTRING::from(target);
    let verb = HSTRING::from("open");
    // ShellExecuteW liefert bei Erfolg ein HINSTANCE > 32.
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(file.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
    if result.0 as isize > 32 {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "ShellExecuteW fehlgeschlagen ({})",
            result.0 as isize
        ))
    }
}

/// FileDescription aus der VERSIONINFO der EXE, falls vorhanden.
fn file_description(path: &str) -> Option<String> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };

    let wide: Vec<u16> = std::ffi::OsStr::new(path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let size = unsafe { GetFileVersionInfoSizeW(PCWSTR(wide.as_ptr()), None) };
    if size == 0 {
        return None;
    }
    let mut data = vec![0u8; size as usize];
    if unsafe {
        GetFileVersionInfoW(
            PCWSTR(wide.as_ptr()),
            Some(0),
            size,
            data.as_mut_ptr() as *mut _,
        )
    }
    .is_err()
    {
        return None;
    }
    // Translation → StringFileInfo\<lang><codepage>\FileDescription
    let mut trans_ptr: *mut core::ffi::c_void = std::ptr::null_mut();
    let mut trans_len: u32 = 0;
    let query = w!("\\VarFileInfo\\Translation");
    let ok = unsafe {
        VerQueryValueW(
            data.as_ptr() as *const _,
            query,
            &mut trans_ptr,
            &mut trans_len,
        )
    }
    .as_bool();
    if !ok || trans_len < 4 || trans_ptr.is_null() {
        return None;
    }
    let lang = unsafe { *(trans_ptr as *const u16) };
    let codepage = unsafe { *((trans_ptr as *const u16).add(1)) };
    let key = format!("\\StringFileInfo\\{lang:04x}{codepage:04x}\\FileDescription");
    let key_wide: Vec<u16> = key.encode_utf16().chain(std::iter::once(0)).collect();
    let mut val_ptr: *mut core::ffi::c_void = std::ptr::null_mut();
    let mut val_len: u32 = 0;
    let ok = unsafe {
        VerQueryValueW(
            data.as_ptr() as *const _,
            PCWSTR(key_wide.as_ptr()),
            &mut val_ptr,
            &mut val_len,
        )
    }
    .as_bool();
    if !ok || val_len == 0 || val_ptr.is_null() {
        return None;
    }
    let slice = unsafe { std::slice::from_raw_parts(val_ptr as *const u16, val_len as usize) };
    let s = String::from_utf16_lossy(slice)
        .trim_end_matches('\0')
        .trim()
        .to_string();
    (!s.is_empty()).then_some(s)
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

/// Windows: Corner-Radius läuft über CSS + transparent; kein natives Pendant nötig.
pub fn round_window_corners(_window: &tauri::WebviewWindow, _radius: f64) {}

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
