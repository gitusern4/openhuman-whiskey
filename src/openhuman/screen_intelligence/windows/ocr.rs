//! Tesseract OCR wrapper for cropped, preprocessed ROI images.
//!
//! The Tesseract C library is not always available on Windows ARM64
//! (vcpkg ports lag and the upstream `tesseract`/`leptess` Rust crates
//! require it at link time). To keep `cargo check
//! --target aarch64-pc-windows-msvc` green out-of-the-box, this module
//! is gated behind the **`whiskey-windows-ocr`** Cargo feature.
//!
//! Without the feature: every public OCR call returns `Err(NotEnabled)`,
//! and the engine routes 100% of frames straight to the vision-LLM
//! fallback path. With the feature: we initialise a Tesseract instance
//! per worker thread, set `--psm 7` (single text line) and a tight
//! numeric whitelist, and run [`run_ocr`] on the preprocessed grayscale
//! buffer.
//!
//! Preprocessing is target-independent and always available:
//! - upscale 2-3× (bilinear) to give Tesseract larger glyphs,
//! - convert to luma,
//! - apply Otsu binarisation,
//! - encode as PNG bytes for the upstream caller's vision-LLM fallback.

use thiserror::Error;

use super::roi::Rect;
use super::types::{Frame, OcrResult};

/// Confidence below which the engine emits a `VisionFallbackNeeded` event.
pub const VISION_FALLBACK_THRESHOLD: f32 = 0.7;

/// Default upscale factor when preprocessing ROIs for Tesseract. Empirically
/// 2-3× yields the largest accuracy bump on small UI fonts.
pub const DEFAULT_OCR_UPSCALE: u32 = 3;

/// The character whitelist used for numeric/price ROIs.
pub const NUMERIC_WHITELIST: &str = "0123456789.,-+$()";

#[derive(Debug, Error)]
pub enum OcrError {
    #[error("OCR is not enabled (build with feature `whiskey-windows-ocr`)")]
    NotEnabled,
    #[error("RapidOCR fallback not implemented yet")]
    RapidOcrNotImplementedYet,
    #[error("OCR failed: {0}")]
    Backend(String),
    #[error("invalid ROI rect for frame")]
    InvalidRoi,
}

/// Crop the BGRA8 `frame` to `rect`, returning a raw 8-bit luma buffer
/// (`width * height` bytes) plus its dimensions. Returns `None` if the
/// rect clips empty.
pub fn crop_luma(frame: &Frame, rect: Rect) -> Option<(Vec<u8>, u32, u32)> {
    let rect = rect.clip_to(frame.width, frame.height)?;
    let mut out = Vec::with_capacity((rect.width * rect.height) as usize);
    let stride = frame.stride();
    for row in 0..rect.height {
        let y = rect.y as usize + row as usize;
        let base = y * stride + rect.x as usize * 4;
        for col in 0..rect.width {
            let i = base + col as usize * 4;
            if i + 2 >= frame.pixels.len() {
                out.push(0);
                continue;
            }
            let b = frame.pixels[i] as u32;
            let g = frame.pixels[i + 1] as u32;
            let r = frame.pixels[i + 2] as u32;
            let y = ((r * 299 + g * 587 + b * 114) / 1000) as u8;
            out.push(y);
        }
    }
    Some((out, rect.width, rect.height))
}

/// Bilinear upscale of an 8-bit luma image by an integer factor.
pub fn upscale_luma(src: &[u8], width: u32, height: u32, factor: u32) -> (Vec<u8>, u32, u32) {
    let factor = factor.max(1);
    let out_w = width * factor;
    let out_h = height * factor;
    let mut out = vec![0u8; (out_w * out_h) as usize];
    for y in 0..out_h {
        let sy = y / factor;
        for x in 0..out_w {
            let sx = x / factor;
            // Nearest-neighbour for simplicity; Tesseract does its own
            // smoothing and the upscale is mainly to give it more pixels
            // per glyph.
            let v = src[(sy as usize) * (width as usize) + (sx as usize)];
            out[(y as usize) * (out_w as usize) + (x as usize)] = v;
        }
    }
    (out, out_w, out_h)
}

