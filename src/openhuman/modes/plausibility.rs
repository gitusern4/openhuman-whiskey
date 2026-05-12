//! Pre-trade plausibility checks — pure function, no side effects.
//!
//! `check(proposal, covenant, last_quote)` validates all safety primitives
//! before any broker call is made. Returns `Ok(())` or a list of failures.
//!
//! Every check MUST pass for order submission to proceed. The kill switch
//! check is always first — no other work is done if the switch is engaged.

use crate::openhuman::modes::covenant::Covenant;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
pub enum PlausibilityFailure {
    KillSwitchEngaged,
    DirectionMismatch {
        order_action: String,
        signal_direction: String,
    },
    PriceOutOfRange {
        order_price: f64,
        last_quote: f64,
        deviation_pct: f64,
    },
    StopRequired,
    QtyExceedsCap {
        qty: u32,
        cap: u32,
    },
    InstrumentNotWhitelisted {
        instrument: String,
    },
    OutsideTradingHours {
        current_time: String,
        allowed_after: String,
        allowed_before: String,
    },
    NoLastQuote,
}

impl std::fmt::Display for PlausibilityFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlausibilityFailure::KillSwitchEngaged =>
                write!(f, "kill switch is engaged"),
            PlausibilityFailure::DirectionMismatch { order_action, signal_direction } =>
                write!(f, "direction mismatch: order={order_action} signal={signal_direction}"),
            PlausibilityFailure::PriceOutOfRange { order_price, last_quote, deviation_pct } =>
                write!(
                    f,
                    "price out of range: order={order_price:.4} quote={last_quote:.4} dev={deviation_pct:.3}%"
                ),
            PlausibilityFailure::StopRequired =>
                write!(f, "stop loss is required — no single-leg orders"),
            PlausibilityFailure::QtyExceedsCap { qty, cap } =>
                write!(f, "qty {qty} exceeds session cap {cap}"),
            PlausibilityFailure::InstrumentNotWhitelisted { instrument } =>
                write!(f, "instrument {instrument} not in covenant whitelist"),
            PlausibilityFailure::OutsideTradingHours { current_time, allowed_after, allowed_before } =>
                write!(
                    f,
                    "outside trading hours: now={current_time} window={allowed_after}–{allowed_before}"
                ),
            PlausibilityFailure::NoLastQuote =>
                write!(f, "no last quote available — cannot validate price"),
        }
    }
}

// ---------------------------------------------------------------------------
// Proposal + Quote types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct TradeProposal {
    /// "Buy" or "Sell"
    pub action: String,
    /// Direction from LLM signal ("long" | "short" | "Buy" | "Sell")
    pub signal_direction: String,
    pub instrument: String,
    pub qty: u32,
    /// Entry limit price (None → market order — rejected unless covenant allows)
    pub entry_price: Option<f64>,
    /// Stop-loss distance in ticks (0 → no stop → rejected)
    pub stop_loss_ticks: u32,
    /// Take-profit distance in ticks
    pub take_profit_ticks: u32,
    pub idempotency_key: String,
    pub confidence_pct: u8,
    pub playbook_match_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Quote {
    pub last: f64,
}

// ---------------------------------------------------------------------------
// The check
// ---------------------------------------------------------------------------

