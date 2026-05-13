//! Tauri command: pre-trade readiness check (§5 + §7 of INTELLIGENCE_SYNTHESIS.md).
//!
//! Exposes `whiskey_readiness_check` as a Tauri command. The pure evaluation
//! logic lives in `openhuman_core::openhuman::modes::readiness` — this module
//! is a thin adapter that handles serialisation and Tauri plumbing, following
//! the same pattern as `order_flow_commands`.

use openhuman_core::openhuman::modes::readiness::{evaluate, ReadinessInput, ReadinessScore};

/// Evaluate the five-question pre-trade readiness check.
///
/// Takes a `ReadinessInput` JSON object from the frontend and returns a
/// `ReadinessScore` with per-question scores, total, trade_blocked flag,
/// and the `psychology_readiness_factor` to feed into the Bayesian
/// confidence formula.
///
/// This is a synchronous command — evaluation is O(1) arithmetic with no
/// I/O. Under Tauri v2 the `#[tauri::command]` macro handles
/// serialisation to/from JSON automatically when both input and output
/// implement `serde::Serialize` / `serde::Deserialize`.
#[tauri::command]
pub fn whiskey_readiness_check(input: ReadinessInput) -> ReadinessScore {
    evaluate(&input)
}
