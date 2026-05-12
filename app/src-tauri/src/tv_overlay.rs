//! Whiskey TradingView overlay panel — in-page DOM injection.
//!
//! Injects a self-contained vanilla JS panel into TV Desktop's renderer
//! via the existing CDP bridge (`tradingview_cdp::TvCdpState`). The panel
//! floats at z-index 999999, docked to the right edge of the chart, and
//! contains symbol favorites, quick SL/TP form, order-flow tag chips, and
//! a walk-away lockout banner.
//!
//! Bidirectional comms:
//!   - Rust → panel: sets `window.__WHISKEY_OVERLAY_STATE` JSON; panel
//!     polls every 100ms and re-renders on change.
//!   - Panel → Rust: panel pushes `{type, ...}` objects into
//!     `window.__WHISKEY_OVERLAY_OUTBOX`; background loop drains every 500ms.
//!
//! Auto-re-inject: the injected JS installs a `MutationObserver` on
//! `document.body` that re-creates the panel if it gets detached (TV
//! reloads the pane on layout changes, symbol switches, and indicator ops).
//!
//! Polling loop: `tv_overlay_inject` starts a 2 Hz tokio background task
//! that pushes current state and drains the outbox. Cancelled on
//! `tv_overlay_remove`.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::tradingview_cdp::TvCdpSession;

/// Bundled overlay JS — injected verbatim via `Runtime.evaluate`.
pub const OVERLAY_JS_SOURCE: &str = include_str!("overlay/whiskey_overlay.js");

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Lockout snapshot passed into the overlay state.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct LockoutStatus {
    pub is_locked: bool,
    pub locked_until_unix: Option<i64>,
    pub lock_reason: Option<String>,
    pub daily_loss_dollars: f64,
    pub consecutive_losses: u32,
}

/// Full overlay state pushed to the panel every 500ms.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OverlayState {
    /// User's symbol favorites (from TK's Mods).
    pub favorites: Vec<String>,
    /// Current lockout status.
    pub lockout: LockoutStatus,
    /// Default (entry, stop, target) from risk preset if available.
    pub default_sltp: (f64, f64, f64),
    /// Currently active order-flow tag for the live trade.
    pub active_tag: Option<String>,
}

/// A command drained from `window.__WHISKEY_OVERLAY_OUTBOX`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverlayCommand {
    /// Discriminator: "set_symbol" | "draw_sltp" | "clear_sltp" | "order_flow_tag"
    #[serde(rename = "type")]
    pub kind: String,
    /// Per-session nonce stamped by `postCommand()` in the panel JS.
    /// Validated against the session nonce in `TvOverlayInner.nonce`
    /// at drain time. Commands missing or with mismatched nonce are
    /// silently dropped — defends against TV-page scripts forging
    /// outbox entries.
    #[serde(rename = "__nonce", default)]
    pub nonce: Option<String>,
    /// For set_symbol
    pub symbol: Option<String>,
    /// For draw_sltp
    pub entry: Option<f64>,
    pub stop: Option<f64>,
    pub target: Option<f64>,
    /// For order_flow_tag
    pub tag: Option<String>,
}

/// Result returned by `tv_overlay_inject`.
#[derive(Debug, Serialize, Deserialize)]
pub struct InjectResult {
    pub ok: bool,
    pub panel_id: Option<String>,
    pub skipped: bool,
    pub error: Option<String>,
}

/// Tauri-managed overlay controller. Holds the polling task handle.
#[derive(Default)]
pub struct TvOverlayState(pub Arc<Mutex<TvOverlayInner>>);

pub struct TvOverlayInner {
    pub injected: bool,
    /// Cancellation handle for the polling loop.
    pub task: Option<JoinHandle<()>>,
    /// Latest state snapshot.
    pub last_state: OverlayState,
    /// Per-session 32-char hex nonce baked into the panel JS at inject
    /// time as a closure variable (NOT on window). Outbox commands
    /// missing or mismatched on this nonce are rejected by drain.
    /// Defends against TV-page scripts (Pine indicator previews, URL
    /// preview iframes, future TV ads) that could otherwise write to
    /// `window.__WHISKEY_OVERLAY_OUTBOX` and escalate via our poll.
    /// Senior architect review 2026-05-12.
    pub nonce: Option<String>,
}

