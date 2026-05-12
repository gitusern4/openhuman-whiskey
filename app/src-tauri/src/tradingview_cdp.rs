//! TradingView Desktop bridge over Chrome DevTools Protocol.
//!
//! TradingView Desktop is an Electron app. Like every Chromium-based app,
//! its renderer process exposes the Chrome DevTools Protocol when launched
//! with `--remote-debugging-port=<port>`. We use that to read the chart's
//! live state (symbol, timeframe, indicator values, drawn levels, alerts)
//! and write back (switch symbol, set alerts, draw lines, inject Pine
//! Script) — bidirectional, all-local, with no contact to TV's servers.
//!
//! Reference implementations:
//!   - github.com/tradesdontlie/tradingview-mcp
//!   - github.com/LewisWJackson/tradingview-mcp-jackson
//! These map TV's internal JavaScript object tree under `window` and
//! provide a known-working set of `Runtime.evaluate` snippets.
//!
//! Why a separate module from `crate::cdp`:
//!   - `crate::cdp` is hardwired to `127.0.0.1:19222` (the app's own
//!     CEF host). TV Desktop listens on a user-chosen port, default
//!     9222. Sharing the discovery layer would force `crate::cdp` to
//!     parameterise its constants and ripple through every scanner.
//!   - The TV bridge has different lifecycle: a single long-lived
//!     attached session per TV process, not per-account scanners.
//!   - The introspection JavaScript here is TV-specific and brittle
//!     (TV releases routinely move internal API paths). Keeping it in
//!     one file lets us patch all of it in one place when TV ships an
//!     update.
//!
//! Setup the user must do once:
//!   1. Quit TradingView Desktop.
//!   2. Add `--remote-debugging-port=9222` to the launch shortcut, OR
//!      run `tradingview.exe --remote-debugging-port=9222` directly.
//!   3. Open a chart. Then call `tv_cdp_attach` from the UI.
//!
//! TOS posture: all traffic is localhost → localhost. No data is sent to
//! TV's servers that wouldn't otherwise be sent by normal chart usage.
//! No community-reported account bans for CDP usage. Still, this is
//! "personal use, local execution" territory — do not redistribute the
//! captured chart data. See WHISKEY_AUDIT.md for the project's TOS
//! discipline notes.

use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::Mutex;

use crate::cdp::target::CdpTarget;
use crate::cdp::CdpConn;

/// Default Chrome DevTools port for TradingView Desktop. The user can
/// override via `tv_cdp_attach`'s `port` argument when they had to pick
/// something else (e.g. 9222 was already in use).
pub const DEFAULT_TV_CDP_PORT: u16 = 9222;

/// Hostname to probe. We try both forms because some Windows DNS
/// resolvers serve `localhost` from a stale cache that resolves to ::1
/// while CDP only listens on IPv4 127.0.0.1.
const TV_CDP_HOSTS: [&str; 2] = ["127.0.0.1", "localhost"];

/// HTTP timeout for `/json/version` discovery — the user-facing
/// "is TV Desktop reachable" probe must not hang the UI.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);

/// Substring matched against a CDP target's `url` to identify a
/// TradingView page. TV Desktop loads `https://www.tradingview.com/...`
/// in its renderer, so substring match is sufficient and survives URL
/// rewrites between TV releases.
const TV_URL_MARKER: &str = "tradingview.com";

/// One attached session to a TradingView Desktop page.
///
/// Held inside a Tokio `Mutex` in `TvCdpState` so concurrent UI
/// commands serialize against the underlying WebSocket (CdpConn is
/// not re-entrant during request/response phase — see `cdp::conn`).
pub struct TvCdpSession {
    pub conn: CdpConn,
    pub session_id: String,
    pub target_url: String,
}

/// Tauri-managed state. Wrapped in an `Arc<Mutex<Option<_>>>` so
/// `attach` and `detach` are idempotent and safe to call repeatedly
/// from the UI without losing the active session.
#[derive(Default)]
pub struct TvCdpState(pub Arc<Mutex<Option<TvCdpSession>>>);