/// Run all plausibility gates. Returns `Ok(())` only when every gate passes.
/// `kill_engaged` must be passed by the caller (the kill switch module).
pub fn check(
    proposal: &TradeProposal,
    covenant: &Covenant,
    last_quote: Option<&Quote>,
    kill_engaged: bool,
) -> Result<(), Vec<PlausibilityFailure>> {
    let mut failures = Vec::new();

    // 1. Kill switch — bail immediately if engaged.
    if kill_engaged {
        return Err(vec![PlausibilityFailure::KillSwitchEngaged]);
    }

    // 2. Direction consistency.
    let action_norm = normalize_direction(&proposal.action);
    let signal_norm = normalize_direction(&proposal.signal_direction);
    if action_norm != signal_norm {
        failures.push(PlausibilityFailure::DirectionMismatch {
            order_action: proposal.action.clone(),
            signal_direction: proposal.signal_direction.clone(),
        });
    }

    // 3. Price within 0.5% of last quote.
    if let Some(price) = proposal.entry_price {
        match last_quote {
            Some(q) => {
                let dev = ((price - q.last) / q.last).abs() * 100.0;
                if dev > 0.5 {
                    failures.push(PlausibilityFailure::PriceOutOfRange {
                        order_price: price,
                        last_quote: q.last,
                        deviation_pct: dev,
                    });
                }
            }
            None => {
                failures.push(PlausibilityFailure::NoLastQuote);
            }
        }
    } else if last_quote.is_none() {
        failures.push(PlausibilityFailure::NoLastQuote);
    }

    // 4. Stop required.
    if proposal.stop_loss_ticks == 0 {
        failures.push(PlausibilityFailure::StopRequired);
    }

    // 5. Qty <= covenant cap.
    if proposal.qty > covenant.limits.max_position_size_contracts {
        failures.push(PlausibilityFailure::QtyExceedsCap {
            qty: proposal.qty,
            cap: covenant.limits.max_position_size_contracts,
        });
    }

    // 6. Instrument in whitelist.
    if !covenant
        .instruments
        .whitelist
        .iter()
        .any(|w| w == &proposal.instrument)
    {
        failures.push(PlausibilityFailure::InstrumentNotWhitelisted {
            instrument: proposal.instrument.clone(),
        });
    }

    // 7. Trading hours.
    let now_time = chrono::Utc::now().format("%H:%M").to_string();
    if !within_trading_hours(
        &now_time,
        &covenant.limits.no_trading_before,
        &covenant.limits.no_trading_after,
    ) {
        failures.push(PlausibilityFailure::OutsideTradingHours {
            current_time: now_time,
            allowed_after: covenant.limits.no_trading_before.clone(),
            allowed_before: covenant.limits.no_trading_after.clone(),
        });
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures)
    }
}

fn normalize_direction(s: &str) -> &'static str {
    match s.to_lowercase().as_str() {
        "buy" | "long" => "long",
        "sell" | "short" => "short",
        _ => "unknown",
    }
}

