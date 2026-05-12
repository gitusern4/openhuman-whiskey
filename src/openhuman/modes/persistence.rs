//! Persistence of the active mode selection across process restarts.
//!
//! The mode registry (see `super::registry`) keeps the active `Mode`
//! pointer behind a `RwLock` for the in-process hot path. That pointer
//! is rebuilt from `DefaultMode` on every boot, so without persistence a
//! user who picks `WhiskeyMode` in the settings UI would see the
//! selection reset to default after the next relaunch.
//!
//! This module mirrors the canonical TOML-persistence pattern used by
//! `app/src-tauri/src/window_state.rs`: lazy path resolution via the
//! configured openhuman root dir (`crate::openhuman::config::
//! default_root_openhuman_dir`), best-effort read/write, and warn-and-
//! skip on every failure mode. The persisted file lives at
//! `<openhuman_dir>/active_mode.toml` and contains a single record:
//!
//! ```toml
//! active_mode = "whiskey"
//! ```
//!
//! Both [`save`] and [`load`] are infallible from the caller's point of
//! view: `save` swallows errors with a warn-log, and `load` returns
//! `None` whenever the file is missing, malformed, or unreadable. That
//! way the registry's startup path always falls back to `DefaultMode`
//! cleanly when persistence is unavailable.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const STATE_FILE: &str = "active_mode.toml";

/// Env-var override used by the unit tests to redirect the persistence
/// file to a temp dir without touching `$HOME/.openhuman`. Production
/// callers never set this — when unset, [`state_path`] falls through to
/// `default_root_openhuman_dir()`.
const TEST_OVERRIDE_ENV: &str = "OPENHUMAN_ACTIVE_MODE_FILE";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ActiveModeState {
    active_mode: String,
}

fn state_path() -> Option<PathBuf> {
    if let Ok(override_path) = std::env::var(TEST_OVERRIDE_ENV) {
        if !override_path.is_empty() {
            return Some(PathBuf::from(override_path));
        }
    }
    match crate::openhuman::config::default_root_openhuman_dir() {
        Ok(root) => Some(root.join(STATE_FILE)),
        Err(err) => {
            log::warn!("[modes/persistence] no openhuman dir available: {err}");
            None
        }
    }
}

/// Persist the given mode id. Best-effort: any failure (path
/// resolution, mkdir, serialize, write) is logged at warn level and
/// swallowed. Never panics, never returns an error.
pub(crate) fn save(id: &str) {
    let Some(path) = state_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            log::warn!(
                "[modes/persistence] mkdir {} failed: {}; skip save",
                parent.display(),
                err
            );
            return;
        }
    }
    let state = ActiveModeState {
        active_mode: id.to_string(),
    };
    let raw = match toml::to_string_pretty(&state) {
        Ok(r) => r,
        Err(err) => {
            log::warn!("[modes/persistence] serialize failed: {err}; skip save");
            return;
        }
    };
    if let Err(err) = std::fs::write(&path, raw) {
        log::warn!("[modes/persistence] write {} failed: {err}", path.display());
    } else {
        log::info!("[modes/persistence] saved active_mode={id}");
    }
}

/// Read the persisted active mode id, if any.
///
/// Returns `None` for every failure mode (no path, missing file, IO
/// error, malformed TOML). Never panics. The caller (registry init) is
/// expected to validate the returned id against the registered mode set
/// and fall back to `DefaultMode` when the id is unknown.
pub(crate) fn load() -> Option<String> {
    let path = state_path()?;
    let raw = match std::fs::read_to_string(&path) {
        Ok(r) => r,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return None,
        Err(err) => {
            log::warn!("[modes/persistence] read {} failed: {err}", path.display());
            return None;
        }
    };
    match toml::from_str::<ActiveModeState>(&raw) {
        Ok(state) => Some(state.active_mode),
        Err(err) => {
            log::warn!(
                "[modes/persistence] parse {} failed: {err}; ignoring",
                path.display()
            );
            None
        }
    }
}

/// Process-wide lock for tests — the env-var override is global state,
/// so any test that touches it (in this module OR in `super::registry`'s
/// tests, where `set_active_mode` triggers `save`) must serialize on
/// this single mutex. Exposed `pub(super)` so the registry tests can
/// share it.
#[cfg(test)]
pub(super) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Env-var name re-exported for test-only use by sibling modules that
/// also need to redirect persistence to a temp file.
#[cfg(test)]
pub(super) const TEST_OVERRIDE_ENV_FOR_TESTS: &str = TEST_OVERRIDE_ENV;

#[cfg(test)]
mod tests {
    use super::*;

    /// RAII guard: sets the override env var on construct, clears it
    /// on drop. Ensures even a panicking test does not leak the
    /// override into a sibling test.
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

    #[test]
    fn round_trip_via_temp_dir_override() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("active_mode.toml");
        let _env = EnvGuard::set(&path);

        save("whiskey");
        assert!(path.exists(), "save should have written the file");
        let loaded = load();
        assert_eq!(loaded.as_deref(), Some("whiskey"));
    }

    #[test]
    fn missing_file_returns_none() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("does_not_exist.toml");
        let _env = EnvGuard::set(&path);

        assert!(load().is_none());
    }

    #[test]
    fn malformed_toml_returns_none() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("active_mode.toml");
        std::fs::write(&path, "this is not = valid toml ===\n[[[").unwrap();
        let _env = EnvGuard::set(&path);

        assert!(load().is_none());
    }

    #[test]
    fn save_creates_missing_parent_dir() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        // Two levels of missing directories — exercise create_dir_all.
        let nested = tmp.path().join("nested").join("deeper");
        let path = nested.join("active_mode.toml");
        assert!(!nested.exists());
        let _env = EnvGuard::set(&path);

        save("default");
        assert!(nested.exists(), "save should have created the parent dir");
        assert!(path.exists());
        assert_eq!(load().as_deref(), Some("default"));
    }
}
