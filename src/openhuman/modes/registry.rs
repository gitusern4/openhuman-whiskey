//! Process-wide registry of available modes + the active selection.
//!
//! The registry is built once at startup from the available concrete
//! `Mode` impls and stored behind a `RwLock` so settings UI mutations
//! (mode switch) are cheap and lock-free on the read side. All callers
//! that need the current mode go through [`active_mode()`] — which
//! returns an `Arc<dyn Mode>` so the mode object can outlive the lock
//! guard.
//!
//! Mode selection is persisted via `~/.openhuman/config.toml` under the
//! key `agent.active_mode = "whiskey"` (or `"default"`). The config
//! loader calls [`set_active_mode`] on boot; the settings UI calls it
//! when the user picks a different mode in the dropdown.

use std::collections::BTreeMap;
use std::sync::Arc;

use once_cell::sync::Lazy;
use parking_lot::RwLock;

use serde::Serialize;

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
    use std::sync::Mutex;

    /// Process-wide lock so the four logical test cases below can mutate
    /// the global active-mode pointer without races. Using a single
    /// in-file `Mutex` instead of an external `serial_test` dev-dep
    /// keeps the project's dependency surface flat.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn reset_to_default() {
        let _ = set_active_mode(DefaultMode::ID);
    }

    #[test]
    fn default_mode_is_active_at_startup() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        reset_to_default();
        assert_eq!(active_mode().id(), DefaultMode::ID);
    }

    #[test]
    fn list_includes_default_and_whiskey() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let ids: Vec<&str> = list_modes().into_iter().map(|d| d.id).collect();
        assert!(ids.contains(&DefaultMode::ID));
        assert!(ids.contains(&WhiskeyMode::ID));
    }

    #[test]
    fn switch_to_whiskey_then_back() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
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
        reset_to_default();
        let err = set_active_mode("does-not-exist").unwrap_err();
        assert!(err.contains("unknown mode id"));
        // Active mode unchanged.
        assert_eq!(active_mode().id(), DefaultMode::ID);
    }
}
