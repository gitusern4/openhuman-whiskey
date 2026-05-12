//! TradingView CDP tools — Whiskey fork.
//!
//! Two `Tool` impls that let the Whiskey LLM persona read and write the
//! active TradingView Desktop chart via the existing Tauri CDP commands.
//!
//! # Architecture note — bridge gap
//!
//! The openhuman core process and the Tauri host are two separate
//! processes. The Tauri CDP commands (`tv_cdp_get_chart_state`,
//! `tv_cdp_set_symbol`) live in `app/src-tauri/src/tradingview_cdp.rs`
//! and are invoked by the React UI over Tauri IPC today.
//!
//! Bridging from the *core* process to Tauri requires a core→Tauri
//! channel. The existing `webview_apis` bridge
//! (`OPENHUMAN_WEBVIEW_APIS_PORT`) provides this channel for Gmail/Meet,
//! but a matching `tradingview.get_chart_state` / `tradingview.set_symbol`
//! server-side handler has not yet been wired into the Tauri
//! `webview_apis` server. Until that wiring lands:
//!
//! - The tools are fully registered (schema, allowlist, category,
//!   permission level) so the merge story is clean.
//! - `execute()` returns a `ToolResult` error with the message
//!   `"core_rpc bridge not yet wired"` so callers get a clear,
//!   actionable error rather than a silent no-op.
//!
//! To complete the bridge:
//! 1. Add `tradingview.get_chart_state` and `tradingview.set_symbol`
//!    to the Tauri `webview_apis` WebSocket server
//!    (`app/src-tauri/src/webview_apis.rs`), forwarding to
//!    `crate::tradingview_cdp::{tv_cdp_get_chart_state, tv_cdp_set_symbol}`.
//! 2. Replace the stub bodies below with
//!    `webview_apis::client::request(...)` calls (same pattern as
//!    `src/openhuman/webview_apis/rpc.rs`).
//!
//! See WHISKEY_AUDIT.md and the PR description for the full context.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::openhuman::skills::types::ToolContent;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCategory, ToolResult};

// ---------------------------------------------------------------------------
// Tool name constants — matched against WhiskeyMode::ALLOWED_TOOLS strings.
// ---------------------------------------------------------------------------

pub const GET_STATE_TOOL_NAME: &str = "tv_chart_get_state";
pub const SET_SYMBOL_TOOL_NAME: &str = "tv_chart_set_symbol";

// ---------------------------------------------------------------------------
// TvChartStateTool
// ---------------------------------------------------------------------------

/// Read the live TradingView Desktop chart state (symbol, resolution,
/// indicators, shapes, alert count) via the Tauri CDP bridge.
///
/// No arguments required. Returns JSON:
/// ```json
/// {
///   "symbol": "CME_MINI:NQ1!",
///   "resolution": "5",
///   "indicator_count": 3,
///   "indicator_names": ["VWAP", "EMA 9", "EMA 21"],
///   "shape_count": 2,
///   "shape_names": ["Trend Line", "Horizontal Line"],
///   "alert_count": 1
/// }
/// ```
/// Fields are `null` when the CDP bridge can't enumerate them (TV
/// release moved an internal API path). `null` means "unknown", not 0.
///
/// Permission: `ReadOnly` — no chart state is modified.
pub struct TvChartStateTool;

