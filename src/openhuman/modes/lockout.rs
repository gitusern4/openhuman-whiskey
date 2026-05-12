//! Walk-away lockout — persisted state machine.
//!
//! Persists to `<openhuman_dir>/tks_lockout.toml` so an app restart
//! cannot dodge an active lockout. The design intent: the user pre-
//! commits their own risk limits; the lockout enforces them even when
//! willpower runs out.
//!
//! # State machine
//! ```text
//! Unlocked
//!   │  trip(reason) or daily_loss >= max_daily_loss
//!   │  or consecutive_losses >= max_consecutive_losses
//!   ▼
//! Locked (locked_until timestamp stored)
//!   │  lockout_until <= now()  OR  force_reset()
//!   ▼
//! Unlocked
//! ```
//!
//! `force_reset` requires a deliberate second call — the UI gates it
//! behind an explicit toggle (`force_reset_armed`) so a misclick cannot
//! clear an active lockout.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const STATE_FILE: &str = "tks_lockout.toml";
const TEST_OVERRIDE_ENV: &str = "OPENHUMAN_TKS_LOCKOUT_FILE";

// ---------------------------------------------------------------------------
// Config sub-struct (the thresholds the user sets once)
// ---------------------------------------------------------------------------

/// User-chosen lockout thresholds, stored alongside state so a single
/// file round-trip can check everything.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockoutConfig {
    /// Trip lockout when cumulative daily loss hits this amount ($).
    /// `None` means "no daily-loss limit".
    #[serde(default)]
    pub max_daily_loss_dollars: Option<f64>,

    /// Trip lockout when consecutive losses hit this count.
    /// `None` means "no consecutive-loss limit".
    #[serde(default)]
    pub max_consecutive_losses: Option<u32>,

    /// How many minutes the lockout lasts after being tripped.
    #[serde(default = "default_cooldown_minutes")]
    pub cooldown_minutes: u32,
}

fn default_cooldown_minutes() -> u32 {
    60
}

impl Default for LockoutConfig {
    fn default() -> Self {
        Self {
            max_daily_loss_dollars: None,
            max_consecutive_losses: None,
            cooldown_minutes: default_cooldown_minutes(),
        }
    }
}

// ---------------------------------------------------------------------------
// Full persisted state
// ---------------------------------------------------------------------------

/// The full TOML record stored to disk.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockoutState {
    // Config (thresholds).
    #[serde(flatten)]
    pub config: LockoutConfig,

    // Running counters — reset when the user explicitly clears them or
    // when a new trading day begins (caller's responsibility).
    #[serde(default)]
    pub daily_loss_dollars: f64,

    #[serde(default)]
    pub consecutive_losses: u32,

    /// Unix timestamp (seconds) until which the lockout is active.
    /// `None` means unlocked.
    #[serde(default)]
    pub locked_until_unix: Option<u64>,

    /// Human-readable reason the lockout was tripped.
    #[serde(default)]
    pub lock_reason: Option<String>,

    /// Server-side armed-reset timestamp. When the user clicks "arm
    /// force-reset" the system records this Unix-seconds + 300 (5
    /// min). `request_force_reset` will only honor the reset after
    /// that timestamp passes. Defends against DevTools-IPC bypass:
    /// a tilted trader calling `invoke('lockout_reset')` first has
    /// to arm + wait through the cooldown. Architect review
    /// 2026-05-12.
    #[serde(default)]
    pub armed_for_reset_until: Option<u64>,
}

