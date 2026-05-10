//! WGC (Windows Graphics Capture) binding for trading-window screen-watch.
//!
//! ## Status: STUB — v1 scaffolding only.
//!
//! The intended implementation uses the `windows-capture` crate
//! (NiiightmareXD) to attach to a specific HWND or `GraphicsCaptureItem`
//! and stream `Frame`s into a bounded mpsc channel at 2 Hz, dropping
//! intermediate frames the WGC `frame_arrived` callback emits faster.
//!
//! Why this is a stub for v1:
//! - Adding `windows-capture` as a dep on aarch64-pc-windows-msvc needs
//!   verification that the crate's vcpkg build artefacts are available
//!   on Windows ARM64. Worth doing in the same commit as a smoke test
//!   on the actual hardware so we don't ship a build that fails at
//!   `cargo check`.
//! - The capture loop is intertwined with `engine.rs` orchestration —
//!   landing them together as a single PR makes review easier.
//!
//! ## Public API the engine depends on
//!
//! - [`enumerate_trading_windows`] — lightweight `EnumWindows` filter
//!   that returns plausible trading-platform windows for the user to
//!   pick from in the bind UI. Currently returns an empty Vec.
//! - [`CaptureHandle`] — opaque handle returned from `start_capture`
//!   that the engine drops to stop capture. Currently a unit struct.
//!
//! See `WHISKEY.md` §11 for the full target architecture, and the
//! `roman-rr` April 2026 benchmark cited there for why we deliberately
//! never feed chart pixels to a vision LLM.

use super::types::{Frame, WindowInfo};

/// Handle returned by `start_capture`. Dropping it stops the capture
/// loop and releases the WGC `GraphicsCaptureItem`.
#[must_use = "dropping the CaptureHandle stops the capture loop"]
pub struct CaptureHandle {
    // Real impl will hold the WGC session + the JoinHandle for the
    // capture thread.
    _stub: (),
}

/// List trading-platform candidate windows. Real impl walks `EnumWindows`
/// and filters by class name + title heuristics for known platforms
/// (Tradovate Trader, NinjaTrader, Sierra Chart, ToS, TradingView desktop).
///
/// Returns an empty Vec in the v1 stub so the bind UI can render its
/// "no windows found" empty state without crashing.
pub fn enumerate_trading_windows() -> Vec<WindowInfo> {
    Vec::new()
}

/// Start a capture loop on the given window. Real impl creates a WGC
/// session, attaches a frame-arrived callback that pushes `Frame`s into
/// the mpsc channel at 2 Hz (intermediates dropped).
///
/// Currently a no-op that returns a handle whose Drop is no-op.
pub fn start_capture(_target_hwnd: u64) -> Result<CaptureHandle, CaptureError> {
    Err(CaptureError::NotImplementedYet)
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("WGC capture not yet implemented (v1 stub)")]
    NotImplementedYet,
    #[error("target window not found: hwnd={0:#x}")]
    WindowNotFound(u64),
    #[error("WGC initialisation failed: {0}")]
    InitFailed(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enumerate_returns_empty_in_stub() {
        // Sanity test — when capture is wired we'll replace this with
        // a stub-mode flag that lets tests run without spawning a real
        // EnumWindows pass.
        assert!(enumerate_trading_windows().is_empty());
    }

    #[test]
    fn start_capture_returns_not_implemented() {
        let err = start_capture(0).unwrap_err();
        assert!(matches!(err, CaptureError::NotImplementedYet));
    }

    #[test]
    fn capture_error_messages_are_human_readable() {
        let err = CaptureError::WindowNotFound(0x1234);
        let msg = format!("{err}");
        assert!(msg.contains("0x1234"));
    }

    #[allow(dead_code)]
    fn frame_unused_warning_silencer(_f: Frame) {
        // Frame is re-exported / used by other modules; no-op here keeps
        // the import alive while capture is a stub.
    }
}
