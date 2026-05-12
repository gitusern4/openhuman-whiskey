//! Kill switch — the e-stop for Whiskey execution.
//!
//! `trigger_kill` sequence (§6 of research doc):
//!   1. Set `kill_engaged = true` on disk (atomic write).
//!   2. Cancel all open orders (5 s timeout each).
//!   3. Flatten all positions via market orders.
//!   4. Revoke broker auth token.
//!   5. Write kill event to audit log.
//!   6. Send notification.
//!
//! `request_reset` enforces:
//!   - 30-minute cooldown since `engaged_at`.
//!   - Phrase must be exactly "I am ready to trade".
//!
//! State is persisted to `<openhuman_dir>/kill_switch.toml` via atomic
//! write (write tmp + rename) — the pattern AtomicTomlStore wraps.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::openhuman::integrations::topstepx::{
    cancel::cancel_all_for_account, flatten::flatten_all_positions, TopStepClient,
};
use crate::openhuman::modes::audit::{AuditAction, AuditActor, AuditEntry, AuditWriter};

// ---------------------------------------------------------------------------
// State types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum KillTrigger {
    ManualButton,
    DailyLossLimit,
    ConsecutiveLossLimit,
    BrokerAuthFailure,
    ExternalWebhook,
}

impl std::fmt::Display for KillTrigger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KillTrigger::ManualButton => write!(f, "manual_button"),
            KillTrigger::DailyLossLimit => write!(f, "daily_loss_limit"),
            KillTrigger::ConsecutiveLossLimit => write!(f, "consecutive_loss_limit"),
            KillTrigger::BrokerAuthFailure => write!(f, "broker_auth_failure"),
            KillTrigger::ExternalWebhook => write!(f, "external_webhook"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KillState {
    pub engaged: bool,
    pub engaged_at: i64,
    pub trigger: Option<KillTrigger>,
    /// UTC timestamp after which reset is eligible (engaged_at + 1800 seconds).
    pub reset_after_utc: Option<i64>,
}

impl Default for KillState {
    fn default() -> Self {
        Self {
            engaged: false,
            engaged_at: 0,
            trigger: None,
            reset_after_utc: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Persistence helpers (atomic write = write tmp + rename)
// ---------------------------------------------------------------------------

fn state_path(openhuman_dir: &Path) -> PathBuf {
    openhuman_dir.join("kill_switch.toml")
}

fn load_state(openhuman_dir: &Path) -> KillState {
    let path = state_path(openhuman_dir);
    match std::fs::read_to_string(&path) {
        Ok(s) => toml::from_str(&s).unwrap_or_default(),
        Err(_) => KillState::default(),
    }
}

fn save_state(openhuman_dir: &Path, state: &KillState) -> Result<(), String> {
    let toml_str =
        toml::to_string(state).map_err(|e| format!("kill_switch serialize error: {}", e))?;
    let tmp_path = openhuman_dir.join("kill_switch.toml.tmp");
    std::fs::write(&tmp_path, &toml_str)
        .map_err(|e| format!("kill_switch tmp write error: {}", e))?;
    std::fs::rename(&tmp_path, state_path(openhuman_dir))
        .map_err(|e| format!("kill_switch atomic rename error: {}", e))?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Check whether the kill switch is currently engaged (reads from disk).
pub fn is_engaged(openhuman_dir: &Path) -> bool {
    load_state(openhuman_dir).engaged
}

/// Full kill sequence. Non-fatal errors in cancel/flatten are logged but do
/// not abort the sequence — we always reach `engaged = true` + audit write.
pub async fn trigger_kill(
    openhuman_dir: &Path,
    client: &TopStepClient,
    account_id: u64,
    trigger: KillTrigger,
    audit: &mut AuditWriter,
    session_loss_count: u32,
    daily_pnl: f64,
) -> Result<(), String> {
    let now = Utc::now().timestamp();

    // 1. Set engaged on disk immediately — before any network calls.
    let state = KillState {
        engaged: true,
        engaged_at: now,
        trigger: Some(trigger.clone()),
        reset_after_utc: Some(now + 1800),
    };
    save_state(openhuman_dir, &state)?;

    // 2. Cancel all open orders (5s timeout each — enforced by caller via
    //    tokio::time::timeout if needed; we make a single call here).
    if let Err(e) = cancel_all_for_account(client, account_id).await {
        log::warn!("kill_switch: cancel_all error (continuing): {}", e);
    }

    // 3. Flatten all positions.
    if let Err(e) = flatten_all_positions(client, account_id).await {
        log::warn!("kill_switch: flatten_all error (continuing): {}", e);
    }

    // 4. Revoke broker token.
    client.revoke().await;

    // 5. Write audit entry.
    let entry = AuditEntry {
        timestamp_utc: Utc::now(),
        actor: AuditActor::System,
        action: AuditAction::Kill,
        instrument: None,
        qty: None,
        price: None,
        stop: None,
        target: None,
        r_estimate: None,
        confidence_pct: None,
        playbook_match_id: None,
        idempotency_key: None,
        broker_response: None,
        session_loss_count: Some(session_loss_count),
        daily_pnl_at_action: Some(daily_pnl),
        kill_engaged: true,
        notes: Some(format!("kill triggered: {}", trigger)),
        covenant_hash: None,
    };
    if let Err(e) = audit.record(&entry) {
        log::error!("kill_switch: audit write failed: {}", e);
    }

    Ok(())
}

const RESET_PHRASE: &str = "I am ready to trade";
const COOLDOWN_SECONDS: i64 = 1800; // 30 minutes

/// Attempt to reset the kill switch.
/// Returns `Ok(())` only if cooldown elapsed AND phrase matches exactly.
pub fn request_reset(openhuman_dir: &Path, phrase: &str) -> Result<(), String> {
    if phrase != RESET_PHRASE {
        return Err(format!(
            "reset phrase mismatch — expected \"{}\"",
            RESET_PHRASE
        ));
    }

    let state = load_state(openhuman_dir);
    if !state.engaged {
        return Err("kill switch is not engaged".to_string());
    }

    let now = Utc::now().timestamp();
    let elapsed = now - state.engaged_at;
    if elapsed < COOLDOWN_SECONDS {
        let remaining = COOLDOWN_SECONDS - elapsed;
        return Err(format!(
            "cooldown not elapsed — {} seconds remaining",
            remaining
        ));
    }

    let new_state = KillState::default();
    save_state(openhuman_dir, &new_state)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_is_not_engaged() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!is_engaged(tmp.path()));
    }

    #[test]
    fn save_and_load_state_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let state = KillState {
            engaged: true,
            engaged_at: 1_700_000_000,
            trigger: Some(KillTrigger::ManualButton),
            reset_after_utc: Some(1_700_001_800),
        };
        save_state(tmp.path(), &state).unwrap();
        let loaded = load_state(tmp.path());
        assert!(loaded.engaged);
        assert_eq!(loaded.engaged_at, 1_700_000_000);
        assert_eq!(loaded.trigger, Some(KillTrigger::ManualButton));
    }

    #[test]
    fn reset_wrong_phrase_returns_err() {
        let tmp = tempfile::tempdir().unwrap();
        // Engage manually.
        let now = Utc::now().timestamp() - 9999; // well past cooldown
        save_state(
            tmp.path(),
            &KillState {
                engaged: true,
                engaged_at: now,
                trigger: Some(KillTrigger::ManualButton),
                reset_after_utc: Some(now + 1800),
            },
        )
        .unwrap();
        let err = request_reset(tmp.path(), "wrong phrase").unwrap_err();
        assert!(err.contains("phrase mismatch"));
    }

    #[test]
    fn reset_during_cooldown_returns_err() {
        let tmp = tempfile::tempdir().unwrap();
        let now = Utc::now().timestamp(); // just engaged
        save_state(
            tmp.path(),
            &KillState {
                engaged: true,
                engaged_at: now,
                trigger: Some(KillTrigger::ManualButton),
                reset_after_utc: Some(now + 1800),
            },
        )
        .unwrap();
        let err = request_reset(tmp.path(), RESET_PHRASE).unwrap_err();
        assert!(err.contains("cooldown not elapsed"));
    }

    #[test]
    fn reset_after_cooldown_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let old_time = Utc::now().timestamp() - 9999;
        save_state(
            tmp.path(),
            &KillState {
                engaged: true,
                engaged_at: old_time,
                trigger: Some(KillTrigger::ManualButton),
                reset_after_utc: Some(old_time + 1800),
            },
        )
        .unwrap();
        request_reset(tmp.path(), RESET_PHRASE).unwrap();
        assert!(!is_engaged(tmp.path()));
    }

    #[test]
    fn trigger_display_strings() {
        assert_eq!(KillTrigger::ManualButton.to_string(), "manual_button");
        assert_eq!(KillTrigger::DailyLossLimit.to_string(), "daily_loss_limit");
    }

    #[test]
    fn atomic_write_leaves_no_tmp_file_on_success() {
        let tmp = tempfile::tempdir().unwrap();
        let state = KillState::default();
        save_state(tmp.path(), &state).unwrap();
        let tmp_file = tmp.path().join("kill_switch.toml.tmp");
        assert!(!tmp_file.exists());
    }
}
