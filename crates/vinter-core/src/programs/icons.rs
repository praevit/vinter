//! Extracting a program's icon from its `.exe` as a PNG.

use std::io::Cursor;
use std::path::Path;

use crate::error::IoResultExt;
use crate::{Error, Result};

/// Icons are stored at most this many pixels wide/high.
const MAX_SIZE: u32 = 128;

/// Reads the first icon group of a PE file and returns its largest image as
/// PNG bytes (scaled down to 128 px). `None` if the file has no icon.
pub fn extract_png(exe: &Path) -> Result<Option<Vec<u8>>> {
    let bytes = std::fs::read(exe).at(exe)?;
    let Ok(pe) = pelite::PeFile::from_bytes(&bytes) else {
        return Ok(None);
    };
    let Ok(resources) = pe.resources() else {
        return Ok(None);
    };
    let Some(Ok((_, group))) = resources.icons().next() else {
        return Ok(None);
    };
    let mut ico = Vec::new();
    if group.write(&mut ico).is_err() {
        return Ok(None);
    }
    // The ICO decoder picks the largest (best) entry, including PNG-compressed ones.
    let Ok(image) = image::load_from_memory_with_format(&ico, image::ImageFormat::Ico) else {
        return Ok(None);
    };
    encode(image).map(Some)
}

/// An icon chosen by the user: a `.png`, `.jpg` or `.ico` image, or an
/// `.exe`/`.dll` to take the icon from. `None` if there is no usable image.
pub fn png_from_file(path: &Path) -> Result<Option<Vec<u8>>> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if ext == "exe" || ext == "dll" {
        return extract_png(path);
    }
    let bytes = std::fs::read(path).at(path)?;
    match image::load_from_memory(&bytes) {
        Ok(image) => encode(image).map(Some),
        Err(_) => Ok(None),
    }
}

/// A picture (e.g. a game's wide header) as a square icon: scaled to fit
/// and centred on a transparent background, so launchers don't stretch it.
pub fn square_png_from_image(path: &Path) -> Result<Option<Vec<u8>>> {
    let bytes = std::fs::read(path).at(path)?;
    let Ok(image) = image::load_from_memory(&bytes) else {
        return Ok(None);
    };
    let image = image.resize(MAX_SIZE, MAX_SIZE, image::imageops::FilterType::Lanczos3);
    let side = image.width().max(image.height());
    let mut square = image::RgbaImage::new(side, side);
    image::imageops::overlay(
        &mut square,
        &image.to_rgba8(),
        i64::from((side - image.width()) / 2),
        i64::from((side - image.height()) / 2),
    );
    encode(square.into()).map(Some)
}

/// Scales down to [`MAX_SIZE`] and encodes as PNG.
fn encode(image: image::DynamicImage) -> Result<Vec<u8>> {
    let image = if image.width() > MAX_SIZE || image.height() > MAX_SIZE {
        image.resize(MAX_SIZE, MAX_SIZE, image::imageops::FilterType::Lanczos3)
    } else {
        image
    };
    let mut png = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| Error::BadArchive(format!("could not encode icon: {e}")))?;
    Ok(png)
}

/// `data:` URL for a PNG, which the UI can show under its strict CSP.
pub fn data_url(png: &[u8]) -> String {
    use base64::Engine;
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_pe_files_have_no_icon() {
        let tmp = tempfile::tempdir().unwrap();
        let fake = tmp.path().join("fake.exe");
        std::fs::write(&fake, b"#!/bin/sh\n").unwrap();
        assert!(extract_png(&fake).unwrap().is_none());
    }

    #[test]
    fn icons_from_image_files_are_scaled_png() {
        let tmp = tempfile::tempdir().unwrap();
        let big = tmp.path().join("big.png");
        image::RgbaImage::new(512, 256).save(&big).unwrap();
        let png = png_from_file(&big).unwrap().unwrap();
        let back = image::load_from_memory(&png).unwrap();
        assert_eq!((back.width(), back.height()), (128, 64));

        let text = tmp.path().join("notes.png");
        std::fs::write(&text, "not an image").unwrap();
        assert!(png_from_file(&text).unwrap().is_none());
    }

    #[test]
    fn wide_pictures_become_square_icons() {
        let tmp = tempfile::tempdir().unwrap();
        let header = tmp.path().join("header.jpg");
        image::RgbImage::new(460, 215).save(&header).unwrap();
        let png = square_png_from_image(&header).unwrap().unwrap();
        let back = image::load_from_memory(&png).unwrap();
        assert_eq!((back.width(), back.height()), (128, 128));
    }

    #[test]
    fn data_urls() {
        assert_eq!(data_url(b"abc"), "data:image/png;base64,YWJj");
    }
}
