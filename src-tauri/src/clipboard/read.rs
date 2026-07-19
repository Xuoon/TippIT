use std::io::Cursor;

use image::codecs::png::PngEncoder;
use image::{ImageEncoder, RgbaImage};

const THUMB_MAX: u32 = 256;

pub enum ClipContent {
    Text(String),
    Image { png: Vec<u8>, thumb_png: Vec<u8> },
    Files(Vec<String>),
}

/// Zwischenablage lesen. Priorität: Dateien > Text > Bild
/// (kopierte Dateien haben oft auch Text-Repräsentationen).
pub fn read_clipboard(capture_images: bool, capture_files: bool) -> Option<ClipContent> {
    if capture_files {
        if let Ok(files) =
            clipboard_win::get_clipboard::<Vec<String>, _>(clipboard_win::formats::FileList)
        {
            if !files.is_empty() {
                return Some(ClipContent::Files(files));
            }
        }
    }

    let mut cb = arboard::Clipboard::new().ok()?;
    if let Ok(text) = cb.get_text() {
        if !text.trim().is_empty() {
            return Some(ClipContent::Text(text));
        }
    }
    if capture_images {
        if let Ok(img) = cb.get_image() {
            return encode_image(img);
        }
    }
    None
}

fn encode_image(img: arboard::ImageData<'_>) -> Option<ClipContent> {
    let (w, h) = (img.width as u32, img.height as u32);
    let rgba = RgbaImage::from_raw(w, h, img.bytes.into_owned())?;

    let mut png = Vec::new();
    PngEncoder::new(Cursor::new(&mut png))
        .write_image(rgba.as_raw(), w, h, image::ExtendedColorType::Rgba8)
        .ok()?;

    let scale = (THUMB_MAX as f64 / w.max(h) as f64).min(1.0);
    let (tw, th) = (
        ((w as f64 * scale) as u32).max(1),
        ((h as f64 * scale) as u32).max(1),
    );
    let thumb = image::imageops::thumbnail(&rgba, tw, th);
    let mut thumb_png = Vec::new();
    PngEncoder::new(Cursor::new(&mut thumb_png))
        .write_image(thumb.as_raw(), tw, th, image::ExtendedColorType::Rgba8)
        .ok()?;

    Some(ClipContent::Image { png, thumb_png })
}

/// Nach jedem erfolgreichen EIGENEN Clipboard-Write aufrufen: merkt sich die
/// Sequenznummer, damit der Monitor genau diesen Write nicht als Kopie erfasst.
pub fn mark_own_write(state: &crate::state::AppState) {
    let seq = unsafe { windows::Win32::System::DataExchange::GetClipboardSequenceNumber() };
    state
        .own_clip_seq
        .store(seq, std::sync::atomic::Ordering::SeqCst);
}

/// PNG-Bytes zurück in die Zwischenablage legen (für „Kopieren" aus der Historie).
pub fn write_image_to_clipboard(png: &[u8]) -> anyhow::Result<()> {
    let decoded = image::load_from_memory_with_format(png, image::ImageFormat::Png)?.to_rgba8();
    let (w, h) = decoded.dimensions();
    let mut cb = arboard::Clipboard::new()?;
    cb.set_image(arboard::ImageData {
        width: w as usize,
        height: h as usize,
        bytes: decoded.into_raw().into(),
    })?;
    Ok(())
}
