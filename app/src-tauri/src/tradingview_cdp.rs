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
    /// V2: id + name of each indicator on the active chart. `None` when
    /// the introspection couldn't enumerate (TV moved the API path);
    /// empty `Some(vec![])` when there genuinely are no indicators.
    #[serde(default)]
    pub indicators: Option<Vec<TvIndicatorSummary>>,
    /// V2: id + name of each drawn shape (trendline, horizontal,
    /// text, fib, etc.). Same null-vs-empty semantics as `indicators`.
    #[serde(default)]
    pub shapes: Option<Vec<TvShapeSummary>>,
    /// V2: count of alert entries visible in the alert manager DOM.
    /// `None` when the alert panel isn't open (best we can do without
    /// a stable public surface).
    #[serde(default)]
    pub alert_count: Option<u32>,
    /// Raw JSON of whatever else our introspection snippet returns —
    /// keeps the schema forward-compatible while the bridge matures.
    pub raw: Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TvIndicatorSummary {
    pub id: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TvShapeSummary {
    pub id: Option<String>,
    pub name: Option<String>,
}

/// Result of `tv_cdp_set_symbol`. `ok: false` when TV's internal
/// `setSymbol` is unavailable for the active chart (TV release moved
/// the API path) — the user-facing error is in `error`.
#[derive(Debug, Serialize, Deserialize)]
pub struct TvSetSymbolResult {
    pub ok: bool,
    pub symbol: Option<String>,
    pub error: Option<String>,
}

/// Result of `tv_cdp_launch_tv`. Best-effort: searches a handful of
/// well-known install paths for `TradingView.exe`, spawns it with
/// `--remote-debugging-port=<port>`, returns the resolved path or
/// `error` if no install was found.
#[derive(Debug, Serialize, Deserialize)]
pub struct TvLaunchResult {
    pub launched: bool,
    pub path: Option<String>,
    pub port: u16,
    pub error: Option<String>,
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

/// Read the chart state — symbol, timeframe, drawn levels, indicator
/// list, and alert summaries — from the TV renderer.
///
/// Defensive style: every property access is `?.`, every iteration is
/// guarded by `Array.isArray` / `typeof === 'function'`, and the entire
/// expression is wrapped in a `try/catch` that returns `{ error }`
/// rather than throwing. The contract is "fields the snippet can't
/// resolve come back as `null`/empty-array, the bridge stays alive."
/// That keeps Whiskey usable when a TV release moves an internal API
/// path — the affected field degrades, the rest still works.
///
/// TV's internal object tree (paths we probe, in priority order):
///   - `window.chartWidget`                 — primary widget on Desktop
///   - `window.tradingViewApi.chart()`      — embedded library API
///   - `window.TradingView.chart()`         — legacy global
///   - `widget.activeChart()`               — currently focused chart
///   - `chart.symbol() / .resolution()`     — string accessors
///   - `chart.getAllStudies()`              — indicator list
///   - `chart.getAllShapes()`               — drawn levels / lines / text
///   - `chart.getStudyById(id).getInputValues()` — indicator params
///   - DOM fallback: `document.title`, symbol-search input value
///
/// Reference: the patterns here come from the open-source
/// `tradingview-mcp` projects (tradesdontlie + LewisWJackson forks).
/// When TV breaks one of these paths, check those repos' issue trackers
/// for the documented patch first — they're our canary.
const JS_GET_CHART_STATE: &str = r#"
(() => {
  try {
    const tv = window.tradingViewApi || window.TradingView || {};
    const chartWidget = window.chartWidget || (typeof tv.chart === 'function' ? tv.chart() : null);
    const activeChart = (chartWidget && typeof chartWidget.activeChart === 'function')
      ? chartWidget.activeChart()
      : null;

    const symbol = (activeChart && typeof activeChart.symbol === 'function')
      ? activeChart.symbol()
      : (document.querySelector('[data-name="symbol-search-items-dialog"] input')?.value
         || document.title
         || null);
    const resolution = (activeChart && typeof activeChart.resolution === 'function')
      ? activeChart.resolution()
      : null;

    // Indicators (studies). Each entry is reduced to {id, name} so the
    // JSON payload stays small. Inputs/values per indicator are an
    // opt-in V3 expansion — they balloon the payload on busy charts.
    let indicators = [];
    if (activeChart && typeof activeChart.getAllStudies === 'function') {
      const studies = activeChart.getAllStudies();
      if (Array.isArray(studies)) {
        indicators = studies.map(s => ({
          id: s?.id ?? null,
          name: s?.name ?? null
        }));
      }
    }

    // Drawn shapes: trendlines, horizontals, text labels, fib levels.
    // Same reduction policy — id + name only. Coordinates would need
    // an additional API call per shape; defer to V3.
    let shapes = [];
    if (activeChart && typeof activeChart.getAllShapes === 'function') {
      const allShapes = activeChart.getAllShapes();
      if (Array.isArray(allShapes)) {
        shapes = allShapes.map(s => ({
          id: s?.id ?? null,
          name: s?.name ?? null
        }));
      }
    }

    // Alerts: TV's alert model lives outside the chart widget in v2
    // and has no stable public surface. Best-effort: count DOM-visible
    // entries in the alert manager panel if it happens to be open.
    // Returns null (not 0) when the panel isn't mounted so the UI
    // shows "—" instead of a misleading "0".
    let alert_count = null;
    const alertList = document.querySelectorAll('[data-name="alerts-manager-item"]');
    if (alertList) {
      alert_count = alertList.length;
    }

    return JSON.stringify({
      symbol: symbol || null,
      resolution: resolution || null,
      price: null,
      indicator_count: indicators.length,
      indicators,
      shape_count: shapes.length,
      shapes,
      alert_count,
      _probe: {
        has_chartWidget: !!chartWidget,
        has_activeChart: !!activeChart,
        has_getAllStudies: !!(activeChart && typeof activeChart.getAllStudies === 'function'),
        has_getAllShapes: !!(activeChart && typeof activeChart.getAllShapes === 'function'),
        document_title: document.title || null
      }
    });
  } catch (e) {
    return JSON.stringify({ error: String(e) });
  }
})()
"#;

/// Write path: change the active chart's symbol.
///
/// The expression placeholder `__SYMBOL__` is substituted by
/// `tv_cdp_set_symbol` at call time using `serde_json::to_string` so
/// the value is safely JSON-encoded (handles quotes, backslashes, and
/// non-ASCII). Substitution at the Rust layer rather than the JS layer
/// means the LLM-provided symbol string never reaches V8 unescaped.
const JS_SET_SYMBOL: &str = r#"
(() => {
  try {
    const tv = window.tradingViewApi || window.TradingView || {};
    const chartWidget = window.chartWidget || (typeof tv.chart === 'function' ? tv.chart() : null);
    const activeChart = (chartWidget && typeof chartWidget.activeChart === 'function')
      ? chartWidget.activeChart()
      : null;
    if (!activeChart || typeof activeChart.setSymbol !== 'function') {
      return JSON.stringify({ ok: false, error: 'activeChart.setSymbol unavailable' });
    }
    activeChart.setSymbol(__SYMBOL__);
    return JSON.stringify({ ok: true, symbol: __SYMBOL__ });
  } catch (e) {
    return JSON.stringify({ ok: false, error: String(e) });
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
    let indicators = parsed
        .get("indicators")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|i| TvIndicatorSummary {
                    id: i.get("id").and_then(|x| x.as_str()).map(str::to_string),
                    name: i.get("name").and_then(|x| x.as_str()).map(str::to_string),
                })
                .collect::<Vec<_>>()
        });
    let shapes = parsed
        .get("shapes")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|i| TvShapeSummary {
                    id: i.get("id").and_then(|x| x.as_str()).map(str::to_string),
                    name: i.get("name").and_then(|x| x.as_str()).map(str::to_string),
                })
                .collect::<Vec<_>>()
        });
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
        indicators,
        shapes,
        alert_count: parsed
            .get("alert_count")
            .and_then(|v| v.as_u64())
            .map(|n| n as u32),
        raw: parsed,
    })
}

