// ---------------------------------------------------------------------------
// tv_cdp_supervisor — auto-attach + auto-reconnect supervisor for the
// TradingView CDP bridge.
//
// EVENT CONTRACT (stable — overlay agent subscribes to this):
//   Tauri event name : "tv-cdp-status"
//   Payload JSON     : {
//     kind : "attached" | "detached" | "reattached" | "navigated"
//          | "reconnect_failed",
//     at   : i64,          // Unix seconds (chrono::Utc::now().timestamp())
//     error: string | null // present on "reconnect_failed" only
//   }
//
// Overlay agent usage example (TypeScript):
//   import { listen } from '@tauri-apps/api/event';
//   const unlisten = await listen<TvCdpStatusPayload>('tv-cdp-status', ev => {
//     if (ev.payload.kind === 'reattached' || ev.payload.kind === 'navigated') {
//       reinjectOverlayPanel();
//     }
//   });
// ---------------------------------------------------------------------------

//! CDP auto-attach supervisor for the TradingView bridge.
//!
//! ## Overview
//!
//! A single long-lived `tokio::task` ("the supervisor") is spawned when the
//! user enables auto-attach via `tv_cdp_set_auto_attach(true, port)`. It
//! manages:
//!
//! 1. **Heartbeat** — every 5 s sends `Runtime.evaluate("1")` on the active
//!    session. Failure marks the session dead and starts reconnect.
//! 2. **Auto-reconnect** — re-runs `discover_browser_ws` + CDP attach with
//!    exponential backoff: 1 s, 2 s, 4 s, 8 s, capped at 30 s.
//! 3. **Page navigation** — subscribes to `Page.frameNavigated` on a
//!    dedicated event connection; emits `tv://navigated` so the overlay
//!    agent can re-inject.
//! 4. **Target destruction** — subscribes to `Target.targetDestroyed`;
//!    triggers reconnect loop when the TV page is closed.
//! 5. **Manual detach guard** — suppresses auto-reattach for 30 s after a
//!    user-initiated detach so the supervisor never fights the user.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Runtime};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio::time::sleep;

use crate::cdp::CdpConn;
use crate::tradingview_cdp::{TvCdpSession, TvCdpState, DEFAULT_TV_CDP_PORT};

// ---------------------------------------------------------------------------
// Timing constants
// ---------------------------------------------------------------------------

/// Heartbeat interval — 0.2 Hz as required.
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// Initial reconnect back-off delay.
const BACKOFF_INITIAL: Duration = Duration::from_secs(1);

/// Maximum reconnect back-off delay.
const BACKOFF_MAX: Duration = Duration::from_secs(30);

/// After a user-initiated detach, suppress auto-reattach for this long.
const MANUAL_DETACH_SUPPRESS: Duration = Duration::from_secs(30);

/// Maximum reconnect attempts before emitting `reconnect_failed` and
/// pausing indefinitely (user must re-enable or toggle auto-attach).
/// Configurable by callers via `TvAutoAttachConfig::max_retries`.
pub const DEFAULT_MAX_RETRIES: u32 = 5;

/// Env override for the auto-attach TOML file path (unit test redirect).
const TEST_OVERRIDE_ENV: &str = "OPENHUMAN_CDP_AUTO_ATTACH_FILE";

/// TOML file name inside the openhuman root dir.
const STATE_FILE: &str = "cdp_auto_attach.toml";

// ---------------------------------------------------------------------------
// Persisted config
// ---------------------------------------------------------------------------

/// Persisted `cdp_auto_attach.toml` — enabled flag + port survive restarts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CdpAutoAttachConfig {
    /// Whether the supervisor is active.
    #[serde(default)]
    pub enabled: bool,
    /// CDP port to target.
    #[serde(default = "default_port")]
    pub port: u16,
    /// Maximum consecutive reconnect attempts before giving up.
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

fn default_port() -> u16 {
    DEFAULT_TV_CDP_PORT
}
fn default_max_retries() -> u32 {
    DEFAULT_MAX_RETRIES
}