impl Default for TvOverlayInner {
    fn default() -> Self {
        Self {
            injected: false,
            task: None,
            last_state: OverlayState::default(),
            nonce: None,
        }
    }
}

/// Generate a fresh 32-char hex nonce for the overlay session.
/// Uses tokio's task-local time + std::process::id as entropy source.
/// Not cryptographically rigorous — the threat model is "TV-page
/// scripts in the same renderer guessing our nonce", and 128 bits of
/// time+pid+counter entropy is overkill for that adversary.
fn fresh_overlay_nonce() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let pid = std::process::id() as u64;
    let ctr = COUNTER.fetch_add(1, Ordering::SeqCst);
    let mixed: u128 =
        ((nanos as u128) ^ ((pid as u128) << 64)) ^ ((ctr as u128).wrapping_mul(0x9E37_79B9_7F4A_7C15));
    format!("{:032x}", mixed)
}

// ---------------------------------------------------------------------------
// JS helpers
// ---------------------------------------------------------------------------

/// Push a state JSON blob to `window.__WHISKEY_OVERLAY_STATE`.
fn js_push_state(state_json: &str) -> String {
    format!(
        r#"(() => {{ try {{ window.__WHISKEY_OVERLAY_STATE = {}; return JSON.stringify({{ok:true}}); }} catch(e) {{ return JSON.stringify({{ok:false,error:String(e)}}); }} }})()"#,
        state_json
    )
}

/// Read and drain `window.__WHISKEY_OVERLAY_OUTBOX`.
const JS_DRAIN_OUTBOX: &str = r#"
(() => {
  try {
    var box = window.__WHISKEY_OVERLAY_OUTBOX;
    if (!Array.isArray(box) || box.length === 0) return JSON.stringify([]);
    var drained = box.splice(0, box.length);
    return JSON.stringify(drained);
  } catch (e) {
    return JSON.stringify([]);
  }
})()
"#;

/// Remove the overlay panel and veil from TV's page.
const JS_REMOVE_OVERLAY: &str = r#"
(() => {
  document.getElementById('whiskey-tv-overlay')?.remove();
  document.getElementById('whiskey-lockout-veil')?.remove();
  return JSON.stringify({ok:true});
})()
"#;

// ---------------------------------------------------------------------------
// Internal CDP eval helper (works against a raw session, not Tauri state)
// ---------------------------------------------------------------------------

