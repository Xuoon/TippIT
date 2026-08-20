use std::io::Cursor;

use image::codecs::png::PngEncoder;
use image::{ImageEncoder, RgbaImage};

const THUMB_MAX: u32 = 256;

pub enum ClipContent {
    /// `html` ist bereits sanitisiert (`super::html::sanitize`) und nur gesetzt,
    /// wenn die Quelle echte Formatierung geliefert hat.
    Text {
        text: String,
        html: Option<String>,
    },
    Image {
        png: Vec<u8>,
        thumb_png: Vec<u8>,
    },
    Files(Vec<String>),
}

/// Zwischenablage lesen. Priorität: Dateien > Text > Bild
/// (kopierte Dateien haben oft auch Text-Repräsentationen).
pub fn read_clipboard(
    capture_images: bool,
    capture_files: bool,
    capture_html: bool,
) -> Option<ClipContent> {
    if capture_files {
        if let Some(files) = crate::platform::clipboard_file_list() {
            return Some(ClipContent::Files(files));
        }
    }

    let mut cb = arboard::Clipboard::new().ok()?;
    if let Ok(text) = cb.get_text() {
        if !text.trim().is_empty() {
            let html = capture_html
                .then(crate::platform::clipboard_html)
                .flatten()
                .and_then(|raw| super::html::sanitize(&raw));
            return Some(ClipContent::Text { text, html });
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

    let thumb_png = thumbnail_rgba(&rgba)?;
    Some(ClipContent::Image { png, thumb_png })
}

/// Vorschaubild (max. 256 px Kantenlänge) aus einem PNG erzeugen — beim Import
/// liegt nur das fertige PNG vor, kein Clipboard-Bitmap.
pub fn thumbnail_png(png: &[u8]) -> Option<Vec<u8>> {
    let rgba = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .ok()?
        .to_rgba8();
    thumbnail_rgba(&rgba)
}

fn thumbnail_rgba(rgba: &RgbaImage) -> Option<Vec<u8>> {
    let (w, h) = rgba.dimensions();
    let scale = (THUMB_MAX as f64 / w.max(h) as f64).min(1.0);
    let (tw, th) = (
        ((w as f64 * scale) as u32).max(1),
        ((h as f64 * scale) as u32).max(1),
    );
    let thumb = image::imageops::thumbnail(rgba, tw, th);
    let mut out = Vec::new();
    PngEncoder::new(Cursor::new(&mut out))
        .write_image(thumb.as_raw(), tw, th, image::ExtendedColorType::Rgba8)
        .ok()?;
    Some(out)
}

/// Nach jedem erfolgreichen EIGENEN Clipboard-Write aufrufen: merkt sich die
/// Sequenznummer, damit der Monitor genau diesen Write nicht als Kopie erfasst.
pub fn mark_own_write(state: &crate::state::AppState) {
    state.own_clip_seq.store(
        crate::platform::clipboard_seq(),
        std::sync::atomic::Ordering::SeqCst,
    );
}

/// Text mit Formatierung in die Zwischenablage legen (Klartext immer mit dabei).
pub fn write_html_to_clipboard(html: &str, text: &str) -> anyhow::Result<()> {
    crate::platform::clipboard_set_html(html, text)
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