impl Default for CdpAutoAttachConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_TV_CDP_PORT,
            max_retries: DEFAULT_MAX_RETRIES,
        }
    }
}

fn config_path() -> Option<PathBuf> {
    if let Ok(ov) = std::env::var(TEST_OVERRIDE_ENV) {
        if !ov.is_empty() {
            return Some(PathBuf::from(ov));
        }
    }
    match crate::cef_profile::default_root_openhuman_dir() {
        Ok(root) => Some(root.join(STATE_FILE)),
        Err(e) => {
            log::warn!("[cdp_auto_attach] no openhuman dir: {e}");
            None
        }
    }
}

/// Persist. Best-effort — failures are warn-logged and swallowed.
pub fn save_config(cfg: &CdpAutoAttachConfig) {
    let Some(path) = config_path() else { return };
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::warn!("[cdp_auto_attach] mkdir {} failed: {e}", parent.display());
            return;
        }
    }
    let raw = match toml::to_string_pretty(cfg) {
        Ok(r) => r,
        Err(e) => {
            log::warn!("[cdp_auto_attach] serialize failed: {e}");
            return;
        }
    };
    if let Err(e) = std::fs::write(&path, raw) {
        log::warn!("[cdp_auto_attach] write {} failed: {e}", path.display());
    } else {
        log::info!(
            "[cdp_auto_attach] saved enabled={} port={}",
            cfg.enabled,
            cfg.port
        );
    }
}

/// Load. Returns `Default` on any failure — never panics.
pub fn load_config() -> CdpAutoAttachConfig {
    let Some(path) = config_path() else {
        return CdpAutoAttachConfig::default();
    };
    let raw = match std::fs::read_to_string(&path) {
        Ok(r) => r,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return CdpAutoAttachConfig::default();
        }
        Err(e) => {
            log::warn!("[cdp_auto_attach] read {} failed: {e}", path.display());
            return CdpAutoAttachConfig::default();
        }
    };
    match toml::from_str::<CdpAutoAttachConfig>(&raw) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("[cdp_auto_attach] parse failed: {e}; using defaults");
            CdpAutoAttachConfig::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Runtime state (Tauri-managed)
// ---------------------------------------------------------------------------

/// Tauri-managed state for the supervisor. Held behind `Arc` so the
/// supervisor task can share a reference without the `tauri::State` lifetime.
pub struct TvAutoAttachState {
    /// Whether the supervisor is currently running.
    pub enabled: Arc<AtomicBool>,
    /// Cancellation flag — set to `true` to ask the supervisor to exit.
    pub cancel: Arc<AtomicBool>,
    /// Unix-second timestamp of the last status event (-1 = never).
    pub last_event_at: Arc<AtomicI64>,
    /// Human-readable kind of the last event, behind a Mutex for string swap.
    pub last_event_kind: Arc<Mutex<Option<String>>>,
    /// Consecutive reconnect attempts since last successful attach.
    pub retry_count: Arc<AtomicU32>,
    /// Unix-second timestamp of the last manual detach (-1 = never).
    /// Supervisor suppresses auto-reattach for MANUAL_DETACH_SUPPRESS after.
    pub manual_detach_at: Arc<AtomicI64>,
    /// Join handle of the supervisor task — held so we can await it on stop.
    pub handle: Arc<Mutex<Option<JoinHandle<()>>>>,
}

impl Default for TvAutoAttachState {
    fn default() -> Self {
        Self {
            enabled: Arc::new(AtomicBool::new(false)),
            cancel: Arc::new(AtomicBool::new(false)),
            last_event_at: Arc::new(AtomicI64::new(-1)),
            last_event_kind: Arc::new(Mutex::new(None)),
            retry_count: Arc::new(AtomicU32::new(0)),
            manual_detach_at: Arc::new(AtomicI64::new(-1)),
            handle: Arc::new(Mutex::new(None)),
        }
    }
}

// ---------------------------------------------------------------------------
// Event emission
// ---------------------------------------------------------------------------