async fn cdp_eval_raw(
    session: &mut TvCdpSession,
    expression: &str,
) -> Result<Value, String> {
    let result = session
        .conn
        .call(
            "Runtime.evaluate",
            json!({
                "expression": expression,
                "returnByValue": true,
                "awaitPromise": false,
            }),
            Some(&session.session_id),
        )
        .await?;
    Ok(result
        .get("result")
        .and_then(|r| r.get("value"))
        .cloned()
        .unwrap_or(Value::Null))
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Inject the Whiskey overlay panel into the currently attached TV page.
/// Idempotent — if the panel is already present the JS skips re-creation.
/// Starts the 2 Hz polling loop.
#[tauri::command]
pub async fn tv_overlay_inject(
    tv_state: tauri::State<'_, crate::tradingview_cdp::TvCdpState>,
    overlay_state: tauri::State<'_, TvOverlayState>,
) -> Result<InjectResult, String> {
    // Generate a fresh per-session nonce and bake it into the JS as
    // a closure variable (replacement of the `__WHISKEY_NONCE__`
    // placeholder). The nonce is NEVER written to `window` from the
    // injected code — it lives in IIFE closure scope, so TV-page
    // scripts cannot read it. Outbox commands stamp this nonce and
    // the drain checks it server-side; commands without a valid
    // nonce are dropped.
    let nonce = fresh_overlay_nonce();
    {
        let mut o = overlay_state.0.lock().await;
        o.nonce = Some(nonce.clone());
    }
    let bundled_source = OVERLAY_JS_SOURCE.replace("__WHISKEY_NONCE__", &nonce);

    // Inject the JS bundle through the shared TV session.
    let raw = {
        let mut tv_guard = tv_state.0.lock().await;
        let session = tv_guard
            .as_mut()
            .ok_or_else(|| "Not attached to TV. Call tv_cdp_attach first.".to_string())?;
        cdp_eval_raw(session, &bundled_source).await?
    };

    let parsed = match &raw {
        Value::String(s) => serde_json::from_str::<Value>(s).unwrap_or(Value::Null),
        other => other.clone(),
    };

    let ok = parsed.get("ok").and_then(|v| v.as_bool()).unwrap_or(false);
    let panel_id = parsed
        .get("panel_id")
        .and_then(|v| v.as_str())
        .map(str::to_string);
    let skipped = parsed
        .get("skipped")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // Mark injected + cancel any existing polling task.
    {
        let mut guard = overlay_state.0.lock().await;
        guard.injected = true;
        if let Some(t) = guard.task.take() {
            t.abort();
        }
    }

    // Clone Arc handles for the background task.
    let tv_arc = tv_state.inner().0.clone();
    let ov_arc = overlay_state.inner().0.clone();

    let handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
        loop {
            interval.tick().await;

            // Read current state snapshot + the expected nonce.
            let (state_json, still_injected, expected_nonce) = {
                let guard = ov_arc.lock().await;
                let json = serde_json::to_string(&guard.last_state).unwrap_or_default();
                (json, guard.injected, guard.nonce.clone())
            };
            if !still_injected {
                break;
            }

            let mut tv_guard = tv_arc.lock().await;
            let session = match tv_guard.as_mut() {
                Some(s) => s,
                None => continue,
            };

            // Push current state.
            let push_expr = js_push_state(&state_json);
            let _ = cdp_eval_raw(session, &push_expr).await;

            // Drain outbox + nonce-filter before dispatching. Any
            // un-nonced or mismatched-nonce commands were forged by
            // a TV-page script (Pine indicator, ad iframe, URL
            // preview) and must NOT reach dispatch.
            if let Ok(drain_val) = cdp_eval_raw(session, JS_DRAIN_OUTBOX).await {
                let raw_str = match &drain_val {
                    Value::String(s) => s.clone(),
                    _ => continue,
                };
                if let Ok(commands) = serde_json::from_str::<Vec<OverlayCommand>>(&raw_str) {
                    let filtered = filter_by_nonce(commands, expected_nonce.as_deref());
                    for cmd in filtered {
                        dispatch_command(session, cmd).await;
                    }
                }
            }
        }
    });

    {
        let mut guard = overlay_state.0.lock().await;
        guard.task = Some(handle);
    }

    Ok(InjectResult {
        ok,
        panel_id,
        skipped,
        error: None,
    })
}

/// Push an explicit state snapshot to the overlay immediately (supplements the poll).
#[tauri::command]
pub async fn tv_overlay_send_state(
    tv_state: tauri::State<'_, crate::tradingview_cdp::TvCdpState>,
    overlay_state: tauri::State<'_, TvOverlayState>,
    new_state: OverlayState,
) -> Result<(), String> {
    {
        let mut guard = overlay_state.0.lock().await;
        guard.last_state = new_state.clone();
    }
    let json = serde_json::to_string(&new_state).map_err(|e| e.to_string())?;
    let expr = js_push_state(&json);
    let mut tv_guard = tv_state.0.lock().await;
    let session = tv_guard
        .as_mut()
        .ok_or_else(|| "Not attached to TV.".to_string())?;
    cdp_eval_raw(session, &expr).await?;
    Ok(())
}

/// Remove the overlay panel from TV's page and cancel the polling loop.
#[tauri::command]
pub async fn tv_overlay_remove(
    tv_state: tauri::State<'_, crate::tradingview_cdp::TvCdpState>,
    overlay_state: tauri::State<'_, TvOverlayState>,
) -> Result<(), String> {
    {
        let mut guard = overlay_state.0.lock().await;
        guard.injected = false;
        if let Some(t) = guard.task.take() {
            t.abort();
        }
    }
    let mut tv_guard = tv_state.0.lock().await;
    if let Some(session) = tv_guard.as_mut() {
        let _ = cdp_eval_raw(session, JS_REMOVE_OVERLAY).await;
    }
    Ok(())
}

