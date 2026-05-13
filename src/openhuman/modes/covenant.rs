//! Covenant loader — immutable session config for Whiskey execution authority.
//!
//! `covenant.toml` lives in `<openhuman_dir>/covenant.toml` and is user-owned.
//! Rust code NEVER writes this file. The covenant is loaded once at session
//! start, validated, hashed, and frozen for the lifetime of the process.
//! Any modification requires an app restart (the friction is intentional).

use sha2::{Digest, Sha256};
use std::sync::OnceLock;

use serde::Deserialize;

// ---------------------------------------------------------------------------
// Schema — mirrors §9 of EXECUTION_LAYER_RESEARCH.md exactly
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize)]
pub struct CovenantLimits {
    pub daily_max_loss_usd: f64,
    pub max_position_size_contracts: u32,
    pub max_consecutive_losses: u32,
    pub no_trading_after: String,
    pub no_trading_before: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CovenantInstruments {
    pub whitelist: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CovenantConfirmation {
    pub require_per_trade_confirm: bool,
    pub confirm_countdown_seconds: u32,
    pub single_leg_market_orders_allowed: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CovenantCooldown {
    pub base_cooldown_seconds: u32,
    pub per_loss_additional_seconds: u32,
    pub walk_away_trigger_loss_fraction: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CovenantSession {
    pub reset_kills_at_session_start: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CovenantMeta {
    pub version: String,
    pub signed_at: String,
}

/// Top-level covenant struct. All fields are mandatory.
#[derive(Debug, Clone, Deserialize)]
pub struct Covenant {
    pub covenant: CovenantMeta,
    pub limits: CovenantLimits,
    pub instruments: CovenantInstruments,
    pub confirmation: CovenantConfirmation,
    pub cooldown: CovenantCooldown,
    pub session: CovenantSession,

    /// Raw TOML bytes stored for deterministic hashing.
    #[serde(skip)]
    raw_toml: String,
}

impl Covenant {
    /// Load covenant from `<openhuman_dir>/covenant.toml`.
    /// Missing or corrupt file → `Err`. No defaults — covenant is mandatory.
    pub fn load(openhuman_dir: &std::path::Path) -> Result<Self, String> {
        let path = openhuman_dir.join("covenant.toml");
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("covenant.toml not found at {}: {}", path.display(), e))?;
        let mut cov: Covenant =
            toml::from_str(&raw).map_err(|e| format!("corrupt covenant.toml: {}", e))?;
        cov.raw_toml = raw;
        Ok(cov)
    }

    /// Enforce hard invariants. Returns first violation as `Err`.
    pub fn validate(&self) -> Result<(), String> {
        if !self.confirmation.require_per_trade_confirm {
            return Err("covenant: require_per_trade_confirm must be true — \
                 setting it to false is not permitted in v1"
                .to_string());
        }
        if self.limits.daily_max_loss_usd <= 0.0 {
            return Err("covenant: daily_max_loss_usd must be > 0".to_string());
        }
        if self.instruments.whitelist.is_empty() {
            return Err("covenant: instruments.whitelist must not be empty".to_string());
        }
        if self.confirmation.confirm_countdown_seconds < 3 {
            return Err(format!(
                "covenant: confirm_countdown_seconds must be >= 3, got {}",
                self.confirmation.confirm_countdown_seconds
            ));
        }
        Ok(())
    }

    /// SHA-256 of the raw TOML bytes (hex-encoded). Stable across reads of
    /// the same file content; written to the audit log at session start.
    pub fn hash(&self) -> String {
        let mut hasher = Sha256::new();
        hasher.update(self.raw_toml.as_bytes());
        format!("{:x}", hasher.finalize())
    }
}

// ---------------------------------------------------------------------------
// Singleton — loaded once, immutable for the process lifetime
// ---------------------------------------------------------------------------

static COVENANT: OnceLock<Covenant> = OnceLock::new();

/// Load and validate the covenant, storing it in the process-wide singleton.
/// Subsequent calls return the cached value; the file is not re-read.
/// Returns `Err` if the file is missing/corrupt or fails validation.
pub fn load_covenant(openhuman_dir: &std::path::Path) -> Result<&'static Covenant, String> {
    if let Some(cov) = COVENANT.get() {
        return Ok(cov);
    }
    let cov = Covenant::load(openhuman_dir)?;
    cov.validate()?;
    Ok(COVENANT.get_or_init(|| cov))
}

/// Access the already-loaded covenant. Panics if `load_covenant` was never
/// called — callers that need the singleton must call `load_covenant` first.
pub fn covenant() -> &'static Covenant {
    COVENANT
        .get()
        .expect("covenant not loaded — call load_covenant() at session start")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn valid_toml() -> &'static str {
        r#"
[covenant]
version = "1.0"
signed_at = "2026-05-12T00:00:00Z"

[limits]
daily_max_loss_usd = 500.0
max_position_size_contracts = 2
max_consecutive_losses = 3
no_trading_after = "20:00"
no_trading_before = "06:30"

[instruments]
whitelist = ["MESH5", "MNQH5", "MES", "MNQ"]

[confirmation]
require_per_trade_confirm = true
confirm_countdown_seconds = 3
single_leg_market_orders_allowed = false

[cooldown]
base_cooldown_seconds = 3
per_loss_additional_seconds = 1
walk_away_trigger_loss_fraction = 0.75

[session]
reset_kills_at_session_start = true
"#
    }

    fn write_covenant(dir: &std::path::Path, content: &str) {
        let mut f = std::fs::File::create(dir.join("covenant.toml")).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn valid_load_and_validate() {
        let tmp = tempfile::tempdir().unwrap();
        write_covenant(tmp.path(), valid_toml());
        let cov = Covenant::load(tmp.path()).unwrap();
        assert!(cov.validate().is_ok());
        assert_eq!(cov.limits.daily_max_loss_usd, 500.0);
        assert_eq!(cov.instruments.whitelist.len(), 4);
    }

    #[test]
    fn missing_file_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        let result = Covenant::load(tmp.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("covenant.toml not found"));
    }

    #[test]
    fn corrupt_toml_returns_error() {
        let tmp = tempfile::tempdir().unwrap();
        write_covenant(tmp.path(), "this is not valid toml ][[[");
        let result = Covenant::load(tmp.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("corrupt covenant.toml"));
    }

    #[test]
    fn validation_rejects_require_per_trade_confirm_false() {
        let tmp = tempfile::tempdir().unwrap();
        let bad = valid_toml().replace(
            "require_per_trade_confirm = true",
            "require_per_trade_confirm = false",
        );
        write_covenant(tmp.path(), &bad);
        let cov = Covenant::load(tmp.path()).unwrap();
        let err = cov.validate().unwrap_err();
        assert!(err.contains("require_per_trade_confirm"));
    }

    #[test]
    fn validation_rejects_empty_whitelist() {
        let tmp = tempfile::tempdir().unwrap();
        let bad = valid_toml().replace(
            r#"whitelist = ["MESH5", "MNQH5", "MES", "MNQ"]"#,
            "whitelist = []",
        );
        write_covenant(tmp.path(), &bad);
        let cov = Covenant::load(tmp.path()).unwrap();
        let err = cov.validate().unwrap_err();
        assert!(err.contains("whitelist"));
    }

    #[test]
    fn validation_rejects_zero_daily_loss() {
        let tmp = tempfile::tempdir().unwrap();
        let bad = valid_toml().replace("daily_max_loss_usd = 500.0", "daily_max_loss_usd = 0.0");
        write_covenant(tmp.path(), &bad);
        let cov = Covenant::load(tmp.path()).unwrap();
        let err = cov.validate().unwrap_err();
        assert!(err.contains("daily_max_loss_usd"));
    }

    #[test]
    fn validation_rejects_countdown_below_3() {
        let tmp = tempfile::tempdir().unwrap();
        let bad = valid_toml().replace(
            "confirm_countdown_seconds = 3",
            "confirm_countdown_seconds = 2",
        );
        write_covenant(tmp.path(), &bad);
        let cov = Covenant::load(tmp.path()).unwrap();
        let err = cov.validate().unwrap_err();
        assert!(err.contains("confirm_countdown_seconds"));
    }

    #[test]
    fn hash_is_stable_across_two_loads() {
        let tmp = tempfile::tempdir().unwrap();
        write_covenant(tmp.path(), valid_toml());
        let cov1 = Covenant::load(tmp.path()).unwrap();
        let cov2 = Covenant::load(tmp.path()).unwrap();
        assert_eq!(cov1.hash(), cov2.hash());
        assert_eq!(cov1.hash().len(), 64); // hex SHA-256
    }

    #[test]
    fn different_content_produces_different_hash() {
        let tmp = tempfile::tempdir().unwrap();
        write_covenant(tmp.path(), valid_toml());
        let cov1 = Covenant::load(tmp.path()).unwrap();
        let modified = valid_toml().replace("500.0", "600.0");
        write_covenant(tmp.path(), &modified);
        let cov2 = Covenant::load(tmp.path()).unwrap();
        assert_ne!(cov1.hash(), cov2.hash());
    }

    #[test]
    fn validation_passes_with_countdown_exactly_3() {
        let tmp = tempfile::tempdir().unwrap();
        write_covenant(tmp.path(), valid_toml());
        let cov = Covenant::load(tmp.path()).unwrap();
        // confirm_countdown_seconds = 3 in valid_toml → must pass
        assert!(cov.validate().is_ok());
    }
}
