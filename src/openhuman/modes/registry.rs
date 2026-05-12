//! Process-wide registry of available modes + the active selection.
//!
//! The registry is built once at startup from the available concrete
//! `Mode` impls and stored behind a `RwLock` so settings UI mutations
//! (mode switch) are cheap and lock-free on the read side. All callers
//! that need the current mode go through [`active_mode()`] — which
//! returns an `Arc<dyn Mode>` so the mode object can outlive the lock
//! guard.
//!
//! Mode selection is persisted to `<openhuman_dir>/active_mode.toml`
//! via the [`super::persistence`] helper (the previous "via
//! `~/.openhuman/config.toml` under `agent.active_mode`" claim is
//! superseded by this dedicated file). On startup the `ACTIVE` lazy
//! initializer attempts a `persistence::load()` and, when it returns a
//! known mode id, swaps the registry's `DefaultMode` fallback for that
//! mode. On every successful [`set_active_mode`] call we fire-and-
//! forget a `persistence::save(id)` so the next process boot sees the
//! same selection. The settings UI calls [`set_active_mode`] when the
//! user picks a different mode in the dropdown.

use std::collections::BTreeMap;
use std::sync::Arc;

use once_cell::sync::Lazy;
use parking_lot::RwLock;

use serde::Serialize;

use super::persistence;
use super::{DefaultMode, Mode, ModeId, SharedMode, WhiskeyMode};

/// Snapshot of one mode for the settings UI dropdown.
///
/// `Serialize` is implemented so this struct round-trips through Tauri
/// IPC (the `list_modes` command in `app/src-tauri/src/lib.rs` returns
/// `Vec<ModeDescriptor>` directly to the frontend mode picker).
/// `id: &'static str` serializes to a JSON string fine; we only need
/// the serialize half because the frontend never sends descriptors
/// back — it just sends the `id` string to `set_active_mode`.
#[derive(Debug, Clone, Serialize)]
pub struct ModeDescriptor {
    pub id: &'static str,
    pub display_name: String,
    pub description: String,
}

/// Fixed registry of every Mode impl shipped in the binary. Adding a new
/// persona = adding one entry here and one `pub mod` in `mod.rs`.
pub struct ModeRegistry {
    modes: BTreeMap<ModeId, SharedMode>,
}

impl ModeRegistry {
    fn build_default() -> Self {
        let mut modes: BTreeMap<ModeId, SharedMode> = BTreeMap::new();
        let default = Arc::new(DefaultMode::new()) as SharedMode;
        let whiskey = Arc::new(WhiskeyMode::new()) as SharedMode;
        modes.insert(default.id(), default);
        modes.insert(whiskey.id(), whiskey);
        Self { modes }
    }

    pub fn list(&self) -> Vec<ModeDescriptor> {
        self.modes
            .values()
            .map(|m| ModeDescriptor {
                id: m.id(),
                display_name: m.display_name().to_string(),
                description: m.description().to_string(),
            })
            .collect()
    }

    pub fn get(&self, id: &str) -> Option<SharedMode> {
        self.modes.get(id).cloned()
    }
}

static REGISTRY: Lazy<ModeRegistry> = Lazy::new(ModeRegistry::build_default);

/// Active mode pointer. RwLock so the hot path (`active_mode()`) takes a
/// read lock — `parking_lot::RwLock` reads are essentially atomic on
/// uncontended state.
static ACTIVE: Lazy<RwLock<SharedMode>> = Lazy::new(|| {
    let default = REGISTRY
        .get(DefaultMode::ID)
        .expect("DefaultMode is always registered");
    // Best-effort: if a persisted selection exists AND the id is known
    // to the registry, restore it. Otherwise fall through to default
    // so a missing/corrupt persistence file never blocks boot.
    if let Some(persisted_id) = persistence::load() {
        if let Some(persisted_mode) = REGISTRY.get(&persisted_id) {
            log::info!("[modes] restored persisted active mode: {persisted_id}");
            return RwLock::new(persisted_mode);
        } else {
            log::warn!(
                "[modes] persisted active_mode={persisted_id} is not registered; falling back to default"
            );
        }
    }
    RwLock::new(default)
});

/// Get the currently active mode. Cheap; intended for the per-LLM-call
/// hot path in `providers::router`.
pub fn active_mode() -> SharedMode {
    ACTIVE.read().clone()
}