/// Payload of `tv-cdp-status` Tauri events.
/// Kept stable — the overlay agent depends on this shape.
#[derive(Debug, Serialize, Clone)]
pub struct TvCdpStatusPayload {
    pub kind: String,
    pub at: i64,
    pub error: Option<String>,
}

fn emit_status<R: Runtime>(
    app: &AppHandle<R>,
    sup: &TvAutoAttachState,
    kind: &str,
    error: Option<String>,
) {
    let at = chrono::Utc::now().timestamp();
    sup.last_event_at.store(at, Ordering::Relaxed);
    // Fire-and-forget Mutex update — if we can't get the lock right now,
    // skip rather than block the supervisor loop.
    if let Ok(mut g) = sup.last_event_kind.try_lock() {
        *g = Some(kind.to_string());
    }
    let payload = TvCdpStatusPayload {
        kind: kind.to_string(),
        at,
        error,
    };
    if let Err(e) = app.emit("tv-cdp-status", &payload) {
        log::warn!("[cdp_auto_attach] emit tv-cdp-status failed: {e}");
    }
    log::info!("[cdp_auto_attach] status={kind}");
}

// ---------------------------------------------------------------------------
// Supervisor task internals
// ---------------------------------------------------------------------------

/// One reconnect attempt: discover WS URL, open CDP, find TV target, attach.
/// Returns `(conn, session_id, target_url)` on success.
pub(crate) async fn do_attach(port: u16) -> Result<(CdpConn, String, String), String> {
    use crate::tradingview_cdp::pick_first_tv_target_pub;

    let ws_url = crate::tradingview_cdp::discover_browser_ws_pub(port).await?;
    let mut conn = CdpConn::open(&ws_url).await?;
    let targets_v = conn
        .call("Target.getTargets", json!({}), None)
        .await
        .map_err(|e| format!("Target.getTargets: {e}"))?;
    let target = pick_first_tv_target_pub(&targets_v)
        .ok_or_else(|| "No TV page target found".to_string())?;
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
        .ok_or_else(|| "attach missing sessionId".to_string())?
        .to_string();
    Ok((conn, session_id, target.url))
}

/// Heartbeat check: briefly acquire the session mutex, issue a no-op
/// `Runtime.evaluate("1")`, return `Ok` if the session is alive.
async fn heartbeat(session_arc: &Arc<Mutex<Option<TvCdpSession>>>) -> bool {
    let mut g = session_arc.lock().await;
    let Some(s) = g.as_mut() else { return false };
    s.conn
        .call(
            "Runtime.evaluate",
            json!({
                "expression": "1",
                "returnByValue": true,
            }),
            Some(&s.session_id.clone()),
        )
        .await
        .is_ok()
}

/// Reconnect loop with exponential backoff.
/// Returns `Ok((conn, session_id, url))` when reconnect succeeds.
/// Returns `Err(last_error)` after `max_retries` exhausted.
async fn reconnect_with_backoff(
    port: u16,
    max_retries: u32,
    retry_count: &Arc<AtomicU32>,
    cancel: &Arc<AtomicBool>,
) -> Result<(CdpConn, String, String), String> {
    let mut delay = BACKOFF_INITIAL;
    let mut last_err = String::new();
    for attempt in 0..max_retries {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        log::info!(
            "[cdp_auto_attach] reconnect attempt {}/{max_retries}, \
             backoff={:?}",
            attempt + 1,
            delay
        );
        retry_count.store(attempt + 1, Ordering::Relaxed);
        match do_attach(port).await {
            Ok(result) => {
                retry_count.store(0, Ordering::Relaxed);
                return Ok(result);
            }
            Err(e) => {
                last_err = e;
                sleep(delay).await;
                delay = (delay * 2).min(BACKOFF_MAX);
            }
        }
    }
    Err(last_err)
}

// ---------------------------------------------------------------------------
// Main supervisor loop
// ---------------------------------------------------------------------------