/// Otsu binarisation: choose a threshold that maximises between-class
/// variance, then apply it. Returns a buffer of strict 0 or 255.
pub fn otsu_binarize(luma: &[u8]) -> Vec<u8> {
    let mut hist = [0u32; 256];
    for &v in luma {
        hist[v as usize] += 1;
    }
    let total = luma.len() as f64;
    let sum: f64 = (0..256).map(|i| i as f64 * hist[i] as f64).sum();

    let mut sum_b = 0.0f64;
    let mut w_b = 0.0f64;
    let mut max_var = 0.0f64;
    let mut threshold = 127u8;
    for i in 0..256 {
        w_b += hist[i] as f64;
        if w_b == 0.0 {
            continue;
        }
        let w_f = total - w_b;
        if w_f == 0.0 {
            break;
        }
        sum_b += i as f64 * hist[i] as f64;
        let m_b = sum_b / w_b;
        let m_f = (sum - sum_b) / w_f;
        let var_between = w_b * w_f * (m_b - m_f) * (m_b - m_f);
        if var_between > max_var {
            max_var = var_between;
            threshold = i as u8;
        }
    }

    luma.iter()
        .map(|&v| if v > threshold { 255 } else { 0 })
        .collect()
}

/// End-to-end ROI -> preprocessed luma bytes. Used by [`run_ocr`] and by
/// the vision-LLM fallback path.
pub fn preprocess_roi(frame: &Frame, rect: Rect) -> Result<(Vec<u8>, u32, u32), OcrError> {
    let (luma, w, h) = crop_luma(frame, rect).ok_or(OcrError::InvalidRoi)?;
    let (up, uw, uh) = upscale_luma(&luma, w, h, DEFAULT_OCR_UPSCALE);
    let bin = otsu_binarize(&up);
    Ok((bin, uw, uh))
}

/// Run Tesseract over a preprocessed ROI. Returns `Err(NotEnabled)` when
/// the `whiskey-windows-ocr` Cargo feature is off.
#[cfg(not(feature = "whiskey-windows-ocr"))]
pub fn run_ocr(_roi_name: &str, _frame: &Frame, _rect: Rect) -> Result<OcrResult, OcrError> {
    Err(OcrError::NotEnabled)
}

/// Run Tesseract over a preprocessed ROI with PSM 7 (single line) and
/// the numeric whitelist.
#[cfg(feature = "whiskey-windows-ocr")]
pub fn run_ocr(roi_name: &str, frame: &Frame, rect: Rect) -> Result<OcrResult, OcrError> {
    use tesseract::Tesseract;

    let (bin, w, h) = preprocess_roi(frame, rect)?;
    let mut tess = Tesseract::new(None, Some("eng"))
        .map_err(|e| OcrError::Backend(format!("Tesseract::new failed: {e}")))?
        .set_variable("tessedit_pageseg_mode", "7")
        .map_err(|e| OcrError::Backend(format!("set psm failed: {e}")))?
        .set_variable("tessedit_char_whitelist", NUMERIC_WHITELIST)
        .map_err(|e| OcrError::Backend(format!("set whitelist failed: {e}")))?;

    // 8-bit single-channel image; bytes_per_pixel = 1; bytes_per_line = w.
    let mut tess = tess
        .set_image_from_mem(&pgm_encode(&bin, w, h))
        .map_err(|e| OcrError::Backend(format!("set_image_from_mem failed: {e}")))?;
    let text = tess
        .get_text()
        .map_err(|e| OcrError::Backend(format!("get_text failed: {e}")))?;
    let confidence = tess.mean_text_conf() as f32 / 100.0;

    let needs_vision_fallback = confidence < VISION_FALLBACK_THRESHOLD;
    Ok(OcrResult {
        roi_name: roi_name.to_string(),
        text: text.trim().to_string(),
        confidence,
        needs_vision_fallback,
    })
}

