//! Shared types for the Windows screen-watch submodule.
//!
//! Kept deliberately small and pure (no Win32 imports here) so they can be
//! consumed from any thread or from non-Windows tests with `cfg(test)`.

use serde::{Deserialize, Serialize};

/// A captured frame from Windows Graphics Capture (WGC).
///
/// `pixels` is BGRA8 row-major, length = `4 * width * height`. Owned so the
/// capture thread can hand off to the OCR pool without sharing the WGC
/// callback's internal D3D buffer.
#[derive(Debug, Clone)]
pub struct Frame {
    pub captured_at_ms: i64,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Frame {
    /// Stride in bytes (BGRA8 = 4 bytes per pixel).
    pub fn stride(&self) -> usize {
        (self.width as usize) * 4
    }

    /// `true` when `pixels.len()` matches `4 * width * height`. A mismatch
    /// means the capture handed us a torn frame; callers should drop it.
    pub fn is_well_formed(&self) -> bool {
        self.pixels.len() == self.stride() * (self.height as usize)
    }
}

/// Information about a Windows top-level window discovered via `EnumWindows`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowInfo {
    /// Raw HWND value (cast to u64 so the type is Send + serializable).
    pub hwnd_raw: u64,
    pub title: String,
    pub class_name: String,
    /// Process executable path (best-effort). May be empty if access is denied.
    pub exe_path: String,
    /// Client-area rect in screen coordinates.
    pub client_left: i32,
    pub client_top: i32,
    pub client_width: u32,
    pub client_height: u32,
}

/// Reason a frame ran through the OCR pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OcrReason {
    /// Periodic 2 Hz sample tick.
    Periodic,
    /// Forced manual snapshot from the orchestrator.
    Manual,
    /// Layout drift suspected — re-run to confirm.
    DriftRecheck,
}

/// Outcome of a single OCR pass over one ROI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrResult {
    pub roi_name: String,
    pub text: String,
    /// `0.0..=1.0`. Tesseract gives 0..100; we normalise.
    pub confidence: f32,
    /// True when confidence is below the vision-LLM fallback threshold (0.7).
    pub needs_vision_fallback: bool,
}

/// High-level events emitted by the Windows screen-watch engine over a
/// `tokio::sync::broadcast` channel. The Whiskey-trader subscriber (see
/// `WHISKEY.md` §12) consumes these to cross-reference against the A+
/// catalog and emit overlay attention messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TradingEvent {
    PositionOpened {
        symbol: String,
        side: String,
        size: f64,
        entry: f64,
        at_ms: i64,
    },
    PositionClosed {
        symbol: String,
        size: f64,
        exit: f64,
        realized_pnl: Option<f64>,
        at_ms: i64,
    },
    PnLUpdated {
        symbol: Option<String>,
        unrealized_pnl: Option<f64>,
        realized_pnl_session: Option<f64>,
        at_ms: i64,
    },
    QuoteUpdated {
        symbol: String,
        last: Option<f64>,
        bid: Option<f64>,
        ask: Option<f64>,
        at_ms: i64,
    },
    /// A possible setup the parser flagged for human review. The Whiskey
    /// subscriber matches `tags` against playbook keywords.
    SetupCandidate {
        symbol: String,
        tags: Vec<String>,
        notes: String,
        at_ms: i64,
    },
    /// Layout drift detected on `roi_name` — UI layout shifted enough that
    /// previously-anchored ROIs may no longer be aligned. One-shot.
    LayoutDrift {
        roi_name: String,
        distance: u32,
        at_ms: i64,
    },
    /// OCR confidence below threshold; vision-LLM fallback is requested but
    /// not yet wired (see TODO in `engine.rs`).
    VisionFallbackNeeded {
        roi_name: String,
        confidence: f32,
        at_ms: i64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_well_formed_matches_stride_times_height() {
        let f = Frame {
            captured_at_ms: 0,
            width: 10,
            height: 4,
            pixels: vec![0u8; 4 * 10 * 4],
        };
        assert!(f.is_well_formed());
        assert_eq!(f.stride(), 40);

        let torn = Frame {
            captured_at_ms: 0,
            width: 10,
            height: 4,
            pixels: vec![0u8; 4 * 10 * 3], // missing one row
        };
        assert!(!torn.is_well_formed());
    }

    #[test]
    fn ocr_result_serializes_round_trip() {
        let r = OcrResult {
            roi_name: "pnl".into(),
            text: "+$123.45".into(),
            confidence: 0.92,
            needs_vision_fallback: false,
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: OcrResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.roi_name, "pnl");
        assert_eq!(back.text, "+$123.45");
    }
}