/// Write the active chart's symbol. Returns `{ok, symbol, error}`.
/// Symbol is JSON-encoded at the Rust layer so it can't break out of
/// the JS expression — defensive against any LLM-controlled value.
#[tauri::command]
pub async fn tv_cdp_set_symbol(
    state: tauri::State<'_, TvCdpState>,
    symbol: String,
) -> Result<TvSetSymbolResult, String> {
    // Trim + length-cap on the Rust side. TV's own symbol parser handles
    // exchange prefixes (`NASDAQ:AAPL`, `CME_MINI:NQ1!`), but a 400-char
    // payload is almost certainly an LLM hallucination not a real ticker.
    let trimmed = symbol.trim();
    if trimmed.is_empty() {
        return Err("symbol must not be empty".to_string());
    }
    if trimmed.len() > 64 {
        return Err(format!(
            "symbol too long ({} > 64); refusing to set",
            trimmed.len()
        ));
    }
    let encoded = serde_json::to_string(trimmed).map_err(|e| format!("encode: {e}"))?;
    let expr = JS_SET_SYMBOL.replace("__SYMBOL__", &encoded);
    let raw = tv_cdp_eval(state, expr).await?;
    let parsed: Value = match &raw {
        Value::String(s) => serde_json::from_str(s).unwrap_or(Value::Null),
        other => other.clone(),
    };
    Ok(TvSetSymbolResult {
        ok: parsed.get("ok").and_then(|v| v.as_bool()).unwrap_or(false),
        symbol: parsed
            .get("symbol")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        error: parsed
            .get("error")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    })
}