/// Returned by `tv_cdp_probe`. A serialisable summary the UI shows
/// before the user commits to an attach.
#[derive(Debug, Serialize, Deserialize)]
pub struct TvCdpProbeResult {
    pub reachable: bool,
    pub port: u16,
    pub browser_ws_url: Option<String>,
    pub tv_targets: Vec<TvCdpTargetSummary>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TvCdpTargetSummary {
    pub id: String,
    pub url: String,
    pub title: String,
}

/// Returned by `tv_cdp_get_chart_state`. The UI consumes this directly,
/// and Whiskey's playbook scorer consumes a serialised form via the
/// event bus. Fields are `Option<...>` because TV updates routinely
/// rename the internal JS paths — a missing field is "TV moved this",
/// not "broken bridge".
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct TvChartState {
    pub symbol: Option<String>,
    pub resolution: Option<String>,
    pub price: Option<f64>,
    pub indicator_count: Option<u32>,
    /// Raw JSON of whatever else our introspection snippet returns —
    /// keeps the schema forward-compatible while the bridge matures.
    pub raw: Value,
}

// ---------------------------------------------------------------------------
// HTTP discovery
// ---------------------------------------------------------------------------

/// Hit `/json/version` on the TV Desktop CDP host to discover the
/// browser-level WebSocket URL. Returns `Err` if TV isn't running with
/// `--remote-debugging-port=<port>` or if the port is firewalled.
async fn discover_browser_ws(port: u16) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent("openhuman-whiskey-tv-cdp/1.0")
        .timeout(PROBE_TIMEOUT)
        .build()
        .map_err(|e| format!("reqwest build: {e}"))?;
    let mut last_err: Option<String> = None;
    for host in TV_CDP_HOSTS {
        let url = format!("http://{host}:{port}/json/version");
        match client.get(&url).send().await {
            Ok(resp) => match resp.json::<Value>().await {
                Ok(v) => {
                    if let Some(ws) = v.get("webSocketDebuggerUrl").and_then(|x| x.as_str()) {
                        return Ok(ws.to_string());
                    }
                    last_err = Some(format!("no webSocketDebuggerUrl in {url}"));
                }
                Err(e) => last_err = Some(format!("parse {url}: {e}")),
            },
            Err(e) => last_err = Some(format!("GET {url}: {e}")),
        }
    }
    Err(last_err.unwrap_or_else(|| "TV CDP unreachable".to_string()))
}

/// List `/json/list` page targets and filter to those whose URL contains
/// the TradingView marker. TV Desktop typically has multiple targets
/// (the chart, settings popouts, etc.) — the caller usually wants the
/// largest/oldest chart target, but this returns all of them so the UI
/// can let the user pick when there's ambiguity.
async fn list_tv_page_targets(port: u16) -> Result<Vec<TvCdpTargetSummary>, String> {
    let client = reqwest::Client::builder()
        .user_agent("openhuman-whiskey-tv-cdp/1.0")
        .timeout(PROBE_TIMEOUT)
        .build()
        .map_err(|e| format!("reqwest build: {e}"))?;
    let mut last_err: Option<String> = None;
    for host in TV_CDP_HOSTS {
        let url = format!("http://{host}:{port}/json/list");
        match client.get(&url).send().await {
            Ok(resp) => match resp.json::<Value>().await {
                Ok(v) => return Ok(extract_tv_targets(&v)),
                Err(e) => last_err = Some(format!("parse {url}: {e}")),
            },
            Err(e) => last_err = Some(format!("GET {url}: {e}")),
        }
    }
    Err(last_err.unwrap_or_else(|| "TV CDP target list unreachable".to_string()))
}