impl Default for LockoutState {
    fn default() -> Self {
        Self {
            config: LockoutConfig::default(),
            daily_loss_dollars: 0.0,
            consecutive_losses: 0,
            locked_until_unix: None,
            lock_reason: None,
            armed_for_reset_until: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Serialisable result returned to the frontend via Tauri commands
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LockoutStatus {
    pub is_locked: bool,
    pub locked_until_unix: Option<u64>,
    pub lock_reason: Option<String>,
    pub daily_loss_dollars: f64,
    pub consecutive_losses: u32,
    pub config: LockoutConfig,
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

fn state_path() -> Option<PathBuf> {
    if let Ok(ov) = std::env::var(TEST_OVERRIDE_ENV) {
        if !ov.is_empty() {
            return Some(PathBuf::from(ov));
        }
    }
    match crate::openhuman::config::default_root_openhuman_dir() {
        Ok(root) => Some(root.join(STATE_FILE)),
        Err(err) => {
            log::warn!("[lockout] no openhuman dir: {err}");
            None
        }
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ---------------------------------------------------------------------------
// Persistence
// ---------------------------------------------------------------------------

pub fn save(state: &LockoutState) {
    let Some(path) = state_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            log::warn!(
                "[lockout] mkdir {} failed: {err}; skip save",
                parent.display()
            );
            return;
        }
    }
    let raw = match toml::to_string_pretty(state) {
        Ok(r) => r,
        Err(err) => {
            log::warn!("[lockout] serialize failed: {err}");
            return;
        }
    };
    if let Err(err) = std::fs::write(&path, raw) {
        log::warn!("[lockout] write {} failed: {err}", path.display());
    }
}

pub fn load() -> LockoutState {
    let Some(path) = state_path() else {
        return LockoutState::default();
    };
    let raw = match std::fs::read_to_string(&path) {
        Ok(r) => r,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return LockoutState::default();
        }
        Err(err) => {
            log::warn!("[lockout] read {} failed: {err}", path.display());
            return LockoutState::default();
        }
    };
    match toml::from_str::<LockoutState>(&raw) {
        Ok(s) => s,
        Err(err) => {
            log::warn!("[lockout] parse failed: {err}; using defaults");
            LockoutState::default()
        }
    }
}

// ---------------------------------------------------------------------------
// State machine operations
// ---------------------------------------------------------------------------

/// Returns true if currently locked (checks the timestamp against now).
pub fn is_locked(state: &LockoutState) -> bool {
    match state.locked_until_unix {
        None => false,
        Some(until) => until > now_unix(),
    }
}

/// Trip the lockout for `cooldown_minutes` from now with a human-readable
/// reason. Saves to disk.
pub fn trip(state: &mut LockoutState, reason: &str) {
    let cooldown_secs = u64::from(state.config.cooldown_minutes) * 60;
    state.locked_until_unix = Some(now_unix() + cooldown_secs);
    state.lock_reason = Some(reason.to_string());
    save(state);
}

/// Force-reset an active lockout — DEPRECATED unchecked variant.
///
/// Use [`request_force_reset`] instead. This function unconditionally
/// clears the lockout, which means a tilted trader who calls the
/// Tauri IPC from DevTools console (`invoke('lockout_reset')`)
/// trivially bypasses the entire lockout feature. The architect-
/// review 2026-05-12 flagged this as the lockout feature literally
/// not locking anyone out.
///
/// Retained for compatibility with existing tests + the
/// `lockout_reset` Tauri command which now routes through
/// [`request_force_reset`]. New callers MUST use that.
pub fn force_reset(state: &mut LockoutState) {
    state.locked_until_unix = None;
    state.lock_reason = None;
    state.armed_for_reset_until = None;
    save(state);
}

/// Arm a 5-minute window during which `request_force_reset` will
/// actually clear the lockout. Calling this starts the timer; the
/// user must wait through it before the reset is honored. A tilted
/// trader who calls `arm_force_reset` impulsively must then sit
/// idle for 5 minutes before they can complete the bypass — that
/// pause is the discipline.
pub fn arm_force_reset(state: &mut LockoutState) {
    state.armed_for_reset_until = Some(now_unix() + 5 * 60);
    save(state);
}

/// Check whether the reset is armed AND the 5-minute window has
/// already elapsed. If yes, force-reset; otherwise return an
/// instructive error including the seconds remaining. The pure-
/// function return value lets the UI render a countdown without
/// re-querying.
pub fn request_force_reset(state: &mut LockoutState) -> Result<(), String> {
    match state.armed_for_reset_until {
        None => Err(
            "Reset not armed. Call arm_force_reset and wait 5 minutes before requesting reset."
                .to_string(),
        ),
        Some(t) => {
            let now = now_unix();
            if now < t {
                Err(format!(
                    "Reset armed but cooldown active. {} seconds remaining.",
                    t - now
                ))
            } else {
                force_reset(state);
                Ok(())
            }
        }
    }
}

/// Record a loss event. Updates running counters and checks whether a
/// threshold is now breached. Returns `true` if a lockout was tripped.
pub fn record_loss(state: &mut LockoutState, loss_dollars: f64) -> bool {
    state.daily_loss_dollars += loss_dollars.abs();
    state.consecutive_losses += 1;

    // Check daily loss threshold.
    if let Some(max_dl) = state.config.max_daily_loss_dollars {
        if state.daily_loss_dollars >= max_dl {
            let reason = format!(
                "Daily loss limit ${:.2} reached (total today: ${:.2}).",
                max_dl, state.daily_loss_dollars
            );
            trip(state, &reason);
            return true;
        }
    }

    // Check consecutive loss threshold.
    if let Some(max_cl) = state.config.max_consecutive_losses {
        if state.consecutive_losses >= max_cl {
            let reason = format!(
                "Consecutive loss limit {} reached ({} losses in a row).",
                max_cl, state.consecutive_losses
            );
            trip(state, &reason);
            return true;
        }
    }

    save(state);
    false
}

/// Reset the running counters (call at start of new trading day).
pub fn reset_counters(state: &mut LockoutState) {
    state.daily_loss_dollars = 0.0;
    state.consecutive_losses = 0;
    save(state);
}

/// Derive the `LockoutStatus` view returned to the frontend.
pub fn status(state: &LockoutState) -> LockoutStatus {
    LockoutStatus {
        is_locked: is_locked(state),
        locked_until_unix: state.locked_until_unix,
        lock_reason: state.lock_reason.clone(),
        daily_loss_dollars: state.daily_loss_dollars,
        consecutive_losses: state.consecutive_losses,
        config: state.config.clone(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static TEST_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard;
    impl EnvGuard {
        fn set(path: &std::path::Path) -> Self {
            std::env::set_var(TEST_OVERRIDE_ENV, path);
            Self
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            std::env::remove_var(TEST_OVERRIDE_ENV);
        }
    }

    fn make_state(max_dl: Option<f64>, max_cl: Option<u32>, cooldown: u32) -> LockoutState {
        LockoutState {
            config: LockoutConfig {
                max_daily_loss_dollars: max_dl,
                max_consecutive_losses: max_cl,
                cooldown_minutes: cooldown,
            },
            ..Default::default()
        }
    }

    #[test]
    fn manual_trip_sets_locked_and_expires() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tks_lockout.toml");
        let _env = EnvGuard::set(&path);

        let mut state = make_state(None, None, 60);
        assert!(!is_locked(&state));

        trip(&mut state, "manual test");
        assert!(is_locked(&state));
        assert_eq!(state.lock_reason.as_deref(), Some("manual test"));

        // Simulate expiry by backdating the timestamp.
        state.locked_until_unix = Some(now_unix() - 1);
        assert!(!is_locked(&state));
    }

    #[test]
    fn force_reset_clears_lockout() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tks_lockout.toml");
        let _env = EnvGuard::set(&path);

        let mut state = make_state(None, None, 60);
        trip(&mut state, "testing reset");
        assert!(is_locked(&state));

        force_reset(&mut state);
        assert!(!is_locked(&state));
        assert!(state.lock_reason.is_none());
    }

    #[test]
    fn consecutive_loss_trip() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tks_lockout.toml");
        let _env = EnvGuard::set(&path);

        let mut state = make_state(None, Some(3), 30);
        assert!(!record_loss(&mut state, 50.0));
        assert!(!record_loss(&mut state, 50.0));
        // Third loss should trip the lockout.
        let tripped = record_loss(&mut state, 50.0);
        assert!(tripped);
        assert!(is_locked(&state));
        assert_eq!(state.consecutive_losses, 3);
    }

    #[test]
    fn daily_loss_trip() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tks_lockout.toml");
        let _env = EnvGuard::set(&path);

        let mut state = make_state(Some(200.0), None, 60);
        assert!(!record_loss(&mut state, 100.0));
        // Second loss pushes daily total to $200, hitting the limit.
        let tripped = record_loss(&mut state, 100.0);
        assert!(tripped);
        assert!(is_locked(&state));
        assert!((state.daily_loss_dollars - 200.0).abs() < 0.01);
    }

    #[test]
    fn round_trip_persist_and_load() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tks_lockout.toml");
        let _env = EnvGuard::set(&path);

        let mut state = make_state(Some(500.0), Some(5), 45);
        trip(&mut state, "round-trip test");
        let loaded = load();
        assert!(is_locked(&loaded));
        assert_eq!(loaded.lock_reason.as_deref(), Some("round-trip test"));
        assert_eq!(loaded.config.cooldown_minutes, 45);
    }
}
