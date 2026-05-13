//! Windows screen-watch submodule for the Whiskey trading-mentor mode.
//!
//! Mirrors the macOS-focused screen_intelligence pipeline structure but
//! built around Windows Graphics Capture (WGC), Tesseract OCR (feature-
//! gated to keep stock builds green), and anchor-based ROIs that survive
//! window resize. See [`WHISKEY.md`](../../../../WHISKEY.md) section #11
//! for the full architectural rationale.
//!
//! ## Module layout
//!
//! - [`types`] — pure data types (`Frame`, `TradingEvent`, `OcrResult`).
//!   No Win32 imports → safe to use from any thread or non-Windows tests.
//! - [`roi`] — anchor + offset ROIs and dHash perceptual-hash drift.
//! - [`ocr`] — Tesseract wrapper. Compiles unconditionally; OCR itself
//!   gated behind the `whiskey-windows-ocr` Cargo feature so the default
//!   build stays green when the C library is unavailable.
//! - [`state`] — `TradingState` snapshot + diff-emit logic.
//! - [`idle`] — `GetLastInputInfo` polling for capture-rate throttling.
//! - [`parsers`] — text-to-`TradingState` parsing (`generic.rs` first;
//!   platform-specific parsers added later).
//! - [`capture`] — WGC binding. **STUB** for v1; see file header.
//! - [`engine`] — orchestrator that wires capture → OCR → diff →
//!   broadcast. **STUB** for v1; see file header.
//!
//! ## Public API
//!
//! - [`subscribe_trading_events`] — broadcast channel for downstream
//!   subscribers (Whiskey-trader hookup, future automation).
//!
//! ## Status
//!
//! This module is **scaffolding-complete** but the capture loop and
//! orchestrator are stubs. v1 ships the data model, OCR pipeline, ROI
//! anchoring, and idle gating; the WGC capture binding and engine
//! orchestration land in a follow-up commit so they can be tested with
//! the user's actual trading platform open.

pub mod capture;
pub mod engine;
pub mod idle;
pub mod ocr;
pub mod parsers;
pub mod roi;
pub mod state;
pub mod types;

pub use types::TradingEvent;

use once_cell::sync::Lazy;
use tokio::sync::broadcast;

/// Channel capacity for trading-event broadcasts. Sized to accommodate a
/// burst (e.g. multi-leg fill, quote storm) without dropping the slowest
/// subscriber.
const EVENT_CHANNEL_CAPACITY: usize = 128;

/// Process-wide broadcast sender. Wrapped in `Lazy` so any module can
/// subscribe without threading a sender through construction.
static TRADING_EVENT_BUS: Lazy<broadcast::Sender<TradingEvent>> = Lazy::new(|| {
    let (tx, _rx) = broadcast::channel(EVENT_CHANNEL_CAPACITY);
    tx
});

/// Subscribe to `TradingEvent`s emitted by the engine. Used by the
/// Whiskey-trader hookup and any future automation that wants to react
/// to position / quote / setup-candidate changes.
pub fn subscribe_trading_events() -> broadcast::Receiver<TradingEvent> {
    TRADING_EVENT_BUS.subscribe()
}

/// Publish a `TradingEvent`. Returns the number of active subscribers
/// that received the event. Fire-and-forget if no one is listening.
pub fn publish_trading_event(event: TradingEvent) -> usize {
    match TRADING_EVENT_BUS.send(event) {
        Ok(n) => n,
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_drift_event() -> TradingEvent {
        TradingEvent::LayoutDrift {
            roi_name: "pnl".into(),
            distance: 12,
            at_ms: 0,
        }
    }

    #[tokio::test]
    async fn publish_with_no_subscribers_is_safe() {
        let delivered = publish_trading_event(fake_drift_event());
        assert_eq!(delivered, 0);
    }

    #[tokio::test]
    async fn subscribe_then_publish_delivers() {
        let mut rx = subscribe_trading_events();
        let delivered = publish_trading_event(fake_drift_event());
        assert!(delivered >= 1);
        let event = rx.recv().await.expect("event delivered");
        assert!(matches!(event, TradingEvent::LayoutDrift { .. }));
    }
}