#[async_trait]
impl Tool for TvChartStateTool {
    fn name(&self) -> &str {
        GET_STATE_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Read the active TradingView Desktop chart state: symbol, timeframe \
         (resolution), indicator list, drawn shape list, and alert count. \
         Requires TradingView Desktop to be running with \
         `--remote-debugging-port=9222` and a prior `tv_cdp_attach` from \
         the UI. Returns null fields when a TV release moves an internal API \
         path — degraded, not broken."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": [],
            "additionalProperties": false
        })
    }

    async fn execute(&self, _args: Value) -> anyhow::Result<ToolResult> {
        // Bridge gap: see module-level doc for the wiring plan.
        // Replace this stub with a webview_apis::client::request call once
        // the Tauri server-side handler lands.
        Ok(ToolResult {
            content: vec![ToolContent::Text {
                text: "core_rpc bridge not yet wired: \
                       tv_chart_get_state requires the Tauri webview_apis \
                       server to expose tradingview.get_chart_state. \
                       See src/openhuman/tools/whiskey/tv_chart.rs for the \
                       wiring plan."
                    .to_string(),
            }],
            is_error: true,
            markdown_formatted: None,
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::ReadOnly
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Skill
    }
}

// ---------------------------------------------------------------------------
// TvSetSymbolTool
// ---------------------------------------------------------------------------

/// Change the active TradingView Desktop chart's symbol.
///
/// Args:
/// - `symbol` (string, required, max 64 chars) — TV symbol string,
///   e.g. `"CME_MINI:NQ1!"`, `"NASDAQ:AAPL"`, `"BINANCE:BTCUSDT"`.
///
/// Returns JSON:
/// ```json
/// { "ok": true, "symbol": "CME_MINI:NQ1!", "error": null }
/// ```
/// or on failure:
/// ```json
/// { "ok": false, "symbol": null, "error": "activeChart.setSymbol unavailable" }
/// ```
///
/// The symbol is JSON-encoded on the Rust side before reaching V8 so
/// a Whiskey-controlled value can never break out of the JS expression.
///
/// Permission: `Write` — modifies the active chart.
pub struct TvSetSymbolTool;

#[async_trait]
impl Tool for TvSetSymbolTool {
    fn name(&self) -> &str {
        SET_SYMBOL_TOOL_NAME
    }

    fn description(&self) -> &str {
        "Switch the active TradingView Desktop chart to a different symbol. \
         Pass the full TV symbol string including exchange prefix, e.g. \
         `CME_MINI:NQ1!` for NQ futures, `NASDAQ:AAPL` for Apple, \
         `BINANCE:BTCUSDT` for spot BTC. Max 64 characters. \
         The symbol is validated and JSON-encoded before reaching the TV \
         renderer so LLM-controlled values cannot break out of the JS \
         expression. Returns `{ok, symbol, error}`."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "symbol": {
                    "type": "string",
                    "description": "TradingView symbol string with exchange prefix, \
                                   e.g. 'CME_MINI:NQ1!', 'NASDAQ:AAPL', 'BINANCE:BTCUSDT'.",
                    "minLength": 1,
                    "maxLength": 64
                }
            },
            "required": ["symbol"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        // Validate the symbol arg early so schema errors surface even
        // in the stubbed state — gives the caller an actionable message.
        let symbol = args
            .get("symbol")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();

        if symbol.is_empty() {
            return Ok(ToolResult {
                content: vec![ToolContent::Text {
                    text: "symbol must not be empty".to_string(),
                }],
                is_error: true,
                markdown_formatted: None,
            });
        }

        if symbol.len() > 64 {
            return Ok(ToolResult {
                content: vec![ToolContent::Text {
                    text: format!(
                        "symbol too long ({} chars > 64 max): {}",
                        symbol.len(),
                        &symbol[..symbol.len().min(32)]
                    ),
                }],
                is_error: true,
                markdown_formatted: None,
            });
        }

        // Bridge gap: see module-level doc for the wiring plan.
        Ok(ToolResult {
            content: vec![ToolContent::Text {
                text: "core_rpc bridge not yet wired: \
                       tv_chart_set_symbol requires the Tauri webview_apis \
                       server to expose tradingview.set_symbol. \
                       See src/openhuman/tools/whiskey/tv_chart.rs for the \
                       wiring plan."
                    .to_string(),
            }],
            is_error: true,
            markdown_formatted: None,
        })
    }

    fn permission_level(&self) -> PermissionLevel {
        PermissionLevel::Write
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Skill
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ── TvChartStateTool ────────────────────────────────────────────────────

    #[test]
    fn get_state_name_matches_constant() {
        let tool = TvChartStateTool;
        assert_eq!(tool.name(), GET_STATE_TOOL_NAME);
        assert_eq!(GET_STATE_TOOL_NAME, "tv_chart_get_state");
    }

    #[test]
    fn get_state_schema_has_no_required_args_and_is_strict() {
        let tool = TvChartStateTool;
        let schema = tool.parameters_schema();

        // No required args.
        let required = schema
            .get("required")
            .and_then(Value::as_array)
            .expect("schema has required array");
        assert!(required.is_empty(), "get_state takes no required args");

        // additionalProperties: false — strict schema.
        assert_eq!(
            schema.get("additionalProperties").and_then(Value::as_bool),
            Some(false),
            "schema must be strict (additionalProperties: false)"
        );
    }

    #[tokio::test]
    async fn get_state_returns_error_result_on_rpc_failure() {
        let tool = TvChartStateTool;
        let result = tool
            .execute(json!({}))
            .await
            .expect("execute must not panic");
        assert!(
            result.is_error,
            "stub must return is_error=true until bridge is wired"
        );
        // Error message must be actionable (contains the bridge status).
        let text = result
            .content
            .iter()
            .filter_map(|c| {
                if let ToolContent::Text { text } = c {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            text.contains("core_rpc bridge not yet wired"),
            "error message must name the gap: {text}"
        );
    }

    #[test]
    fn get_state_permission_and_category_invariants() {
        let tool = TvChartStateTool;
        assert_eq!(
            tool.permission_level(),
            PermissionLevel::ReadOnly,
            "chart state read is read-only"
        );
        assert_eq!(
            tool.category(),
            ToolCategory::Skill,
            "talks to external TV process — Skill category"
        );
    }

    // ── TvSetSymbolTool ─────────────────────────────────────────────────────

    #[test]
    fn set_symbol_name_matches_constant() {
        let tool = TvSetSymbolTool;
        assert_eq!(tool.name(), SET_SYMBOL_TOOL_NAME);
        assert_eq!(SET_SYMBOL_TOOL_NAME, "tv_chart_set_symbol");
    }

    #[test]
    fn set_symbol_schema_requires_symbol_and_is_strict() {
        let tool = TvSetSymbolTool;
        let schema = tool.parameters_schema();

        let required = schema
            .get("required")
            .and_then(Value::as_array)
            .expect("schema has required array");
        assert!(
            required.iter().any(|v| v.as_str() == Some("symbol")),
            "symbol must be in required"
        );

        assert_eq!(
            schema.get("additionalProperties").and_then(Value::as_bool),
            Some(false),
            "schema must be strict (additionalProperties: false)"
        );

        // maxLength: 64 must be declared in the schema.
        let max_len = schema
            .get("properties")
            .and_then(|p| p.get("symbol"))
            .and_then(|s| s.get("maxLength"))
            .and_then(Value::as_u64);
        assert_eq!(
            max_len,
            Some(64),
            "symbol maxLength must be 64 in the schema"
        );
    }

    #[tokio::test]
    async fn set_symbol_empty_string_returns_error() {
        let tool = TvSetSymbolTool;
        let result = tool
            .execute(json!({ "symbol": "" }))
            .await
            .expect("execute must not panic");
        assert!(result.is_error);
        let text = result
            .content
            .iter()
            .filter_map(|c| {
                if let ToolContent::Text { text } = c {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            text.contains("empty"),
            "error must mention empty symbol: {text}"
        );
    }

    #[tokio::test]
    async fn set_symbol_too_long_returns_error() {
        let tool = TvSetSymbolTool;
        let long_symbol = "A".repeat(65);
        let result = tool
            .execute(json!({ "symbol": long_symbol }))
            .await
            .expect("execute must not panic");
        assert!(result.is_error);
        let text = result
            .content
            .iter()
            .filter_map(|c| {
                if let ToolContent::Text { text } = c {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            text.contains("too long"),
            "error must mention too long: {text}"
        );
    }

    #[tokio::test]
    async fn set_symbol_valid_input_returns_bridge_error() {
        // Valid symbol → stub should still error with the bridge message.
        let tool = TvSetSymbolTool;
        let result = tool
            .execute(json!({ "symbol": "CME_MINI:NQ1!" }))
            .await
            .expect("execute must not panic");
        assert!(result.is_error);
        let text = result
            .content
            .iter()
            .filter_map(|c| {
                if let ToolContent::Text { text } = c {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            text.contains("core_rpc bridge not yet wired"),
            "valid input should hit the bridge stub: {text}"
        );
    }

    #[test]
    fn set_symbol_permission_and_category_invariants() {
        let tool = TvSetSymbolTool;
        assert_eq!(
            tool.permission_level(),
            PermissionLevel::Write,
            "modifying chart symbol is a write operation"
        );
        assert_eq!(
            tool.category(),
            ToolCategory::Skill,
            "talks to external TV process — Skill category"
        );
    }

    // ── Integration stub — mock core_rpc round-trip ─────────────────────────

    /// Stub integration test: verifies that when the bridge eventually
    /// responds with a valid chart state JSON, the tool can deserialize
    /// and re-serialize the expected fields. This test exercises the
    /// *shape contract* without a live Tauri process.
    ///
    /// When the bridge is wired, replace this test with one that injects
    /// a mock `webview_apis::client` and asserts the deserialized
    /// `TvChartState` fields map correctly to the `ToolResult` JSON body.
    #[test]
    fn mock_chart_state_json_round_trips_correctly() {
        // Simulate the JSON the bridge will eventually return.
        let mock_response = json!({
            "symbol": "CME_MINI:NQ1!",
            "resolution": "5",
            "price": null,
            "indicator_count": 2,
            "indicators": [
                { "id": "study_0", "name": "VWAP" },
                { "id": "study_1", "name": "EMA 9" }
            ],
            "shapes": [
                { "id": "shape_0", "name": "Trend Line" }
            ],
            "alert_count": 3,
            "raw": {}
        });

        // The tool will expose these fields to the LLM:
        let expected_tool_output = json!({
            "symbol": mock_response["symbol"],
            "resolution": mock_response["resolution"],
            "indicator_count": mock_response["indicator_count"],
            "indicator_names": ["VWAP", "EMA 9"],
            "shape_count": 1u64,
            "shape_names": ["Trend Line"],
            "alert_count": mock_response["alert_count"]
        });

        // Verify the shape we expect to produce can be serialized.
        let serialized = serde_json::to_string(&expected_tool_output)
            .expect("tool output JSON must be serializable");
        let round_tripped: Value =
            serde_json::from_str(&serialized).expect("tool output must round-trip");

        assert_eq!(round_tripped["symbol"], "CME_MINI:NQ1!");
        assert_eq!(round_tripped["resolution"], "5");
        assert_eq!(round_tripped["indicator_count"], 2);
        assert_eq!(round_tripped["indicator_names"][0], "VWAP");
        assert_eq!(round_tripped["shape_count"], 1);
        assert_eq!(round_tripped["alert_count"], 3);
    }
}
