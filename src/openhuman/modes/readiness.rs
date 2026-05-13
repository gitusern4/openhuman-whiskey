//! Pre-trade readiness check — §7 of INTELLIGENCE_SYNTHESIS.md.
//!
//! Scores the trader 0–5 across five psychological-readiness dimensions
//! (sleep, emotional baseline, session state, setup validity, invalidation
//! clarity). Produces a `psychology_readiness_factor` that feeds directly
//! into the Bayesian confidence formula in `intelligence.rs`.
//!
//! # Score → action mapping (§7)
//! | Score   | Action                                       |
//! |---------|----------------------------------------------|
//! | 4.5–5.0 | Full size                                    |
//! | 3.5–4.4 | 75% size                                     |
//! | 2.5–3.4 | 50% size + mandatory post-trade review       |
//! | < 2.5   | Trade BLOCKED                                |
//!
//! Q3 (session state) returning 0 — i.e. 2+ consecutive losses OR at the
//! daily loss limit — hard-blocks regardless of total score.
//!
//! # psychology_readiness_factor mapping
//! Mirrors §2's table:
//!   - 2+ consecutive losses → 0.80
//!   - Daily P&L < −1.5R → 0.70
//!   - Sleep < 6h → 0.75
//!   - 4+ consecutive wins (overconfidence) → 0.85
//!   - Otherwise → 1.00
//! The lowest applicable factor wins; factors compound by multiplication
//! when multiple conditions hold.

use serde::{Deserialize, Serialize};

// ── input struct ──────────────────────────────────────────────────────────────

/// All inputs required for the five-question readiness check.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadinessInput {
    /// Hours of sleep last night.
    pub sleep_hours: f64,
    /// Emotional baseline on a 1–5 scale (1=calm, 5=highly stressed).
    pub emotional_rating: u8,
    /// Number of consecutive losses in the current session.
    pub consecutive_losses: u32,
    /// Session P&L expressed in R-multiples (negative = loss).
    pub daily_pnl_r: f64,
    /// Daily hard-stop limit in R-multiples (positive value, e.g. 3.0).
    pub daily_loss_limit_r: f64,
    /// Whether the setup name is in the user's playbook.
    pub setup_in_playbook: bool,
    /// Whether stop, dollar risk, and invalidation condition are stated.
    pub invalidation_stated: bool,
    /// Number of consecutive wins in the current session (for overconfidence).
    pub consecutive_wins: u32,
}

// ── output struct ─────────────────────────────────────────────────────────────

/// Per-question score: Pass (1.0), Partial (0.5), or Fail (0.0).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestionScore {
    /// Raw score for this question (0.0 | 0.5 | 1.0).
    pub score: f64,
    /// True when this question triggers a hard-block (Q3 only).
    pub blocked: bool,
}

/// Full readiness evaluation output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReadinessScore {
    /// Q1: Sleep + physical state.
    pub q1_sleep: QuestionScore,
    /// Q2: Emotional baseline.
    pub q2_emotional: QuestionScore,
    /// Q3: Session state (consecutive losses / daily limit).
    pub q3_session: QuestionScore,
    /// Q4: Setup validity (in playbook + stop stated).
    pub q4_setup_validity: QuestionScore,
    /// Q5: Invalidation clarity.
    pub q5_invalidation: QuestionScore,

    /// Sum of all question scores (0.0–5.0).
    pub total: f64,

    /// True when the trade must be blocked (total < 2.5 OR Q3 hard-block).
    pub trade_blocked: bool,

    /// Multiplier to feed into the §3 Bayesian confidence formula.
    /// Computed as the product of all applicable §2 penalty factors.
    pub psychology_readiness_factor: f64,
}

// ── evaluation ────────────────────────────────────────────────────────────────

