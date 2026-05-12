//! Tauri command handlers for the order-flow feature set.
//!
//! All mutable state lives in `OrderFlowStore`, held in Tauri's managed-state
//! map (see registration in `lib.rs`).  Commands follow the identical pattern
//! used by `tradingview_cdp.rs`: state acquired through `tauri::State`, async
//! where I/O is needed, sync where computation is pure.
//!
//! Security gates
//! --------------
//! - `order_flow_tag_active_trade`: tag id checked against
//!   `order_flow::ALLOWED_TAGS` inside `order_flow::tag_active_trade`.
//! - `order_flow_apply_preset`: preset name checked against
//!   `order_flow::WORKSPACE_PRESETS` via `order_flow::lookup_preset`.
//!
//! Config persistence
//! ------------------
//! `order_flow_set_config` persists through `tks_mods_config::save` after
//! embedding the validated `OrderFlowConfig` into the parent struct.

use std::sync::Mutex;

use serde_json::json;
use tauri::State;

use crate::tradingview_cdp::TvCdpState;

// Re-export types from the core crate so the Tauri shell doesn't need to
// depend on internal crate paths directly.
use openhuman_core::openhuman::modes::order_flow::{
    lookup_preset, record_bar, tag_active_trade, ALLOWED_TAGS,
};
pub use openhuman_core::openhuman::modes::order_flow::{
    BarDelta, OrderFlowConfig, OrderFlowState, OrderFlowTag,
};
use openhuman_core::openhuman::modes::tks_mods_config;

/// Process-wide managed state for order flow.
pub struct OrderFlowStore {
    pub config: Mutex<OrderFlowConfig>,
    pub state: Mutex<OrderFlowState>,
}

impl Default for OrderFlowStore {
    fn default() -> Self {
        let persisted = tks_mods_config::load();
        Self {
            config: Mutex::new(persisted.order_flow),
            state: Mutex::new(OrderFlowState::default()),
        }
    }
}

// ─── commands ────────────────────────────────────────────────────────────────

/// Return the current `OrderFlowConfig`.
#[tauri::command]
pub fn order_flow_get_config(store: State<'_, OrderFlowStore>) -> OrderFlowConfig {
    store
        .config
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
}

/// Persist and activate a new `OrderFlowConfig`.
///
/// `polling_hz` is clamped to `MAX_POLLING_HZ` (2) before save.
#[tauri::command]
pub fn order_flow_set_config(cfg: OrderFlowConfig, store: State<'_, OrderFlowStore>) {
    let validated = cfg.validated();
    let mut guard = store.config.lock().unwrap_or_else(|p| p.into_inner());
    *guard = validated.clone();
    drop(guard);

    // Persist through tks_mods_config so a single TOML file owns all
    // TK's Mods state.
    let mut full = tks_mods_config::load();
    full.order_flow = validated;
    tks_mods_config::save(&full);
}

/// Record one bar of order-flow data, advance the ring buffer and cumulative
/// delta, then return the updated `OrderFlowState`.
///
/// The frontend calls this at `polling_hz` (≤ 2 Hz) with values obtained
/// from TradingView's CDP bridge.
#[tauri::command]
pub fn order_flow_record_bar(
    open: f64,
    high: f64,
    low: f64,
    close: f64,
    bid_vol: u64,
    ask_vol: u64,
    store: State<'_, OrderFlowStore>,
) -> OrderFlowState {
    let bar = BarDelta {
        open,
        high,
        low,
        close,
        bid_vol,
        ask_vol,
    };
    let mut state_guard = store.state.lock().unwrap_or_else(|p| p.into_inner());
    record_bar(&mut state_guard, bar);
    state_guard.clone()
}

/// Append a tag to the active-trade in-memory list.
///
/// Returns `Err` when the tag id is not in the allowlist or the list is full.
/// The allowlist is a const slice; no caller-supplied string reaches
/// persistence unchecked.
#[tauri::command]
pub fn order_flow_tag_active_trade(
    tag: String,
    store: State<'_, OrderFlowStore>,
) -> Result<(), String> {
    if !ALLOWED_TAGS.contains(&tag.as_str()) {
        return Err(format!(
            "unknown tag {:?}; allowed: {:?}",
            tag, ALLOWED_TAGS
        ));
    }
    let ts_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let of_tag = OrderFlowTag { id: tag, ts_ms };
    let mut state_guard = store.state.lock().unwrap_or_else(|p| p.into_inner());
    tag_active_trade(&mut state_guard, of_tag)
}

/// Load a workspace preset by name and issue the relevant TradingView CDP
/// `Runtime.evaluate` calls to create each indicator.
///
/// The preset name is checked against `WORKSPACE_PRESETS` before any CDP
/// traffic.  Unknown names are rejected with `Err`.
///
/// Requires a live TvCdpSession (`tv_cdp_attach` must have been called).
#[tauri::command]
pub async fn order_flow_apply_preset(
    name: String,
    tv_state: State<'_, TvCdpState>,
) -> Result<(), String> {
    let indicator_ids = lookup_preset(&name).ok_or_else(|| {
        format!(
            "unknown preset {:?}; allowed: vwap_profile_anchored, \
             standard_orderflow, delta_focused",
            name
        )
    })?;

    let mut guard = tv_state.0.lock().await;
    let session = guard
        .as_mut()
        .ok_or_else(|| "no TV CDP session; call tv_cdp_attach first".to_string())?;

    for &indicator_id in indicator_ids {
        // indicator_id comes from the WORKSPACE_PRESETS const — safe to
        // embed directly in JS.
        let js = format!(
            "(function(){{\
               var c=window.tvWidget&&window.tvWidget.activeChart&&\
                     window.tvWidget.activeChart();\
               if(!c)return\"no-chart\";\
               c.createStudy(\"{indicator_id}\",false,false);\
               return\"ok\";\
             }})()"
        );
        let result = session
            .conn
            .call(
                "Runtime.evaluate",
                json!({
                    "expression": js,
                    "returnByValue": true,
                    "awaitPromise": false,
                }),
                Some(&session.session_id),
            )
            .await
            .map_err(|e| format!("CDP eval failed for indicator {indicator_id:?}: {e}"))?;

        log::debug!(
            "[order_flow] preset={name:?} indicator={indicator_id:?} \
             result={result:?}"
        );
    }

    Ok(())
}