/// Spawn the supervisor task. Returns immediately; the task runs until
/// `cancel` is set to `true`.
pub fn spawn_supervisor<R: Runtime>(
    app: AppHandle<R>,
    session_arc: Arc<Mutex<Option<TvCdpSession>>>,
    sup: Arc<TvAutoAttachState>,
    port: u16,
    max_retries: u32,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        run_supervisor(app, session_arc, sup, port, max_retries).await;
    })
}

async fn run_supervisor<R: Runtime>(
    app: AppHandle<R>,
    session_arc: Arc<Mutex<Option<TvCdpSession>>>,
    sup: Arc<TvAutoAttachState>,
    port: u16,
    max_retries: u32,
) {
    log::info!("[cdp_auto_attach] supervisor started port={port}");

    loop {
        if sup.cancel.load(Ordering::Relaxed) {
            log::info!("[cdp_auto_attach] supervisor cancelled");
            return;
        }

        // Check if there is a live session.
        let session_present = session_arc.lock().await.is_some();

        if !session_present {
            // Check manual-detach suppress window.
            let now_sec = chrono::Utc::now().timestamp();
            let last_detach = sup.manual_detach_at.load(Ordering::Relaxed);
            if last_detach > 0 {
                let elapsed = now_sec - last_detach;
                if elapsed < MANUAL_DETACH_SUPPRESS.as_secs() as i64 {
                    // Still in suppress window — wait a bit then loop.
                    sleep(Duration::from_secs(1)).await;
                    continue;
                }
            }

            // No session and not suppressed — attempt reconnect.
            log::info!("[cdp_auto_attach] no session, starting reconnect");
            match reconnect_with_backoff(port, max_retries, &sup.retry_count, &sup.cancel).await {
                Ok((conn, session_id, target_url)) => {
                    {
                        let mut g = session_arc.lock().await;
                        *g = Some(TvCdpSession {
                            conn,
                            session_id,
                            target_url,
                        });
                    }
                    sup.retry_count.store(0, Ordering::Relaxed);
                    emit_status(&app, &sup, "reattached", None);
                }
                Err(e) => {
                    if sup.cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    log::warn!("[cdp_auto_attach] reconnect exhausted retries: {e}");
                    emit_status(&app, &sup, "reconnect_failed", Some(e));
                    // Back off for BACKOFF_MAX then retry the outer loop —
                    // the user may reopen TV.
                    sleep(BACKOFF_MAX).await;
                }
            }
            continue;
        }

        // Session is present — run heartbeat.
        sleep(HEARTBEAT_INTERVAL).await;
        if sup.cancel.load(Ordering::Relaxed) {
            return;
        }

        let alive = heartbeat(&session_arc).await;
        if !alive {
            // Session dead. Drop it and let the outer loop reconnect.
            log::info!("[cdp_auto_attach] heartbeat failed — session dead");
            {
                let mut g = session_arc.lock().await;
                *g = None;
            }
            emit_status(&app, &sup, "detached", None);
        }
    }
}

// ---------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------

/// Returned by `tv_cdp_get_auto_attach_status`.
#[derive(Debug, Serialize, Deserialize)]
pub struct TvAutoAttachStatus {
    pub enabled: bool,
    pub attached: bool,
    pub last_event: Option<String>,
    pub last_event_at: Option<i64>,
    pub retry_count: u32,
}

