//! Core image resize/compression logic, callable natively (for `cargo test`)
//! and exposed to JavaScript via `wasm-bindgen` when compiled to wasm32.
//!
//! Design: all real logic lives in plain functions that operate on
//! `image::DynamicImage` and `Vec<u8>` / `Result<_, String>`. The
//! `#[wasm_bindgen]` functions at the bottom are thin adapters that convert
//! `String` errors into `JsValue`. This lets `cargo test` exercise the exact
//! same code path used from the browser, on the host target, with no wasm
//! runtime involved.

use image::{DynamicImage, ImageFormat};
use std::io::Cursor;
use wasm_bindgen::prelude::*;

/// Decode raw image bytes (PNG or JPEG) into a `DynamicImage`.
pub fn decode_image(bytes: &[u8]) -> Result<DynamicImage, String> {
    image::load_from_memory(bytes).map_err(|e| format!("failed to decode image: {e}"))
}

/// Resize `img` so it fits within `max_width` x `max_height`, preserving
/// aspect ratio. If the image is already within bounds, it is returned
/// unchanged (we never upscale — that would only inflate file size).
pub fn resize_to_fit(img: &DynamicImage, max_width: u32, max_height: u32) -> DynamicImage {
    let (w, h) = (img.width(), img.height());
    if max_width == 0 || max_height == 0 || (w <= max_width && h <= max_height) {
        return img.clone();
    }

    let width_ratio = max_width as f64 / w as f64;
    let height_ratio = max_height as f64 / h as f64;
    let ratio = width_ratio.min(height_ratio);

    let new_w = ((w as f64 * ratio).round() as u32).max(1);
    let new_h = ((h as f64 * ratio).round() as u32).max(1);

    // Triangle (bilinear-family) filter: good quality/speed tradeoff and,
    // unlike Lanczos3, cheap enough to be comfortable running in a browser
    // tab on modest hardware.
    img.resize(new_w, new_h, image::imageops::FilterType::Triangle)
}

/// Encode `img` as a JPEG at the given quality (1-100).
pub fn encode_jpeg(img: &DynamicImage, quality: u8) -> Result<Vec<u8>, String> {
    let quality = quality.clamp(1, 100);
    let mut out = Vec::new();
    let rgb = img.to_rgb8(); // JPEG has no alpha channel
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    rgb.write_with_encoder(encoder)
        .map_err(|e| format!("failed to encode jpeg: {e}"))?;
    Ok(out)
}

/// Encode `img` as PNG (lossless; "compression" here comes from the resize
/// step and PNG's own deflate, not a quality knob).
pub fn encode_png(img: &DynamicImage) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), ImageFormat::Png)
        .map_err(|e| format!("failed to encode png: {e}"))?;
    Ok(out)
}

/// Full pipeline: decode -> resize (fit within bounds) -> re-encode.
/// `output_format` is `"jpeg"` or `"png"` (anything else defaults to jpeg).
pub fn process_image(
    bytes: &[u8],
    max_width: u32,
    max_height: u32,
    quality: u8,
    output_format: &str,
) -> Result<Vec<u8>, String> {
    let img = decode_image(bytes)?;
    let resized = resize_to_fit(&img, max_width, max_height);
    match output_format {
        "png" => encode_png(&resized),
        _ => encode_jpeg(&resized, quality),
    }
}

// ---------------------------------------------------------------------
// wasm-bindgen bindings (only meaningful when compiled for wasm32; the
// functions above are what `cargo test` actually exercises natively).
// ---------------------------------------------------------------------

/// Call once from JS at startup so Rust panics show up as readable
/// messages in the browser console instead of an opaque "unreachable".
#[wasm_bindgen]
pub fn init_panic_hook() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();
}

/// JS-facing entry point. Takes the raw bytes of an uploaded PNG/JPEG file
/// and returns the resized + re-encoded bytes, or throws a JS error.
#[wasm_bindgen]
pub fn compress_image(
    bytes: &[u8],
    max_width: u32,
    max_height: u32,
    quality: u8,
    output_format: &str,
) -> Result<Vec<u8>, JsValue> {
    process_image(bytes, max_width, max_height, quality, output_format)
        .map_err(|e| JsValue::from_str(&e))
}

/// Convenience getter so JS can report original dimensions without a
/// second decode pass on its side.
#[wasm_bindgen]
pub fn get_dimensions(bytes: &[u8]) -> Result<Vec<u32>, JsValue> {
    let img = decode_image(bytes).map_err(|e| JsValue::from_str(&e))?;
    Ok(vec![img.width(), img.height()])
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    /// Build a small in-memory PNG (deterministic gradient) so tests don't
    /// depend on any file on disk.
    fn make_test_png(width: u32, height: u32) -> Vec<u8> {
        let mut img = RgbaImage::new(width, height);
        for (x, y, px) in img.enumerate_pixels_mut() {
            *px = Rgba([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255]);
        }
        let dynamic = DynamicImage::ImageRgba8(img);
        encode_png(&dynamic).expect("test PNG should encode")
    }

    #[test]
    fn resize_shrinks_oversized_image() {
        let png = make_test_png(400, 300);
        let img = decode_image(&png).unwrap();
        let resized = resize_to_fit(&img, 100, 100);
        assert!(resized.width() <= 100);
        assert!(resized.height() <= 100);
    }

    #[test]
    fn resize_preserves_aspect_ratio() {
        let png = make_test_png(200, 100); // 2:1 aspect ratio
        let img = decode_image(&png).unwrap();
        let resized = resize_to_fit(&img, 100, 100);
        // Width is the binding constraint (2:1 ratio, square box) -> 100x50.
        assert_eq!(resized.width(), 100);
        assert_eq!(resized.height(), 50);
    }

    #[test]
    fn resize_does_not_upscale_smaller_images() {
        let png = make_test_png(50, 50);
        let img = decode_image(&png).unwrap();
        let resized = resize_to_fit(&img, 200, 200);
        assert_eq!(resized.width(), 50);
        assert_eq!(resized.height(), 50);
    }

    #[test]
    fn lower_jpeg_quality_produces_smaller_or_equal_output() {
        let png = make_test_png(256, 256);
        let img = decode_image(&png).unwrap();
        let high = encode_jpeg(&img, 95).unwrap();
        let low = encode_jpeg(&img, 10).unwrap();
        assert!(
            low.len() <= high.len(),
            "low quality ({} bytes) should not be larger than high quality ({} bytes)",
            low.len(),
            high.len()
        );
    }

    #[test]
    fn process_image_end_to_end_png_to_jpeg() {
        let png = make_test_png(400, 400);
        let out = process_image(&png, 100, 100, 80, "jpeg").expect("pipeline should succeed");
        assert!(!out.is_empty());
        // Output must itself be a valid, decodable JPEG at the target size.
        let decoded = image::load_from_memory(&out).expect("output should decode");
        assert_eq!(decoded.width(), 100);
        assert_eq!(decoded.height(), 100);
    }

    #[test]
    fn process_image_rejects_garbage_bytes() {
        let garbage = vec![0u8, 1, 2, 3, 4, 5];
        let result = process_image(&garbage, 100, 100, 80, "jpeg");
        assert!(result.is_err());
    }

    #[test]
    fn get_dimensions_matches_source_image() {
        let png = make_test_png(64, 48);
        let img = decode_image(&png).unwrap();
        assert_eq!((img.width(), img.height()), (64, 48));
    }
}
