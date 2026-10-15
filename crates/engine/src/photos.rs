//! Photo callouts: pictures pinned to a drawing (a server rack, a ceiling, a panel), opened by
//! tapping the pin. A photo is a file attachment annotation (§12.5.6.15) with the push-pin icon
//! holding a JPEG, so other viewers open it too; `/PCPhoto` marks it as a photo and `/Contents`
//! is its caption.
//!
//! Phone photos are large (12 MP and more, several MB each) and stored sideways with an EXIF
//! orientation; a plan with dozens of them would become unwieldy. Photos are turned upright and
//! scaled to at most [`MAX_SIDE`] pixels on their long side before they are pinned.

use std::io::Cursor;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::{DynamicImage, ImageDecoder, ImageReader, Limits};

/// The long side of a pinned photo, in pixels: sharp on a tablet, small in the file.
pub const MAX_SIDE: u32 = 2048;
const QUALITY: u8 = 85;
/// Photos larger than this are refused before decoding (a 200 MP panorama is 800 MB of pixels).
const MAX_PIXELS: u64 = 120_000_000;
const MAX_FILE: usize = 200 << 20;

fn decode(bytes: &[u8]) -> Result<DynamicImage, String> {
    if bytes.len() > MAX_FILE {
        return Err("the photo is too large (over 200 MB)".into());
    }
    let reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format().map_err(|e| e.to_string())?;
    if reader.format().is_none() {
        return Err("that file isn't a photo RevuCraft can read (JPEG, PNG or TIFF)".into());
    }
    let mut decoder = reader.into_decoder().map_err(|e| format!("the photo can't be read ({e})"))?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err("the photo is too large to pin".into());
    }
    let mut limits = Limits::default();
    limits.max_alloc = Some(MAX_PIXELS.saturating_mul(4));
    decoder.set_limits(limits).map_err(|e| e.to_string())?;
    let orientation = decoder.orientation().ok();
    let mut image = DynamicImage::from_decoder(decoder).map_err(|e| format!("the photo can't be read ({e})"))?;
    if let Some(o) = orientation {
        image.apply_orientation(o);
    }
    Ok(image)
}

fn fit(image: DynamicImage, side: u32) -> DynamicImage {
    if image.width().max(image.height()) > side { image.resize(side, side, FilterType::Triangle) } else { image }
}

/// A photo ready to pin: upright, at most [`MAX_SIDE`] pixels on its long side, as a JPEG.
pub fn prepare(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let image = fit(decode(bytes)?, MAX_SIDE);
    let rgb = image.to_rgb8();
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, QUALITY).encode_image(&rgb).map_err(|e| format!("the photo can't be saved ({e})"))?;
    Ok(out)
}

/// A pinned photo's pixels for showing it: width, height and RGBA, at most `side` pixels on its
/// long side.
pub fn decode_rgba(bytes: &[u8], side: u32) -> Result<(u32, u32, Vec<u8>), String> {
    let image = fit(decode(bytes)?, side.clamp(16, 8192));
    let rgba = image.to_rgba8();
    Ok((rgba.width(), rgba.height(), rgba.into_raw()))
}

/// A file name for a photo pinned now: `photo-<n>.jpg`.
pub fn file_name(n: usize) -> String {
    format!("photo-{n}.jpg")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbImage::from_fn(w, h, |x, _| image::Rgb([(x % 256) as u8, 80, 160]));
        let mut out = Vec::new();
        DynamicImage::ImageRgb8(img).write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    #[test]
    fn large_photos_are_scaled_to_a_jpeg() {
        let jpeg = prepare(&png(4000, 1000)).unwrap();
        assert!(jpeg.starts_with(&[0xFF, 0xD8]));
        let (w, h, px) = decode_rgba(&jpeg, 4096).unwrap();
        assert_eq!((w, h), (MAX_SIDE, 512));
        assert_eq!(px.len(), (w * h * 4) as usize);
        let (w, h, _) = decode_rgba(&jpeg, 256).unwrap();
        assert_eq!((w, h), (256, 64));
    }

    #[test]
    fn small_photos_keep_their_size_and_junk_is_refused() {
        let (w, h, _) = decode_rgba(&prepare(&png(300, 200)).unwrap(), 2048).unwrap();
        assert_eq!((w, h), (300, 200));
        assert!(prepare(b"not a photo").is_err());
        assert!(prepare(&[0xFF, 0xD8, 0xFF, 0xE0, 0, 0]).is_err());
        assert!(prepare(&[]).is_err());
    }
}