/// Evaluate pre-trade readiness and return a scored breakdown.
///
/// This is a pure function — no side-effects, no I/O.  Suitable for
/// both the Tauri command layer and direct Bayesian formula integration.
pub fn evaluate(input: &ReadinessInput) -> ReadinessScore {
    // Q1 — sleep + physical
    let q1 = if input.sleep_hours > 6.0 {
        QuestionScore {
            score: 1.0,
            blocked: false,
        }
    } else if input.sleep_hours >= 5.0 {
        QuestionScore {
            score: 0.5,
            blocked: false,
        }
    } else {
        QuestionScore {
            score: 0.0,
            blocked: false,
        }
    };

    // Q2 — emotional baseline (1–5 scale; 1–2 = calm)
    let q2 = if input.emotional_rating <= 2 {
        QuestionScore {
            score: 1.0,
            blocked: false,
        }
    } else if input.emotional_rating == 3 {
        QuestionScore {
            score: 0.5,
            blocked: false,
        }
    } else {
        QuestionScore {
            score: 0.0,
            blocked: false,
        }
    };

    // Q3 — session state: hard-block if 2+ consecutive losses OR at daily limit
    let at_daily_limit = -input.daily_pnl_r >= input.daily_loss_limit_r;
    let q3_hard_block = input.consecutive_losses >= 2 || at_daily_limit;
    let q3 = if q3_hard_block {
        // Hard-block: score = 0.0 AND blocked = true.
        QuestionScore {
            score: 0.0,
            blocked: true,
        }
    } else if input.consecutive_losses == 1 || -input.daily_pnl_r >= input.daily_loss_limit_r * 0.5
    {
        QuestionScore {
            score: 0.5,
            blocked: false,
        }
    } else {
        QuestionScore {
            score: 1.0,
            blocked: false,
        }
    };

    // Q4 — setup validity: in playbook + stop/risk stated
    let q4 = if input.setup_in_playbook && input.invalidation_stated {
        QuestionScore {
            score: 1.0,
            blocked: false,
        }
    } else if input.setup_in_playbook || input.invalidation_stated {
        QuestionScore {
            score: 0.5,
            blocked: false,
        }
    } else {
        QuestionScore {
            score: 0.0,
            blocked: false,
        }
    };

    // Q5 — invalidation clarity: same flag as Q4 but standalone dimension
    let q5 = if input.invalidation_stated {
        QuestionScore {
            score: 1.0,
            blocked: false,
        }
    } else {
        QuestionScore {
            score: 0.0,
            blocked: false,
        }
    };

    let total = q1.score + q2.score + q3.score + q4.score + q5.score;
    let trade_blocked = q3_hard_block || total < 2.5;

    // psychology_readiness_factor: product of all applicable §2 penalty factors.
    let mut factor = 1.0_f64;
    if input.consecutive_losses >= 2 {
        factor *= 0.80; // tilt threshold per §2
    }
    if -input.daily_pnl_r < -1.5 {
        // daily P&L < -1.5R → factor 0.70
        // (daily_pnl_r is negative for a loss, so -daily_pnl_r > 1.5)
    }
    if input.daily_pnl_r < -1.5 {
        factor *= 0.70;
    }
    if input.sleep_hours < 6.0 {
        factor *= 0.75;
    }
    if input.consecutive_wins >= 4 {
        factor *= 0.85; // overconfidence after win streak per §2
    }
    // Clamp to sensible floor (spec minimum shown as 0.40).
    factor = factor.max(0.40);

    ReadinessScore {
        q1_sleep: q1,
        q2_emotional: q2,
        q3_session: q3,
        q4_setup_validity: q4,
        q5_invalidation: q5,
        total,
        trade_blocked,
        psychology_readiness_factor: factor,
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn base_input() -> ReadinessInput {
        ReadinessInput {
            sleep_hours: 7.5,
            emotional_rating: 2,
            consecutive_losses: 0,
            daily_pnl_r: 0.0,
            daily_loss_limit_r: 3.0,
            setup_in_playbook: true,
            invalidation_stated: true,
            consecutive_wins: 0,
        }
    }

    // ── perfect readiness ────────────────────────────────────────────────────

    #[test]
    fn perfect_readiness_full_score() {
        let r = evaluate(&base_input());
        assert!((r.total - 5.0).abs() < 1e-10, "total={}", r.total);
        assert!(!r.trade_blocked);
        assert!((r.psychology_readiness_factor - 1.0).abs() < 1e-10);
    }

    // ── Q1: sleep tiers ──────────────────────────────────────────────────────

    #[test]
    fn q1_sleep_below_5h_is_fail() {
        let mut inp = base_input();
        inp.sleep_hours = 4.5;
        let r = evaluate(&inp);
        assert_eq!(r.q1_sleep.score, 0.0);
        assert!(!r.q1_sleep.blocked);
    }

    #[test]
    fn q1_sleep_5_to_6h_is_partial() {
        let mut inp = base_input();
        inp.sleep_hours = 5.5;
        let r = evaluate(&inp);
        assert!((r.q1_sleep.score - 0.5).abs() < 1e-10);
    }

    #[test]
    fn q1_sleep_above_6h_is_pass() {
        let mut inp = base_input();
        inp.sleep_hours = 7.0;
        let r = evaluate(&inp);
        assert!((r.q1_sleep.score - 1.0).abs() < 1e-10);
    }

    // ── Q2: emotional tiers ──────────────────────────────────────────────────

    #[test]
    fn q2_calm_rating_1_or_2_is_pass() {
        let mut inp = base_input();
        inp.emotional_rating = 1;
        assert!((evaluate(&inp).q2_emotional.score - 1.0).abs() < 1e-10);
        inp.emotional_rating = 2;
        assert!((evaluate(&inp).q2_emotional.score - 1.0).abs() < 1e-10);
    }

    #[test]
    fn q2_rating_3_is_partial() {
        let mut inp = base_input();
        inp.emotional_rating = 3;
        assert!((evaluate(&inp).q2_emotional.score - 0.5).abs() < 1e-10);
    }

    #[test]
    fn q2_rating_4_or_5_is_fail() {
        let mut inp = base_input();
        inp.emotional_rating = 4;
        assert_eq!(evaluate(&inp).q2_emotional.score, 0.0);
        inp.emotional_rating = 5;
        assert_eq!(evaluate(&inp).q2_emotional.score, 0.0);
    }

    // ── Q3: session state hard-block ─────────────────────────────────────────

    #[test]
    fn q3_two_consecutive_losses_hard_blocks() {
        let mut inp = base_input();
        inp.consecutive_losses = 2;
        let r = evaluate(&inp);
        assert_eq!(r.q3_session.score, 0.0);
        assert!(r.q3_session.blocked, "Q3 must set blocked=true");
        assert!(r.trade_blocked, "trade_blocked must be true");
    }

    #[test]
    fn q3_at_daily_limit_hard_blocks() {
        let mut inp = base_input();
        inp.daily_pnl_r = -3.0; // exactly at 3R daily limit
        inp.daily_loss_limit_r = 3.0;
        let r = evaluate(&inp);
        assert!(r.q3_session.blocked);
        assert!(r.trade_blocked);
    }

    #[test]
    fn q3_one_loss_is_partial() {
        let mut inp = base_input();
        inp.consecutive_losses = 1;
        let r = evaluate(&inp);
        assert!((r.q3_session.score - 0.5).abs() < 1e-10);
        assert!(!r.q3_session.blocked);
    }

    #[test]
    fn q3_clean_session_is_pass() {
        let r = evaluate(&base_input());
        assert!((r.q3_session.score - 1.0).abs() < 1e-10);
        assert!(!r.q3_session.blocked);
    }

    // ── total < 2.5 blocks trade ─────────────────────────────────────────────

    #[test]
    fn total_below_2pt5_blocks_trade() {
        // Sleep <5h (0), emotional 5 (0), 0 losses (1.0), not in playbook (0),
        // no invalidation (0) → total = 1.0 < 2.5
        let inp = ReadinessInput {
            sleep_hours: 4.0,
            emotional_rating: 5,
            consecutive_losses: 0,
            daily_pnl_r: 0.0,
            daily_loss_limit_r: 3.0,
            setup_in_playbook: false,
            invalidation_stated: false,
            consecutive_wins: 0,
        };
        let r = evaluate(&inp);
        assert!(r.total < 2.5, "total={}", r.total);
        assert!(r.trade_blocked);
    }

    // ── psychology_readiness_factor: sleep penalty ───────────────────────────

    #[test]
    fn factor_sleep_less_than_6h_applies_075_penalty() {
        let mut inp = base_input();
        inp.sleep_hours = 5.0;
        let r = evaluate(&inp);
        // Only sleep penalty applies → 1.0 × 0.75 = 0.75
        assert!(
            (r.psychology_readiness_factor - 0.75).abs() < 1e-10,
            "factor={}",
            r.psychology_readiness_factor
        );
    }

    // ── psychology_readiness_factor: consecutive wins ────────────────────────

    #[test]
    fn factor_four_consecutive_wins_applies_085_penalty() {
        let mut inp = base_input();
        inp.consecutive_wins = 4;
        let r = evaluate(&inp);
        assert!(
            (r.psychology_readiness_factor - 0.85).abs() < 1e-10,
            "factor={}",
            r.psychology_readiness_factor
        );
    }

    // ── psychology_readiness_factor: daily pnl penalty ───────────────────────

    #[test]
    fn factor_daily_pnl_below_neg_1pt5r_applies_070_penalty() {
        let mut inp = base_input();
        inp.daily_pnl_r = -2.0; // -2R < -1.5R threshold
        inp.consecutive_losses = 0; // no tilt block
        let r = evaluate(&inp);
        assert!(
            (r.psychology_readiness_factor - 0.70).abs() < 1e-10,
            "factor={}",
            r.psychology_readiness_factor
        );
    }
}
