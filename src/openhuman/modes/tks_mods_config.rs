//! TK's Mods — persisted configuration.
//!
//! Stores the three TK's Mods preferences in
//! `<openhuman_dir>/tks_mods.toml`:
//!
//! ```toml
//! theme = "zeth"        # "default" | "zeth"
//! hide_risk_pct = true  # boolean
//! ```
//!
//! Follows the identical pattern as `persistence.rs` (the active-mode
//! file): lazy path via `crate::openhuman::config::default_root_openhuman_dir`,
//! best-effort save (warn + swallow on error), infallible load (returns
//! defaults on any failure).
//!
//! The `theme` field is a free-form string so future themes drop in
//! without a schema migration: unknown values silently fall back to
//! `"default"` in the UI.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const STATE_FILE: &str = "tks_mods.toml";

/// Env-var override so unit tests can redirect the file to a temp dir.
const TEST_OVERRIDE_ENV: &str = "OPENHUMAN_TKS_MODS_FILE";

/// The persisted TK's Mods configuration record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TksModsConfig {
    /// Active theme id.  `"default"` | `"zeth"` | any future id.
    #[serde(default = "default_theme")]
    pub theme: String,

    /// When `true`, Whiskey messages are filtered through the risk
    /// sanitizer before reaching the overlay / chat UI.
    #[serde(default)]
    pub hide_risk_pct: bool,
}

fn default_theme() -> String {
    "default".to_string()
}

impl Default for TksModsConfig {
    fn default() -> Self {
        Self {
            theme: default_theme(),
            hide_risk_pct: false,
        }
    }
}

fn state_path() -> Option<PathBuf> {
    if let Ok(ov) = std::env::var(TEST_OVERRIDE_ENV) {
        if !ov.is_empty() {
            return Some(PathBuf::from(ov));
        }
    }
    match crate::openhuman::config::default_root_openhuman_dir() {
        Ok(root) => Some(root.join(STATE_FILE)),
        Err(err) => {
            log::warn!("[tks_mods_config] no openhuman dir: {err}");
            None
        }
    }
}

/// Persist the config. Best-effort: any failure is warn-logged and
/// swallowed. Never panics.
pub fn save(cfg: &TksModsConfig) {
    let Some(path) = state_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            log::warn!(
                "[tks_mods_config] mkdir {} failed: {err}; skip save",
                parent.display()
            );
            return;
        }
    }
    let raw = match toml::to_string_pretty(cfg) {
        Ok(r) => r,
        Err(err) => {
            log::warn!("[tks_mods_config] serialize failed: {err}");
            return;
        }
    };
    if let Err(err) = std::fs::write(&path, raw) {
        log::warn!("[tks_mods_config] write {} failed: {err}", path.display());
    } else {
        log::info!("[tks_mods_config] saved theme={}", cfg.theme);
    }
}

/// Load the config. Returns `TksModsConfig::default()` on any failure
/// (file missing, malformed TOML, path unavailable). Never panics.
pub fn load() -> TksModsConfig {
    let Some(path) = state_path() else {
        return TksModsConfig::default();
    };
    let raw = match std::fs::read_to_string(&path) {
        Ok(r) => r,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return TksModsConfig::default();
        }
        Err(err) => {
            log::warn!("[tks_mods_config] read {} failed: {err}", path.display());
            return TksModsConfig::default();
        }
    };
    match toml::from_str::<TksModsConfig>(&raw) {
        Ok(cfg) => cfg,
        Err(err) => {
            log::warn!("[tks_mods_config] parse failed: {err}; using defaults");
            TksModsConfig::default()
        }
    }
}

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

    #[test]
    fn round_trip_theme_and_hide_flag() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tks_mods.toml");
        let _env = EnvGuard::set(&path);

        let cfg = TksModsConfig {
            theme: "zeth".to_string(),
            hide_risk_pct: true,
        };
        save(&cfg);
        assert!(path.exists());
        let loaded = load();
        assert_eq!(loaded, cfg);
    }

    #[test]
    fn missing_file_returns_defaults() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("does_not_exist.toml");
        let _env = EnvGuard::set(&path);

        let cfg = load();
        assert_eq!(cfg.theme, "default");
        assert!(!cfg.hide_risk_pct);
    }

    #[test]
    fn malformed_toml_returns_defaults() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tks_mods.toml");
        std::fs::write(&path, "this is not = valid ===\n[[[").unwrap();
        let _env = EnvGuard::set(&path);

        let cfg = load();
        assert_eq!(cfg, TksModsConfig::default());
    }
}