/// Drain and return outbox commands (used for diagnostics/testing).
/// Filters out any command whose `__nonce` doesn't match the current
/// session nonce — those are forged entries from a TV-page script
/// and must not reach the dispatcher.
#[tauri::command]
pub async fn tv_overlay_drain_outbox(
    tv_state: tauri::State<'_, crate::tradingview_cdp::TvCdpState>,
    overlay_state: tauri::State<'_, TvOverlayState>,
) -> Result<Vec<OverlayCommand>, String> {
    let expected_nonce = {
        let o = overlay_state.0.lock().await;
        o.nonce.clone()
    };
    let mut tv_guard = tv_state.0.lock().await;
    let session = tv_guard
        .as_mut()
        .ok_or_else(|| "Not attached to TV.".to_string())?;
    let raw = cdp_eval_raw(session, JS_DRAIN_OUTBOX).await?;
    let all = match &raw {
        Value::String(s) => serde_json::from_str::<Vec<OverlayCommand>>(s).map_err(|e| e.to_string())?,
        _ => Vec::new(),
    };
    Ok(filter_by_nonce(all, expected_nonce.as_deref()))
}

/// Pure helper: drop commands whose nonce doesn't match.
/// If `expected` is `None` (no nonce yet set, e.g. mid-init), drop
/// everything — safer than admitting un-nonced commands.
pub(crate) fn filter_by_nonce(
    cmds: Vec<OverlayCommand>,
    expected: Option<&str>,
) -> Vec<OverlayCommand> {
    let mut dropped = 0usize;
    let kept: Vec<OverlayCommand> = cmds
        .into_iter()
        .filter(|c| match (expected, c.nonce.as_deref()) {
            (Some(want), Some(got)) if want == got => true,
            _ => {
                dropped += 1;
                false
            }
        })
        .collect();
    if dropped > 0 {
        log::warn!(
            "[tv-overlay] dropped {dropped} outbox commands with missing/mismatched nonce — possible TV-page script forgery"
        );
    }
    kept
}

// ---------------------------------------------------------------------------
// Command dispatch
// ---------------------------------------------------------------------------

