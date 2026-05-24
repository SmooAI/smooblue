//! Image preparation for compose attachments.
//!
//! Decode → optionally downscale → re-encode JPEG → return bytes +
//! dimensions. Targets Bluesky's per-blob limit (~1MB).
//!
//! We always re-encode to JPEG q=85 for simplicity: most photos already
//! are JPEG, the quality loss is imperceptible at this setting, and we
//! don't have to deal with format-specific cap juggling. PNG users who
//! want pixel-perfect transparency post elsewhere.

use anyhow::{Context, Result};
use base64::Engine;
use image::imageops::FilterType;
use image::{ImageEncoder, ImageReader};
use std::path::Path;

/// Bluesky's uploadBlob hard limit is 1,000,000 bytes. We aim for
/// ~960KB so a small amount of metadata / multipart wrapping never
/// pushes us over.
const MAX_BYTES: usize = 960 * 1024;
/// Longest-edge cap before downscaling. Bsky AppView re-thumbnails
/// anyway; sending much bigger just burns upload bandwidth.
const MAX_DIM: u32 = 2000;
const JPEG_QUALITY: u8 = 85;
const THUMB_DIM: u32 = 320;

#[derive(Clone, Debug, PartialEq)]
pub struct PreparedImage {
    /// JPEG bytes ready for uploadBlob.
    pub bytes: Vec<u8>,
    pub mime: String,
    pub width: u32,
    pub height: u32,
    /// Small JPEG (≤320px on longest edge) as a base64 data URI suitable
    /// for an `<img src>` thumbnail in the compose UI.
    pub thumb_data_uri: String,
}

/// Read + prepare an image from disk for upload.
pub fn prepare_from_path(path: &Path) -> Result<PreparedImage> {
    let reader = ImageReader::open(path)
        .with_context(|| format!("opening {}", path.display()))?
        .with_guessed_format()
        .with_context(|| format!("sniffing format of {}", path.display()))?;
    let img = reader
        .decode()
        .with_context(|| format!("decoding {}", path.display()))?;
    prepare_from_image(img)
}

/// Same as [`prepare_from_path`] but takes an already-decoded image.
pub fn prepare_from_image(img: image::DynamicImage) -> Result<PreparedImage> {
    let mut current = if img.width().max(img.height()) > MAX_DIM {
        downscale_to_max(&img, MAX_DIM)
    } else {
        img
    };

    let mut quality = JPEG_QUALITY;
    let mut bytes = encode_jpeg(&current, quality)?;

    // If still over the cap, step quality down first, then downscale 25%
    // per round once quality floors. Five passes is plenty.
    for _ in 0..5 {
        if bytes.len() <= MAX_BYTES {
            break;
        }
        if quality > 65 {
            quality = quality.saturating_sub(10);
        } else {
            let new_dim = ((current.width().max(current.height()) as f32) * 0.75) as u32;
            current = downscale_to_max(&current, new_dim.max(640));
        }
        bytes = encode_jpeg(&current, quality)?;
    }

    let thumb = downscale_to_max(&current, THUMB_DIM);
    let thumb_bytes = encode_jpeg(&thumb, 75)?;
    let thumb_data_uri = format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&thumb_bytes)
    );

    Ok(PreparedImage {
        bytes,
        mime: "image/jpeg".into(),
        width: current.width(),
        height: current.height(),
        thumb_data_uri,
    })
}

fn downscale_to_max(img: &image::DynamicImage, max_dim: u32) -> image::DynamicImage {
    let (w, h) = (img.width(), img.height());
    if w <= max_dim && h <= max_dim {
        return img.clone();
    }
    let scale = max_dim as f32 / w.max(h) as f32;
    let nw = ((w as f32) * scale).round().max(1.0) as u32;
    let nh = ((h as f32) * scale).round().max(1.0) as u32;
    img.resize(nw, nh, FilterType::Lanczos3)
}

fn encode_jpeg(img: &image::DynamicImage, quality: u8) -> Result<Vec<u8>> {
    let rgb = img.to_rgb8();
    let mut buf = Vec::with_capacity(64 * 1024);
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
    encoder
        .write_image(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .context("encoding jpeg")?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    fn synth(w: u32, h: u32) -> image::DynamicImage {
        let mut img = RgbImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                img.put_pixel(
                    x,
                    y,
                    Rgb([
                        ((x * 7) % 255) as u8,
                        ((y * 11) % 255) as u8,
                        ((x + y) % 255) as u8,
                    ]),
                );
            }
        }
        image::DynamicImage::ImageRgb8(img)
    }

    #[test]
    fn prepare_downscales_oversize_images() {
        let huge = synth(4096, 3072);
        let prepped = prepare_from_image(huge).unwrap();
        assert!(prepped.width <= MAX_DIM);
        assert!(prepped.height <= MAX_DIM);
        assert!(
            prepped.bytes.len() <= MAX_BYTES,
            "got {} bytes",
            prepped.bytes.len()
        );
        assert_eq!(prepped.mime, "image/jpeg");
        assert!(prepped
            .thumb_data_uri
            .starts_with("data:image/jpeg;base64,"));
    }

    #[test]
    fn prepare_preserves_small_images() {
        let small = synth(800, 600);
        let prepped = prepare_from_image(small).unwrap();
        assert_eq!(prepped.width, 800);
        assert_eq!(prepped.height, 600);
        assert!(prepped.bytes.len() <= MAX_BYTES);
    }
}
