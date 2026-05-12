//! Whiskey first-run onboarding state persistence.
//!
//! Mirrors the pattern in `super::persistence` exactly: lazy path
//! resolution via `crate::openhuman::config::default_root_openhuman_dir`,
//! best-effort read/write, warn-and-skip on every failure mode.
//!
//! The persisted file lives at `<openhuman_dir>/onboarding.toml` and
//! contains two fields:
//!
//! ```toml
//! completed = true
//! tv_bridge_skipped = false
//! ```
//!
//! The Tauri commands exposed here (`onboarding_status`,
//! `onboarding_complete`) are registered in `app/src-tauri/src/lib.rs`
//! alongside the other Whiskey fork commands.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const STATE_FILE: &str = "onboarding.toml";

/// Env-var override used by unit tests to redirect the persistence
/// file to a temp dir without touching `$HOME/.openhuman`. Production
/// callers never set this.
const TEST_OVERRIDE_ENV: &str = "OPENHUMAN_ONBOARDING_FILE";

// ---------------------------------------------------------------------------
// Persisted shape
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OnboardingState {
    /// Whether the wizard has been completed (or explicitly dismissed).
    #[serde(default)]
    completed: bool,
    /// Whether the user skipped the TradingView bridge setup step.
    #[serde(default)]
    tv_bridge_skipped: bool,
    /// Which wizard step the user last reached (0-indexed, 0 = start).
    #[serde(default)]
    current_step: u32,
}

impl Default for OnboardingState {
    fn default() -> Self {
        Self {
            completed: false,
            tv_bridge_skipped: false,
            current_step: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Status struct returned to the frontend
// ---------------------------------------------------------------------------

/// Serialisable status struct that Tauri sends to the React frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardingStatus {
    pub completed: bool,
    pub tv_bridge_skipped: bool,
    pub current_step: u32,
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn state_path() -> Option<PathBuf> {
    if let Ok(override_path) = std::env::var(TEST_OVERRIDE_ENV) {
        let trimmed = override_path.trim().to_string();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    match crate::openhuman::config::default_root_openhuman_dir() {
        Ok(root) => Some(root.join(STATE_FILE)),
        Err(err) => {
            log::warn!("[modes/onboarding] no openhuman dir available: {err}");
            None
        }
    }
}

fn load_state() -> OnboardingState {
    // Route through AtomicTomlStore — same atomic-write + corrupt-tolerant
    // semantics as every other persistence site in modes/.
    let Some(path) = state_path() else {
        return OnboardingState::default();
    };
    crate::openhuman::modes::persistence::AtomicTomlStore::<OnboardingState>::new(path).load()
}

fn save_state(state: &OnboardingState) {
    let Some(path) = state_path() else {
        return;
    };
    let store = crate::openhuman::modes::persistence::AtomicTomlStore::<OnboardingState>::new(path);
    if let Err(err) = store.save(state) {
        log::warn!("[modes/onboarding] atomic save failed: {err}");
    } else {
        log::info!(
            "[modes/onboarding] saved completed={} step={}",
            state.completed,
            state.current_step
        );
    }
}

// ---------------------------------------------------------------------------
// Public API (used by Tauri commands in lib.rs)
// ---------------------------------------------------------------------------

/// Whether the wizard has been completed. Cheap: reads from disk once;
/// callers should cache the result rather than polling in a hot loop.
pub fn is_completed() -> bool {
    load_state().completed
}

/// Return the full status for the React wizard.
pub fn status() -> OnboardingStatus {
    let s = load_state();
    OnboardingStatus {
        completed: s.completed,
        tv_bridge_skipped: s.tv_bridge_skipped,
        current_step: s.current_step,
    }
}

/// Advance the wizard to `step`. Does not mark as completed; call
/// `complete` for that. Best-effort — failures are logged and swallowed.
pub fn advance(step: u32, tv_bridge_skipped: bool) {
    let mut s = load_state();
    s.current_step = step;
    s.tv_bridge_skipped = tv_bridge_skipped;
    save_state(&s);
}

/// Mark the onboarding as completed. Idempotent.
pub fn complete(tv_bridge_skipped: bool) {
    let mut s = load_state();
    s.completed = true;
    s.tv_bridge_skipped = tv_bridge_skipped;
    save_state(&s);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(super) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

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

    fn unique_tmp_file(label: &str) -> std::path::PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("onboarding-{label}-{nanos}.toml"))
    }

    #[test]
    fn fresh_install_reports_not_completed() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let path = unique_tmp_file("fresh");
        let _env = EnvGuard::set(&path);

        assert!(!is_completed(), "fresh install should not be completed");
        let s = status();
        assert!(!s.completed);
        assert_eq!(s.current_step, 0);
    }

    #[test]
    fn complete_marks_as_done_and_round_trips() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let path = unique_tmp_file("complete");
        let _env = EnvGuard::set(&path);

        assert!(!is_completed());
        complete(false);
        assert!(path.exists(), "complete() should have written the file");
        assert!(
            is_completed(),
            "after complete() is_completed should be true"
        );

        let s = status();
        assert!(s.completed);
        assert!(!s.tv_bridge_skipped);
    }

    #[test]
    fn advance_persists_step_and_tv_bridge_flag() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let path = unique_tmp_file("advance");
        let _env = EnvGuard::set(&path);

        advance(2, true);
        let s = status();
        assert_eq!(s.current_step, 2);
        assert!(s.tv_bridge_skipped);
        assert!(!s.completed, "advance should not mark completed");
    }

    #[test]
    fn malformed_toml_falls_back_to_defaults() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let path = unique_tmp_file("malformed");
        std::fs::write(&path, "not valid toml ===\n[[[").unwrap();
        let _env = EnvGuard::set(&path);

        let s = status();
        assert!(!s.completed);
        assert_eq!(s.current_step, 0);
    }

    #[test]
    fn complete_is_idempotent() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let path = unique_tmp_file("idempotent");
        let _env = EnvGuard::set(&path);

        complete(false);
        complete(false);
        assert!(is_completed());
    }

    #[test]
    fn save_creates_missing_parent_dir() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("deep").join("nested");
        let path = nested.join("onboarding.toml");
        let _env = EnvGuard::set(&path);

        complete(true);
        assert!(
            nested.exists(),
            "complete() should have created parent dirs"
        );
        assert!(path.exists());
        assert!(is_completed());
    }
}
