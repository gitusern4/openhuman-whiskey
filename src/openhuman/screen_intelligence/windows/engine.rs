//! Orchestrator for the Windows screen-watch pipeline.
//!
//! ## Status: STUB — v1 scaffolding only.
//!
//! When fully landed, this module wires together:
//!
//!   capture → bounded mpsc → 2-thread OCR pool → state diff
//!     → broadcast `TradingEvent` (consumed by the Whiskey-trader hookup
//!       per `WHISKEY.md` §12)
//!
//! Threading model (locked in by the screen-watch research, May 2026):
//! - **Capture thread** (dedicated, COM-initialised for WGC): receives
//!   frame-arrived callbacks at the monitor refresh rate; only enqueues
//!   ~2 frames/sec to the OCR queue.
//! - **OCR worker pool** (2 threads, bounded queue size 4): pulls
//!   cropped ROI tensors, runs Tesseract first-pass; on low confidence
//!   pushes a `VisionFallbackNeeded` event so the upstream caller can
//!   route the cropped PNG through `crate::openhuman::providers::router`
//!   for a vision LLM.
//! - **Async `tokio` task**: any LLM calls live here so capture is never
//!   blocked.
//! - **Overlay UI**: reads from a `Arc<RwLock<TradingState>>` snapshot
//!   only — never touches the pipeline queues.
//!
//! Why this is a stub for v1:
//! - The capture binding (`capture.rs`) is itself a stub; orchestrator
//!   has nothing real to orchestrate yet.
//! - Lands together with capture in a single PR for a coherent review.

use std::sync::Arc;

use parking_lot::RwLock;
use thiserror::Error;

use super::capture::{CaptureError, CaptureHandle};
use super::roi::Roi;
use super::state::TradingState;

/// Live engine handle. Drop to stop everything.
#[must_use = "dropping the EngineHandle stops capture, OCR pool, and broadcast"]
pub struct EngineHandle {
    _capture: CaptureHandle,
    /// Snapshot the overlay UI reads. Real impl writes to this from the
    /// state-diff stage. Exposed so callers can read without going
    /// through the broadcast channel.
    pub state: Arc<RwLock<TradingState>>,
}

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("capture failed: {0}")]
    Capture(#[from] CaptureError),
    #[error("engine not yet implemented (v1 stub)")]
    NotImplementedYet,
}

/// Configuration for the screen-watch engine.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Sample rate when the user is actively at the keyboard (default 2 Hz).
    pub active_sample_hz: f32,
    /// Sample rate when the user has been idle in another app for >5 min
    /// (default 0.2 Hz).
    pub idle_sample_hz: f32,
    /// User-defined ROIs to OCR each tick.
    pub rois: Vec<Roi>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            active_sample_hz: 2.0,
            idle_sample_hz: 0.2,
            rois: Vec::new(),
        }
    }
}

/// Start the screen-watch engine. Real impl spawns the capture thread,
/// the OCR pool, and the async LLM task; returns a handle whose Drop
/// stops everything cleanly.
///
/// v1 stub returns `EngineError::NotImplementedYet` so callers can wire
/// the API surface without having a real engine.
pub fn start(_target_hwnd: u64, _config: EngineConfig) -> Result<EngineHandle, EngineError> {
    Err(EngineError::NotImplementedYet)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_config_defaults_match_research_recommendations() {
        let c = EngineConfig::default();
        assert!((c.active_sample_hz - 2.0).abs() < f32::EPSILON);
        assert!((c.idle_sample_hz - 0.2).abs() < f32::EPSILON);
        assert!(c.rois.is_empty());
    }

    #[test]
    fn start_returns_not_implemented_in_stub() {
        let err = start(0, EngineConfig::default()).unwrap_err();
        assert!(matches!(err, EngineError::NotImplementedYet));
    }

    #[test]
    fn engine_error_capture_wraps_capture_error() {
        let err: EngineError = CaptureError::WindowNotFound(0xCAFE).into();
        let msg = format!("{err}");
        assert!(msg.contains("capture failed"));
        assert!(msg.contains("0xcafe"));
    }
}