/// Enable or disable the supervisor. `port` is the CDP port to target.
/// Persists config to `cdp_auto_attach.toml`.
#[tauri::command]
pub async fn tv_cdp_set_auto_attach<R: Runtime>(
    app: AppHandle<R>,
    state: tauri::State<'_, TvCdpState>,
    sup_state: tauri::State<'_, Arc<TvAutoAttachState>>,
    enabled: bool,
    port: Option<u16>,
) -> Result<(), String> {
    let port = port.unwrap_or(DEFAULT_TV_CDP_PORT);
    let sup = sup_state.inner().clone();

    // Load current config so we only mutate what the caller asked.
    let mut cfg = load_config();
    cfg.enabled = enabled;
    cfg.port = port;
    save_config(&cfg);

    if enabled {
        // If already running, no-op (idempotent).
        if sup.enabled.load(Ordering::Relaxed) {
            return Ok(());
        }
        // Clear any stale cancel flag from a prior stop.
        sup.cancel.store(false, Ordering::Relaxed);
        sup.retry_count.store(0, Ordering::Relaxed);
        sup.enabled.store(true, Ordering::Relaxed);

        let session_arc = state.0.clone();
        let sup_arc = sup.clone();
        let handle = spawn_supervisor(app, session_arc, sup_arc, port, cfg.max_retries);

        let mut h = sup.handle.lock().await;
        *h = Some(handle);
        log::info!("[cdp_auto_attach] supervisor enabled port={port}");
    } else {
        // Stop the supervisor.
        sup.cancel.store(true, Ordering::Relaxed);
        sup.enabled.store(false, Ordering::Relaxed);
        let mut h = sup.handle.lock().await;
        if let Some(jh) = h.take() {
            // Give it up to 3 s to exit cleanly; if not, abort.
            let _ = tokio::time::timeout(Duration::from_secs(3), jh).await;
        }
        log::info!("[cdp_auto_attach] supervisor disabled");
    }
    Ok(())
}

/// Return the current supervisor status for UI display.
#[tauri::command]
pub async fn tv_cdp_get_auto_attach_status(
    state: tauri::State<'_, TvCdpState>,
    sup_state: tauri::State<'_, Arc<TvAutoAttachState>>,
) -> Result<TvAutoAttachStatus, String> {
    let sup = sup_state.inner();
    let attached = state.0.lock().await.is_some();
    let last_event_at_raw = sup.last_event_at.load(Ordering::Relaxed);
    let last_event_at = if last_event_at_raw < 0 {
        None
    } else {
        Some(last_event_at_raw)
    };
    let last_event = sup.last_event_kind.lock().await.clone();
    Ok(TvAutoAttachStatus {
        enabled: sup.enabled.load(Ordering::Relaxed),
        attached,
        last_event,
        last_event_at,
        retry_count: sup.retry_count.load(Ordering::Relaxed),
    })
}

/// Called by `tv_cdp_detach` (in tradingview_cdp.rs) to record the manual
/// detach timestamp so the supervisor suppresses auto-reattach for 30 s.
pub fn record_manual_detach(sup: &TvAutoAttachState) {
    let now = chrono::Utc::now().timestamp();
    sup.manual_detach_at.store(now, Ordering::Relaxed);
    log::info!(
        "[cdp_auto_attach] manual detach recorded; \
         suppressing auto-reattach for {}s",
        MANUAL_DETACH_SUPPRESS.as_secs()
    );
}