async fn dispatch_command(session: &mut TvCdpSession, cmd: OverlayCommand) {
    match cmd.kind.as_str() {
        "set_symbol" => {
            if let Some(sym) = &cmd.symbol {
                if sym.len() <= 64 && !sym.is_empty() {
                    let encoded = serde_json::to_string(sym).unwrap_or_default();
                    let expr = crate::tradingview_cdp::JS_SET_SYMBOL_SNIPPET
                        .replace("__SYMBOL__", &encoded);
                    let _ = cdp_eval_raw(session, &expr).await;
                }
            }
        }
        "draw_sltp" => {
            if let (Some(entry), Some(stop), Some(target)) = (cmd.entry, cmd.stop, cmd.target) {
                let expr = crate::tradingview_cdp::JS_DRAW_SLTP_SNIPPET
                    .replace("__ENTRY__", &entry.to_string())
                    .replace("__STOP__", &stop.to_string())
                    .replace("__TARGET__", &target.to_string())
                    .replace("__ZETH__", "false");
                let _ = cdp_eval_raw(session, &expr).await;
            }
        }
        "clear_sltp" => {
            let _ = cdp_eval_raw(session, crate::tradingview_cdp::JS_CLEAR_SLTP_SNIPPET).await;
        }
        "order_flow_tag" => {
            // Logged for the order-flow module; actual routing is handled by
            // whatever consumes the drained outbox in the application layer.
            if let Some(tag) = &cmd.tag {
                log::info!("[tv-overlay] order_flow_tag: {tag}");
            }
        }
        other => {
            log::warn!("[tv-overlay] unknown outbox command kind: {other}");
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_state_serializes_roundtrip() {
        let state = OverlayState {
            favorites: vec!["NQ1!".to_string(), "ES1!".to_string()],
            lockout: LockoutStatus {
                is_locked: false,
                locked_until_unix: None,
                lock_reason: None,
                daily_loss_dollars: 0.0,
                consecutive_losses: 0,
            },
            default_sltp: (19800.0, 19750.0, 19900.0),
            active_tag: Some("absorbed".to_string()),
        };
        let json = serde_json::to_string(&state).unwrap();
        let back: OverlayState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.favorites, state.favorites);
        assert_eq!(back.active_tag, state.active_tag);
        assert!((back.default_sltp.0 - 19800.0).abs() < f64::EPSILON);
    }

    #[test]
    fn overlay_state_empty_defaults() {
        let state = OverlayState::default();
        assert!(state.favorites.is_empty());
        assert!(!state.lockout.is_locked);
        assert!(state.active_tag.is_none());
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("\"favorites\":[]"));
    }

    #[test]
    fn lockout_status_serializes_roundtrip() {
        let ls = LockoutStatus {
            is_locked: true,
            locked_until_unix: Some(9_999_999),
            lock_reason: Some("manual".to_string()),
            daily_loss_dollars: 350.0,
            consecutive_losses: 3,
        };
        let json = serde_json::to_string(&ls).unwrap();
        let back: LockoutStatus = serde_json::from_str(&json).unwrap();
        assert!(back.is_locked);
        assert_eq!(back.locked_until_unix, Some(9_999_999));
        assert_eq!(back.lock_reason, Some("manual".to_string()));
        assert!((back.daily_loss_dollars - 350.0).abs() < f64::EPSILON);
    }

    #[test]
    fn outbox_command_set_symbol_parses() {
        let raw = r#"[{"type":"set_symbol","symbol":"NQ1!"}]"#;
        let cmds: Vec<OverlayCommand> = serde_json::from_str(raw).unwrap();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].kind, "set_symbol");
        assert_eq!(cmds[0].symbol.as_deref(), Some("NQ1!"));
        assert!(cmds[0].entry.is_none());
        assert!(cmds[0].tag.is_none());
    }

    #[test]
    fn outbox_command_draw_sltp_parses() {
        let raw = r#"[{"type":"draw_sltp","entry":19800.0,"stop":19750.0,"target":19900.0}]"#;
        let cmds: Vec<OverlayCommand> = serde_json::from_str(raw).unwrap();
        assert_eq!(cmds.len(), 1);
        assert_eq!(cmds[0].kind, "draw_sltp");
        assert!((cmds[0].entry.unwrap() - 19800.0).abs() < f64::EPSILON);
        assert!((cmds[0].stop.unwrap() - 19750.0).abs() < f64::EPSILON);
        assert!((cmds[0].target.unwrap() - 19900.0).abs() < f64::EPSILON);
        assert!(cmds[0].symbol.is_none());
    }

    #[test]
    fn outbox_command_order_flow_tag_parses() {
        let raw = r#"[{"type":"order_flow_tag","tag":"absorbed"}]"#;
        let cmds: Vec<OverlayCommand> = serde_json::from_str(raw).unwrap();
        assert_eq!(cmds[0].kind, "order_flow_tag");
        assert_eq!(cmds[0].tag.as_deref(), Some("absorbed"));
        assert!(cmds[0].symbol.is_none());
    }

    #[test]
    fn outbox_command_clear_sltp_parses() {
        let raw = r#"[{"type":"clear_sltp"}]"#;
        let cmds: Vec<OverlayCommand> = serde_json::from_str(raw).unwrap();
        assert_eq!(cmds[0].kind, "clear_sltp");
        assert!(cmds[0].symbol.is_none());
        assert!(cmds[0].tag.is_none());
    }

    #[test]
    fn js_push_state_wraps_json_defensively() {
        let state = OverlayState::default();
        let json = serde_json::to_string(&state).unwrap();
        let expr = js_push_state(&json);
        assert!(expr.contains("__WHISKEY_OVERLAY_STATE"));
        assert!(expr.contains("try"));
        assert!(expr.contains("catch"));
    }

    #[test]
    fn overlay_js_source_non_empty_and_has_key_markers() {
        assert!(!OVERLAY_JS_SOURCE.is_empty());
        assert!(OVERLAY_JS_SOURCE.contains("whiskey-tv-overlay"));
        assert!(OVERLAY_JS_SOURCE.contains("__WHISKEY_OVERLAY_OUTBOX"));
        assert!(OVERLAY_JS_SOURCE.contains("MutationObserver"));
        assert!(OVERLAY_JS_SOURCE.contains("localStorage"));
    }

    #[test]
    fn multiple_outbox_commands_in_one_batch() {
        let raw = r#"[
          {"type":"set_symbol","symbol":"ES1!"},
          {"type":"order_flow_tag","tag":"delta_div"},
          {"type":"clear_sltp"}
        ]"#;
        let cmds: Vec<OverlayCommand> = serde_json::from_str(raw).unwrap();
        assert_eq!(cmds.len(), 3);
        assert_eq!(cmds[0].kind, "set_symbol");
        assert_eq!(cmds[1].kind, "order_flow_tag");
        assert_eq!(cmds[2].kind, "clear_sltp");
    }
}