/// Best-effort: find TradingView Desktop's installed exe on Windows
/// and spawn it with `--remote-debugging-port=<port>`. Eliminates the
/// "edit your shortcut" step from the onboarding wizard for the most
/// common install layout. macOS / Linux paths land later; on those
/// platforms today this returns `launched: false` with an error.
#[tauri::command]
pub async fn tv_cdp_launch_tv(port: Option<u16>) -> Result<TvLaunchResult, String> {
    let port = port.unwrap_or(DEFAULT_TV_CDP_PORT);
    #[cfg(target_os = "windows")]
    {
        let path = find_tv_exe_windows();
        match path {
            Some(p) => {
                let display = p.display().to_string();
                let arg = format!("--remote-debugging-port={port}");
                match std::process::Command::new(&p).arg(&arg).spawn() {
                    Ok(_child) => Ok(TvLaunchResult {
                        launched: true,
                        path: Some(display),
                        port,
                        error: None,
                    }),
                    Err(e) => Ok(TvLaunchResult {
                        launched: false,
                        path: Some(display),
                        port,
                        error: Some(format!("spawn failed: {e}")),
                    }),
                }
            }
            None => Ok(TvLaunchResult {
                launched: false,
                path: None,
                port,
                error: Some("TradingView.exe not found in common install paths".to_string()),
            }),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = port;
        Ok(TvLaunchResult {
            launched: false,
            path: None,
            port,
            error: Some("auto-launch is Windows-only in v1".to_string()),
        })
    }
}

/// Search the common Windows install paths for `TradingView.exe`.
/// Returns the first hit, or `None` if no install is found.
#[cfg(target_os = "windows")]
fn find_tv_exe_windows() -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let mut candidates: Vec<PathBuf> = Vec::new();

    // Per-user Squirrel install (the modern default). Pattern:
    // %LOCALAPPDATA%\Programs\TradingView\TradingView.exe
    if let Some(localappdata) = std::env::var_os("LOCALAPPDATA") {
        let base = PathBuf::from(&localappdata);
        candidates.push(
            base.join("Programs")
                .join("TradingView")
                .join("TradingView.exe"),
        );
        candidates.push(base.join("TradingView").join("TradingView.exe"));
    }
    // Machine-wide install variants.
    if let Some(pf) = std::env::var_os("ProgramFiles") {
        candidates.push(
            PathBuf::from(&pf)
                .join("TradingView")
                .join("TradingView.exe"),
        );
    }
    if let Some(pfx86) = std::env::var_os("ProgramFiles(x86)") {
        candidates.push(
            PathBuf::from(&pfx86)
                .join("TradingView")
                .join("TradingView.exe"),
        );
    }

    candidates.into_iter().find(|p| p.is_file())
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