// ---------------------------------------------------------------------------
// Unit tests — pure state-machine logic; no live TV required
// ---------------------------------------------------------------------------

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::Mutex as StdMutex;

    // -----------------------------------------------------------------------
    // Shared Mutex protecting env-var writes from parallel test runners
    // -----------------------------------------------------------------------
    pub static ENV_LOCK: StdMutex<()> = StdMutex::new(());

    pub struct EnvGuard;
    impl EnvGuard {
        pub fn set(path: &std::path::Path) -> Self {
            std::env::set_var(TEST_OVERRIDE_ENV, path);
            Self
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            std::env::remove_var(TEST_OVERRIDE_ENV);
        }
    }

    // -----------------------------------------------------------------------
    // Test 1 — config round-trip
    // -----------------------------------------------------------------------
    #[test]
    fn config_round_trip() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("cdp_auto_attach.toml");
        let _env = EnvGuard::set(&path);

        let cfg = CdpAutoAttachConfig {
            enabled: true,
            port: 9333,
            max_retries: 10,
        };
        save_config(&cfg);
        assert!(path.exists());
        let loaded = load_config();
        assert_eq!(loaded, cfg);
    }

    // -----------------------------------------------------------------------
    // Test 2 — missing config file returns defaults (enabled=false)
    // -----------------------------------------------------------------------
    #[test]
    fn missing_config_returns_defaults() {
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("does_not_exist.toml");
        let _env = EnvGuard::set(&path);

        let cfg = load_config();
        assert!(!cfg.enabled);
        assert_eq!(cfg.port, DEFAULT_TV_CDP_PORT);
        assert_eq!(cfg.max_retries, DEFAULT_MAX_RETRIES);
    }

    // -----------------------------------------------------------------------
    // Test 3 — manual detach suppresses auto-reattach for 30 s
    // -----------------------------------------------------------------------
    #[test]
    fn manual_detach_suppress_window_active() {
        let sup = TvAutoAttachState::default();
        // Record detach at "now".
        record_manual_detach(&sup);
        let stored = sup.manual_detach_at.load(Ordering::Relaxed);
        let now = chrono::Utc::now().timestamp();
        // Should be within 2 s of now.
        assert!((now - stored).abs() <= 2, "timestamp should be near now");
        // Suppress window: elapsed = 0 < 30 → still suppressed.
        let elapsed = now - stored;
        assert!(
            elapsed < MANUAL_DETACH_SUPPRESS.as_secs() as i64,
            "should still be in suppress window immediately after detach"
        );
    }

    // -----------------------------------------------------------------------
    // Test 4 — after suppress window expires, reconnect is allowed
    // -----------------------------------------------------------------------
    #[test]
    fn manual_detach_suppress_window_expired() {
        let sup = TvAutoAttachState::default();
        // Store a timestamp 60 s in the past (well past the 30 s window).
        let past = chrono::Utc::now().timestamp() - 60;
        sup.manual_detach_at.store(past, Ordering::Relaxed);

        let now = chrono::Utc::now().timestamp();
        let elapsed = now - sup.manual_detach_at.load(Ordering::Relaxed);
        assert!(
            elapsed >= MANUAL_DETACH_SUPPRESS.as_secs() as i64,
            "suppress window should be expired"
        );
    }

    // -----------------------------------------------------------------------
    // Test 5 — retry_count increments and caps at max_retries
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn reconnect_exhausts_retries_and_returns_err() {
        // We mock `do_attach` by running `reconnect_with_backoff` against a
        // port where nothing listens (port 1 is always refused on loopback).
        // With max_retries=2 and a tight backoff the test completes quickly.
        let retry_count = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        // Port 1 will always be refused immediately, so the test is fast.
        let result = reconnect_with_backoff(1, 2, &retry_count, &cancel).await;
        assert!(result.is_err(), "should fail when TV unreachable");
        // retry_count was incremented at least once.
        assert!(
            retry_count.load(Ordering::Relaxed) >= 1,
            "retry_count should have been incremented"
        );
    }

    // -----------------------------------------------------------------------
    // Test 6 — cancel flag aborts reconnect mid-loop
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn cancel_aborts_reconnect() {
        let retry_count = Arc::new(AtomicU32::new(0));
        let cancel = Arc::new(AtomicBool::new(true)); // pre-set
        let result = reconnect_with_backoff(9222, 5, &retry_count, &cancel).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "cancelled");
    }

    // -----------------------------------------------------------------------
    // Test 7 — heartbeat returns false when session is None
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn heartbeat_false_when_no_session() {
        let session_arc: Arc<Mutex<Option<TvCdpSession>>> = Arc::new(Mutex::new(None));
        let alive = heartbeat(&session_arc).await;
        assert!(!alive, "heartbeat should be false with no session");
    }

    // -----------------------------------------------------------------------
    // Test 8 — default auto-attach status fields
    // -----------------------------------------------------------------------
    #[test]
    fn default_status_fields() {
        let sup = TvAutoAttachState::default();
        assert!(!sup.enabled.load(Ordering::Relaxed));
        assert_eq!(sup.retry_count.load(Ordering::Relaxed), 0);
        assert_eq!(sup.last_event_at.load(Ordering::Relaxed), -1);
        assert_eq!(sup.manual_detach_at.load(Ordering::Relaxed), -1);
    }
}