/// Pure helper: pull TV-marked page targets out of a `/json/list` JSON
/// body. Extracted so the parser is unit-testable without a live TV
/// process.
fn extract_tv_targets(v: &Value) -> Vec<TvCdpTargetSummary> {
    v.as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|t| {
                    let url = t.get("url").and_then(|u| u.as_str()).unwrap_or("");
                    if !url.contains(TV_URL_MARKER) {
                        return None;
                    }
                    let kind = t.get("type").and_then(|u| u.as_str()).unwrap_or("");
                    if kind != "page" {
                        return None;
                    }
                    Some(TvCdpTargetSummary {
                        id: t.get("id").and_then(|u| u.as_str())?.to_string(),
                        url: url.to_string(),
                        title: t
                            .get("title")
                            .and_then(|u| u.as_str())
                            .unwrap_or("")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// JavaScript snippets — TV's internal object tree
// ---------------------------------------------------------------------------

/// Read the currently displayed symbol + timeframe + last price from the
/// TV chart. The expression is written to be forgiving: every property
/// access is `?.` so a missing internal path returns `null` rather than
/// throwing, and the surrounding `JSON.stringify` makes the whole
/// expression safe to feed into `Runtime.evaluate` with `returnByValue`.
///
/// This is the V1 introspection snippet. It captures the high-value
/// fields a trading mentor needs (which symbol is the user looking at,
/// what's the price). Drawn levels + indicator values + alerts come
/// online in V2 once we've validated the symbol/price round-trip works
/// across TV releases.
const JS_GET_CHART_STATE: &str = r#"
(() => {
  try {
    const tv = window.tradingViewApi || window.TradingView || {};
    const chartWidget = window.chartWidget || (tv.chart && tv.chart()) || null;
    const activeChart = (chartWidget && typeof chartWidget.activeChart === 'function')
      ? chartWidget.activeChart()
      : null;
    const symbol = activeChart && typeof activeChart.symbol === 'function'
      ? activeChart.symbol()
      : (document.querySelector('[data-name="symbol-search-items-dialog"] input')?.value
         || document.title);
    const resolution = activeChart && typeof activeChart.resolution === 'function'
      ? activeChart.resolution()
      : null;
    const study = activeChart && typeof activeChart.getAllStudies === 'function'
      ? activeChart.getAllStudies()
      : null;
    return JSON.stringify({
      symbol: symbol || null,
      resolution: resolution || null,
      price: null,
      indicator_count: study ? study.length : null,
      _probe: {
        has_chartWidget: !!chartWidget,
        has_activeChart: !!activeChart,
        document_title: document.title || null
      }
    });
  } catch (e) {
    return JSON.stringify({ error: String(e) });
  }
})()
"#;

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Cheap probe — does TV Desktop have a CDP listener on the requested
/// port? Returns reachability + the discovered targets so the UI can
/// tell the user whether they need to relaunch TV with the debug flag.
#[tauri::command]
pub async fn tv_cdp_probe(port: Option<u16>) -> Result<TvCdpProbeResult, String> {
    let port = port.unwrap_or(DEFAULT_TV_CDP_PORT);
    let ws = discover_browser_ws(port).await;
    let (browser_ws_url, reachable, error) = match ws {
        Ok(url) => (Some(url), true, None),
        Err(e) => (None, false, Some(e)),
    };
    let tv_targets = if reachable {
        list_tv_page_targets(port).await.unwrap_or_default()
    } else {
        Vec::new()
    };
    Ok(TvCdpProbeResult {
        reachable,
        port,
        browser_ws_url,
        tv_targets,
        error,
    })
}

/// Open a CDP session against the first TradingView page target found
/// on the requested port. Stores the live session in `TvCdpState` so
/// follow-up commands (`tv_cdp_eval`, `tv_cdp_get_chart_state`) can
/// reuse it without re-handshaking. Idempotent: calling `attach` while
/// a session already exists will detach the old one first.
#[tauri::command]
pub async fn tv_cdp_attach(
    state: tauri::State<'_, TvCdpState>,
    port: Option<u16>,
) -> Result<TvCdpProbeResult, String> {
    let port = port.unwrap_or(DEFAULT_TV_CDP_PORT);
    // Drop any existing session before replacing it. Best-effort: if
    // the old session is already dead the detach call errors and we
    // proceed regardless.
    {
        let mut guard = state.0.lock().await;
        if let Some(mut old) = guard.take() {
            let _ = crate::cdp::detach_session(&mut old.conn, &old.session_id).await;
        }
    }

    let ws_url = discover_browser_ws(port).await?;
    let mut conn = CdpConn::open(&ws_url).await?;

    // Find the first TradingView page target via CDP itself rather
    // than the HTTP /json/list — keeps us on the same connection.
    let targets_v = conn
        .call("Target.getTargets", json!({}), None)
        .await
        .map_err(|e| format!("Target.getTargets: {e}"))?;
    let target = pick_first_tv_target(&targets_v).ok_or_else(|| {
        "No TradingView page target found. Open a chart in TV Desktop first.".to_string()
    })?;

    let attach = conn
        .call(
            "Target.attachToTarget",
            json!({ "targetId": target.id, "flatten": true }),
            None,
        )
        .await
        .map_err(|e| format!("Target.attachToTarget: {e}"))?;
    let session_id = attach
        .get("sessionId")
        .and_then(|x| x.as_str())
        .ok_or_else(|| "attach response missing sessionId".to_string())?
        .to_string();

    let target_url = target.url.clone();
    {
        let mut guard = state.0.lock().await;
        *guard = Some(TvCdpSession {
            conn,
            session_id,
            target_url: target_url.clone(),
        });
    }

    Ok(TvCdpProbeResult {
        reachable: true,
        port,
        browser_ws_url: Some(ws_url),
        tv_targets: vec![TvCdpTargetSummary {
            id: target.id,
            url: target_url,
            title: target.title,
        }],
        error: None,
    })
}

/// Evaluate an arbitrary JavaScript expression against the attached TV
/// page. The expression is wrapped with `returnByValue: true` so the
/// caller gets a serialisable JSON value, not a remote object handle.
///
/// The Tauri allowlist for this command is intentionally restrictive —
/// arbitrary JS execution against a logged-in TV session is a power
/// tool. Today this command is gated behind the WhiskeyMode tool-
/// allowlist (see `crate::openhuman::modes::whiskey`) so non-Whiskey
/// modes cannot reach it.
#[tauri::command]
pub async fn tv_cdp_eval(
    state: tauri::State<'_, TvCdpState>,
    expression: String,
) -> Result<Value, String> {
    let mut guard = state.0.lock().await;
    let session = guard
        .as_mut()
        .ok_or_else(|| "Not attached to TV. Call tv_cdp_attach first.".to_string())?;
    let result = session
        .conn
        .call(
            "Runtime.evaluate",
            json!({
                "expression": expression,
                "returnByValue": true,
                "awaitPromise": true,
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

/// Higher-level convenience: run the V1 introspection snippet and
/// shape the result into `TvChartState`. The raw JSON is preserved on
/// `state.raw` so the UI / event bus can pick up new fields the snippet
/// learns to extract before the typed struct catches up.
#[tauri::command]
pub async fn tv_cdp_get_chart_state(
    state: tauri::State<'_, TvCdpState>,
) -> Result<TvChartState, String> {
    let raw = tv_cdp_eval(state, JS_GET_CHART_STATE.to_string()).await?;
    let parsed: Value = match &raw {
        Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
        other => other.clone(),
    };
    Ok(TvChartState {
        symbol: parsed
            .get("symbol")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        resolution: parsed
            .get("resolution")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        price: parsed.get("price").and_then(|v| v.as_f64()),
        indicator_count: parsed
            .get("indicator_count")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32),
        raw: parsed,
    })
}

/// Detach the session (if any) and drop the underlying WebSocket.
/// Idempotent — calling `detach` with no live session is a no-op.
#[tauri::command]
pub async fn tv_cdp_detach(state: tauri::State<'_, TvCdpState>) -> Result<(), String> {
    let mut guard = state.0.lock().await;
    if let Some(mut old) = guard.take() {
        let _ = crate::cdp::detach_session(&mut old.conn, &old.session_id).await;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Pick the first TradingView page target from a `Target.getTargets`
/// response. Pure for testability.
fn pick_first_tv_target(v: &Value) -> Option<CdpTarget> {
    v.get("targetInfos")
        .and_then(|x| x.as_array())?
        .iter()
        .filter_map(|t| {
            let url = t.get("url").and_then(|u| u.as_str()).unwrap_or("");
            let kind = t.get("type").and_then(|u| u.as_str()).unwrap_or("");
            if kind == "page" && url.contains(TV_URL_MARKER) {
                Some(CdpTarget {
                    id: t.get("targetId")?.as_str()?.to_string(),
                    kind: kind.to_string(),
                    url: url.to_string(),
                    title: t
                        .get("title")
                        .and_then(|u| u.as_str())
                        .unwrap_or("")
                        .to_string(),
                })
            } else {
                None
            }
        })
        .next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_tv_targets_filters_non_tv_pages() {
        let body = json!([
            {"id": "a", "type": "page", "url": "https://www.tradingview.com/chart/", "title": "NQ"},
            {"id": "b", "type": "page", "url": "https://example.com/", "title": "other"},
            {"id": "c", "type": "service_worker", "url": "https://www.tradingview.com/sw.js", "title": ""},
        ]);
        let out = extract_tv_targets(&body);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "a");
    }

    #[test]
    fn extract_tv_targets_empty_on_non_array() {
        assert!(extract_tv_targets(&json!({"foo": "bar"})).is_empty());
        assert!(extract_tv_targets(&Value::Null).is_empty());
    }

    #[test]
    fn pick_first_tv_target_skips_non_page_and_non_tv() {
        let body = json!({
            "targetInfos": [
                {"targetId": "1", "type": "iframe", "url": "https://www.tradingview.com/", "title": ""},
                {"targetId": "2", "type": "page", "url": "https://other.com/", "title": ""},
                {"targetId": "3", "type": "page", "url": "https://www.tradingview.com/chart/abc/", "title": "ES"},
            ]
        });
        let target = pick_first_tv_target(&body).expect("expected a TV page target");
        assert_eq!(target.id, "3");
        assert_eq!(target.kind, "page");
        assert!(target.url.contains("tradingview.com"));
    }

    #[test]
    fn pick_first_tv_target_none_when_no_match() {
        let body = json!({
            "targetInfos": [
                {"targetId": "1", "type": "page", "url": "https://other.com/", "title": ""},
            ]
        });
        assert!(pick_first_tv_target(&body).is_none());
    }

    #[test]
    fn pick_first_tv_target_none_when_targetinfos_missing() {
        assert!(pick_first_tv_target(&json!({})).is_none());
    }

    #[test]
    fn default_port_is_chrome_devtools_standard() {
        // 9222 is the Chrome DevTools default; TV Desktop inherits it.
        // Our app's own CEF host uses 19222 to avoid collision (see
        // `crate::cdp::CDP_PORT`). Make sure these never converge.
        assert_eq!(DEFAULT_TV_CDP_PORT, 9222);
        assert_ne!(DEFAULT_TV_CDP_PORT, crate::cdp::CDP_PORT);
    }
}