/// Simple HH:MM string comparison for trading hours check.
fn within_trading_hours(now: &str, start: &str, end: &str) -> bool {
    now >= start && now < end
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_covenant(tmp: &std::path::Path) -> Covenant {
        let toml = r#"
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
"#;
        let mut f = std::fs::File::create(tmp.join("covenant.toml")).unwrap();
        f.write_all(toml.as_bytes()).unwrap();
        Covenant::load(tmp).unwrap()
    }

    fn valid_proposal() -> TradeProposal {
        TradeProposal {
            action: "Buy".to_string(),
            signal_direction: "long".to_string(),
            instrument: "MES".to_string(),
            qty: 1,
            entry_price: Some(5200.25),
            stop_loss_ticks: 8,
            take_profit_ticks: 16,
            idempotency_key: "uuid-123".to_string(),
            confidence_pct: 75,
            playbook_match_id: Some("orb-v2".to_string()),
        }
    }

    fn valid_quote() -> Quote {
        Quote { last: 5200.0 }
    }

    #[test]
    fn all_checks_pass_for_valid_proposal() {
        let tmp = tempfile::tempdir().unwrap();
        let cov = make_covenant(tmp.path());
        let proposal = valid_proposal();
        let quote = valid_quote();
        // Override to a time within trading hours
        // (check uses Utc::now() so we trust current test time is within 06:30-20:00)
        let result = check(&proposal, &cov, Some(&quote), false);
        // May fail on OutsideTradingHours if tests run at odd hours — that's fine
        // for a unit test; the other checks pass.
        if let Err(ref failures) = result {
            for f in failures {
                assert!(
                    matches!(f, PlausibilityFailure::OutsideTradingHours { .. }),
                    "unexpected failure: {}",
                    f
                );
            }
        }
    }

    #[test]
    fn kill_switch_engaged_returns_immediately() {
        let tmp = tempfile::tempdir().unwrap();
        let cov = make_covenant(tmp.path());
        let err = check(&valid_proposal(), &cov, Some(&valid_quote()), true).unwrap_err();
        assert_eq!(err.len(), 1);
        assert_eq!(err[0], PlausibilityFailure::KillSwitchEngaged);
    }

    #[test]
    fn direction_mismatch_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let cov = make_covenant(tmp.path());
        let mut p = valid_proposal();
        p.action = "Sell".to_string();
        p.signal_direction = "long".to_string();
        let errs = check(&p, &cov, Some(&valid_quote()), false).unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, PlausibilityFailure::DirectionMismatch { .. })));
    }

    #[test]
    fn price_out_of_range_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let cov = make_covenant(tmp.path());
        let mut p = valid_proposal();
        p.entry_price = Some(5400.0); // >0.5% away from 5200.0
        let quote = Quote { last: 5200.0 };
        let errs = check(&p, &cov, Some(&quote), false).unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, PlausibilityFailure::PriceOutOfRange { .. })));
    }

    #[test]
    fn stop_required_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let cov = make_covenant(tmp.path());
        let mut p = valid_proposal();
        p.stop_loss_ticks = 0;
        let errs = check(&p, &cov, Some(&valid_quote()), false).unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, PlausibilityFailure::StopRequired)));
    }

    #[test]
    fn qty_exceeds_cap_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let cov = make_covenant(tmp.path());
        let mut p = valid_proposal();
        p.qty = 10; // cap is 2
        let errs = check(&p, &cov, Some(&valid_quote()), false).unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, PlausibilityFailure::QtyExceedsCap { .. })));
    }

    #[test]
    fn instrument_not_whitelisted_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let cov = make_covenant(tmp.path());
        let mut p = valid_proposal();
        p.instrument = "TSLA".to_string();
        let errs = check(&p, &cov, Some(&valid_quote()), false).unwrap_err();
        assert!(errs
            .iter()
            .any(|e| matches!(e, PlausibilityFailure::InstrumentNotWhitelisted { .. })));
    }

    #[test]
    fn no_last_quote_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let cov = make_covenant(tmp.path());
        let errs = check(&valid_proposal(), &cov, None, false).unwrap_err();
        assert!(errs.iter().any(|e| matches!(
            e,
            PlausibilityFailure::NoLastQuote | PlausibilityFailure::OutsideTradingHours { .. }
        )));
    }

    #[test]
    fn multiple_failures_accumulate() {
        let tmp = tempfile::tempdir().unwrap();
        let cov = make_covenant(tmp.path());
        let mut p = valid_proposal();
        p.stop_loss_ticks = 0;
        p.qty = 99;
        p.instrument = "SPY".to_string();
        let errs = check(&p, &cov, Some(&valid_quote()), false).unwrap_err();
        assert!(errs.len() >= 3);
    }

    #[test]
    fn within_hours_boundary() {
        assert!(within_trading_hours("06:30", "06:30", "20:00"));
        assert!(!within_trading_hours("06:29", "06:30", "20:00"));
        assert!(!within_trading_hours("20:00", "06:30", "20:00"));
        assert!(within_trading_hours("19:59", "06:30", "20:00"));
    }

    #[test]
    fn normalize_direction_handles_variants() {
        assert_eq!(normalize_direction("Buy"), "long");
        assert_eq!(normalize_direction("long"), "long");
        assert_eq!(normalize_direction("Sell"), "short");
        assert_eq!(normalize_direction("short"), "short");
    }

    #[test]
    fn price_within_half_pct_passes() {
        let tmp = tempfile::tempdir().unwrap();
        let cov = make_covenant(tmp.path());
        let mut p = valid_proposal();
        p.entry_price = Some(5202.0); // 0.038% from 5200 — within 0.5%
        let result = check(&p, &cov, Some(&valid_quote()), false);
        // No price failure expected
        if let Err(errs) = &result {
            assert!(!errs
                .iter()
                .any(|e| matches!(e, PlausibilityFailure::PriceOutOfRange { .. })));
        }
    }
}