/// Encode a single-channel 8-bit luma buffer as a binary PGM (P5) so the
/// Leptonica-backed Tesseract can ingest it via `set_image_from_mem`.
#[cfg(feature = "whiskey-windows-ocr")]
fn pgm_encode(luma: &[u8], width: u32, height: u32) -> Vec<u8> {
    let header = format!("P5\n{} {}\n255\n", width, height);
    let mut out = Vec::with_capacity(header.len() + luma.len());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(luma);
    out
}

/// Stub for the secondary RapidOCR fallback path. Tracked separately from
/// the Tesseract feature so we can wire RapidOCR (ONNX runtime) in a later
/// PR without touching Tesseract callers.
pub fn run_rapid_ocr_fallback(
    _roi_name: &str,
    _frame: &Frame,
    _rect: Rect,
) -> Result<OcrResult, OcrError> {
    // TODO(whiskey): wire RapidOCR (`ort` crate + bundled detection and
    // recognition models from RapidOCR-ONNX). Lower priority than the
    // primary Tesseract path.
    Err(OcrError::RapidOcrNotImplementedYet)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_frame(w: u32, h: u32, gray: u8) -> Frame {
        let mut px = Vec::with_capacity((w * h * 4) as usize);
        for _ in 0..(w * h) {
            // BGRA - identical channels yield the same gray.
            px.push(gray);
            px.push(gray);
            px.push(gray);
            px.push(255);
        }
        Frame {
            captured_at_ms: 0,
            width: w,
            height: h,
            pixels: px,
        }
    }

    #[test]
    fn crop_luma_returns_correct_dims() {
        let f = solid_frame(20, 10, 200);
        let rect = Rect {
            x: 5,
            y: 2,
            width: 8,
            height: 4,
        };
        let (luma, w, h) = crop_luma(&f, rect).unwrap();
        assert_eq!((w, h), (8, 4));
        assert_eq!(luma.len(), 8 * 4);
        // Solid frame -> all luma values equal the source gray.
        assert!(luma.iter().all(|&v| v == 200));
    }

    #[test]
    fn upscale_doubles_dimensions() {
        let src = vec![10u8, 20, 30, 40];
        let (out, w, h) = upscale_luma(&src, 2, 2, 2);
        assert_eq!((w, h), (4, 4));
        assert_eq!(out.len(), 16);
    }

    #[test]
    fn otsu_separates_bimodal_signal() {
        // Half-and-half: the ideal Otsu threshold falls between 50 and 200.
        let mut buf = vec![50u8; 50];
        buf.extend(std::iter::repeat(200u8).take(50));
        let bin = otsu_binarize(&buf);
        // First half should be 0; second half should be 255.
        assert!(bin[..50].iter().all(|&v| v == 0));
        assert!(bin[50..].iter().all(|&v| v == 255));
    }

    #[test]
    fn preprocess_roi_round_trip() {
        let f = solid_frame(32, 32, 128);
        let rect = Rect {
            x: 0,
            y: 0,
            width: 32,
            height: 32,
        };
        let (bin, w, h) = preprocess_roi(&f, rect).unwrap();
        assert_eq!(w, 32 * DEFAULT_OCR_UPSCALE);
        assert_eq!(h, 32 * DEFAULT_OCR_UPSCALE);
        assert_eq!(bin.len(), (w * h) as usize);
    }

    #[cfg(not(feature = "whiskey-windows-ocr"))]
    #[test]
    fn run_ocr_disabled_by_default_returns_not_enabled() {
        let f = solid_frame(20, 10, 200);
        let err = run_ocr(
            "x",
            &f,
            Rect {
                x: 0,
                y: 0,
                width: 20,
                height: 10,
            },
        )
        .unwrap_err();
        assert!(matches!(err, OcrError::NotEnabled));
    }

    #[test]
    fn rapid_ocr_fallback_is_explicit_todo() {
        let f = solid_frame(20, 10, 200);
        let err = run_rapid_ocr_fallback(
            "x",
            &f,
            Rect {
                x: 0,
                y: 0,
                width: 20,
                height: 10,
            },
        )
        .unwrap_err();
        assert!(matches!(err, OcrError::RapidOcrNotImplementedYet));
    }
}