/// Switch the active mode. Returns `Err(unknown_id)` if the id isn't
/// registered. Logs the switch for telemetry.
pub fn set_active_mode(id: &str) -> Result<(), String> {
    match REGISTRY.get(id) {
        Some(mode) => {
            log::info!(
                "[modes] switching active mode: {} -> {}",
                active_mode().id(),
                id
            );
            *ACTIVE.write() = mode;
            // Fire-and-forget: errors are logged inside `save` and
            // never propagated — a persistence failure must not break
            // the in-process mode switch the user just requested.
            persistence::save(id);
            Ok(())
        }
        None => {
            let registered: Vec<&str> = REGISTRY.modes.keys().copied().collect();
            log::warn!(
                "[modes] set_active_mode rejected unknown id={id}; registered={:?}",
                registered
            );
            Err(format!(
                "unknown mode id: {id} (registered: {:?})",
                registered
            ))
        }
    }
}

/// List all registered modes — used by the settings UI to populate the
/// mode-picker dropdown.
pub fn list_modes() -> Vec<ModeDescriptor> {
    REGISTRY.list()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Share `persistence::TEST_LOCK` so tests in this module and the
    /// persistence module can't race on the env-var override that
    /// redirects the persistence file to a temp dir. Each test below
    /// sets the override on entry and clears it on drop, so existing
    /// dev/CI users never see writes to `~/.openhuman/active_mode.toml`
    /// during `cargo test`.
    use super::persistence::{TEST_LOCK, TEST_OVERRIDE_ENV_FOR_TESTS};

    /// RAII guard: redirect the persistence file to a fresh temp path
    /// for the duration of one test, then clear the env var on drop so
    /// nothing leaks into a sibling test or the user's home dir.
    ///
    /// WHISKEY_AUDIT.md M5: also holds a process-wide
    /// `EnvVarTestGuard` so this test serializes against any other
    /// env-var-touching test in the binary (the per-file `TEST_LOCK`
    /// only serializes within `modes::registry::tests`; the env var
    /// itself is process-global).
    struct PersistenceRedirect {
        _tmp: tempfile::TempDir,
        _env_lock: super::EnvVarTestGuard,
    }
    impl PersistenceRedirect {
        fn new() -> Self {
            let env_lock = super::EnvVarTestGuard::new();
            let tmp = tempfile::tempdir().expect("tempdir");
            std::env::set_var(
                TEST_OVERRIDE_ENV_FOR_TESTS,
                tmp.path().join("active_mode.toml"),
            );
            Self {
                _tmp: tmp,
                _env_lock: env_lock,
            }
        }
    }
    impl Drop for PersistenceRedirect {
        fn drop(&mut self) {
            std::env::remove_var(TEST_OVERRIDE_ENV_FOR_TESTS);
            // _env_lock drops after env var cleared so the next test's
            // EnvVarTestGuard acquire sees a clean env.
        }
    }

    fn reset_to_default() {
        let _ = set_active_mode(DefaultMode::ID);
    }

    #[test]
    fn default_mode_is_active_at_startup() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let _redirect = PersistenceRedirect::new();
        reset_to_default();
        assert_eq!(active_mode().id(), DefaultMode::ID);
    }

    #[test]
    fn list_includes_default_and_whiskey() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let _redirect = PersistenceRedirect::new();
        let ids: Vec<&str> = list_modes().into_iter().map(|d| d.id).collect();
        assert!(ids.contains(&DefaultMode::ID));
        assert!(ids.contains(&WhiskeyMode::ID));
    }

    #[test]
    fn switch_to_whiskey_then_back() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let _redirect = PersistenceRedirect::new();
        reset_to_default();
        assert!(set_active_mode(WhiskeyMode::ID).is_ok());
        assert_eq!(active_mode().id(), WhiskeyMode::ID);
        assert!(active_mode().system_prompt_prefix().is_some());
        reset_to_default();
        assert_eq!(active_mode().id(), DefaultMode::ID);
        assert!(active_mode().system_prompt_prefix().is_none());
    }

    #[test]
    fn switch_to_unknown_id_is_rejected() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let _redirect = PersistenceRedirect::new();
        reset_to_default();
        let err = set_active_mode("does-not-exist").unwrap_err();
        assert!(err.contains("unknown mode id"));
        // Active mode unchanged.
        assert_eq!(active_mode().id(), DefaultMode::ID);
    }

    #[test]
    fn set_active_mode_persists_to_disk() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let _redirect = PersistenceRedirect::new();
        reset_to_default();
        assert!(set_active_mode(WhiskeyMode::ID).is_ok());
        // The fire-and-forget save should have written the persisted id
        // — exercise it via the public load() to avoid coupling to the
        // file path used internally by the redirect.
        assert_eq!(persistence::load().as_deref(), Some(WhiskeyMode::ID));
        reset_to_default();
        assert_eq!(persistence::load().as_deref(), Some(DefaultMode::ID));
    }
}
