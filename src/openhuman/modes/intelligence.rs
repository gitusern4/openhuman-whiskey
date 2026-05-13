//! Bayesian intelligence layer — §2/§3/§4 of INTELLIGENCE_SYNTHESIS.md.
//!
//! Replaces vibes-grading with an auditable, math-grounded confidence
//! score derived from sequential log-odds updates. Each evidence source
//! (order flow, time-of-day, regime, psychology, sample uncertainty)
//! contributes a measurable log-likelihood ratio. The result maps to a
//! letter grade and a position-size multiplier.
//!
//! # Formula (§2)
//! ```text
//! log_odds = logit(P_base)
//!          + log(LR_order_flow)
//!          + log(time_of_day_factor)
//!          + log(regime_match_factor)
//!          + log(psychology_readiness_factor)
//!          - log(1 / (1 - uncertainty_penalty))
//!
//! P(win | signals) = 1 / (1 + exp(-log_odds))
//! ```
//! where `P_base = (wins + 0.5) / (N + 1.0)` (Laplace smoothing).
//!
//! # Grade mapping (§2)
//! | P(win)     | Grade | Size multiplier |
//! |------------|-------|-----------------|
//! | ≥ 0.80     | A+    | 1.00            |
//! | 0.65–0.79  | A     | 0.75            |
//! | 0.50–0.64  | B     | 0.50            |
//! | 0.43–0.49  | C     | 0.25            |
//! | < 0.43     | Pass  | 0.00            |
//!
//! # Sample-size tiers (§4)
//! | N       | Tier           | Max grade | Size cap |
//! |---------|----------------|-----------|----------|
//! | 0–19    | Hypothesis     | B         | 50%      |
//! | 20–49   | Developing     | A         | 75%      |
//! | 50–99   | Validated      | A+        | 100%     |
//! | 100+    | HighConfidence | A+        | 100%+    |
//!
//! # Performance budget
//! Every public function executes in < 1 ms for typical inputs.
//! Beta CI uses the Wilson-interval closed-form approximation (no
//! external stats crate required) so latency is O(1) arithmetic.

use serde::{Deserialize, Serialize};

// ── types ─────────────────────────────────────────────────────────────────────

/// Historical performance stats for a setup in the user's playbook.
#[derive(Debug, Clone, PartialEq)]
pub struct SetupStats {
    /// Total instances logged.
    pub n: u32,
    /// Wins logged.
    pub wins: u32,
}

/// Bundle of market-microstructure evidence (from order_flow module).
#[derive(Debug, Clone, PartialEq)]
pub struct EvidenceBundle {
    /// P(evidence | win) / P(evidence | loss) for composite order-flow
    /// signal. Typical range 0.3–3.5 per §2 table.
    pub lr_order_flow: f64,
}

/// Macro-regime context: price relative to VWAP, vol regime, time bucket.
#[derive(Debug, Clone, PartialEq)]
pub struct RegimeContext {
    /// time_of_day_factor from §2 (0.75–1.20).
    pub time_of_day_factor: f64,
    /// regime_match_factor from §2 (0.65–1.30).
    pub regime_match_factor: f64,
}

/// Psychology readiness score from `readiness::evaluate`.
/// This is the multiplier field produced there; also 0.40–1.00.
#[derive(Debug, Clone, PartialEq)]
pub struct ReadinessScore {
    /// Factor fed into the Bayesian formula (0.40–1.00).
    pub psychology_readiness_factor: f64,
}

/// Output of [`compute_confidence`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConfidenceResult {
    /// Posterior P(win | signals), 0.0–1.0.
    pub p_win: f64,
    /// Laplace-smoothed base win rate before updating.
    pub p_base: f64,
    /// Half-width of 95% Beta credible interval (Wilson approximation).
    pub uncertainty_penalty: f64,
    /// Log-odds at each stage (for auditability).
    pub log_odds_final: f64,
    /// Letter grade derived from `p_win`.
    pub grade: Grade,
}

/// Letter grade for a setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Grade {
    /// P(win) >= 0.80
    APlus,
    /// 0.65 <= P(win) < 0.80
    A,
    /// 0.50 <= P(win) < 0.65
    B,
    /// 0.43 <= P(win) < 0.50
    C,
    /// P(win) < 0.43 — trade should be passed
    Pass,
}

impl std::fmt::Display for Grade {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Grade::APlus => write!(f, "A+"),
            Grade::A => write!(f, "A"),
            Grade::B => write!(f, "B"),
            Grade::C => write!(f, "C"),
            Grade::Pass => write!(f, "Pass"),
        }
    }
}

/// Sample-size tier (§4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tier {
    /// 0–19 instances. Max grade B, size cap 50%.
    Hypothesis,
    /// 20–49 instances. Max grade A, size cap 75%.
    Developing,
    /// 50–99 instances. Max grade A+, size cap 100%.
    Validated,
    /// 100+ instances. Full conviction sizing.
    HighConfidence,
}

// ── core math helpers ─────────────────────────────────────────────────────────

/// logit(p) = ln(p / (1 - p)).  Clamps p into (1e-9, 1 - 1e-9) to avoid ±∞.
#[inline]
fn logit(p: f64) -> f64 {
    let p = p.clamp(1e-9, 1.0 - 1e-9);
    (p / (1.0 - p)).ln()
}

/// Inverse logit / sigmoid.
#[inline]
fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

/// Wilson-score 95% CI half-width for a Beta(wins+0.5, losses+0.5) posterior.
///
/// Uses the Wilson score interval with z=1.96 as a fast closed-form
/// approximation that is accurate enough for sizing decisions (±1–2
/// percentage-points relative to exact Beta quantiles for N ≥ 5).
///
/// For N=0 (degenerate) returns 0.5 so the penalty term maxes out.
pub fn beta_ci_half_width(wins: u32, n: u32) -> f64 {
    if n == 0 {
        return 0.5;
    }
    // Laplace-smoothed p_hat
    let p_hat = (wins as f64 + 0.5) / (n as f64 + 1.0);
    let n_f = n as f64 + 1.0; // effective N after smoothing
    let z = 1.96_f64;
    let denom = 1.0 + z * z / n_f;
    let centre = (p_hat + z * z / (2.0 * n_f)) / denom;
    let spread = z / denom * (p_hat * (1.0 - p_hat) / n_f + z * z / (4.0 * n_f * n_f)).sqrt();
    // Return spread as the half-width; clamp to [0, 0.5].
    (centre - (centre - spread)).abs().clamp(0.0, 0.5)
}

// ── public API ────────────────────────────────────────────────────────────────

/// Compute the Bayesian posterior confidence score for a setup.
///
/// Implements the §2 sequential log-odds formula:
/// ```text
/// log_odds = logit(P_base)
///          + log(LR_order_flow)
///          + log(time_of_day_factor)
///          + log(regime_match_factor)
///          + log(psychology_readiness_factor)
///          - log(1 / (1 - uncertainty_penalty))
/// ```
///
/// All LR/factor inputs are clamped to a minimum of 1e-9 before taking
/// the log so pathological zeros don't produce -∞.
pub fn compute_confidence(
    setup: &SetupStats,
    evidence: &EvidenceBundle,
    regime: &RegimeContext,
    psychology: &ReadinessScore,
) -> ConfidenceResult {
    // P_base — Laplace-smoothed win rate.
    let p_base = (setup.wins as f64 + 0.5) / (setup.n as f64 + 1.0);

    // Uncertainty penalty = half-width of 95% Beta CI.
    let uncertainty_penalty = beta_ci_half_width(setup.wins, setup.n);

    // Sequential log-odds update.
    let log_odds = logit(p_base)
        + evidence.lr_order_flow.max(1e-9).ln()
        + regime.time_of_day_factor.max(1e-9).ln()
        + regime.regime_match_factor.max(1e-9).ln()
        + psychology.psychology_readiness_factor.max(1e-9).ln()
        - (1.0 / (1.0 - uncertainty_penalty.clamp(0.0, 1.0 - 1e-9))).ln();

    let p_win = sigmoid(log_odds);
    let grade = grade_from_confidence(p_win, 0.0, 0.0);

    ConfidenceResult {
        p_win,
        p_base,
        uncertainty_penalty,
        log_odds_final: log_odds,
        grade,
    }
}

/// Map a posterior P(win) to a letter grade per §2's table.
///
/// `R_multiple` and `commission_drag_R` are accepted for future use in
/// break-even math but are not used in grade assignment (grade is solely
/// a function of P(win) per the spec).
pub fn grade_from_confidence(p: f64, _r_multiple: f64, _commission_drag_r: f64) -> Grade {
    if p >= 0.80 {
        Grade::APlus
    } else if p >= 0.65 {
        Grade::A
    } else if p >= 0.50 {
        Grade::B
    } else if p >= 0.43 {
        Grade::C
    } else {
        Grade::Pass
    }
}

/// Classify a setup into a sample-size tier (§4).
pub fn sample_size_tier(n: u32) -> Tier {
    match n {
        0..=19 => Tier::Hypothesis,
        20..=49 => Tier::Developing,
        50..=99 => Tier::Validated,
        _ => Tier::HighConfidence,
    }
}

/// Combined grade + tier → position-size multiplier (0.0–1.0).
///
/// The tier imposes a cap; the grade drives the base fraction.
/// A Hypothesis setup can never exceed 0.50 regardless of grade.
pub fn position_size_multiplier(grade: Grade, tier: Tier) -> f64 {
    let grade_frac = match grade {
        Grade::APlus => 1.00,
        Grade::A => 0.75,
        Grade::B => 0.50,
        Grade::C => 0.25,
        Grade::Pass => 0.00,
    };
    let tier_cap = match tier {
        Tier::Hypothesis => 0.50,
        Tier::Developing => 0.75,
        Tier::Validated => 1.00,
        Tier::HighConfidence => 1.00,
    };
    grade_frac.min(tier_cap)
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── helpers ──────────────────────────────────────────────────────────────

    fn setup(wins: u32, n: u32) -> SetupStats {
        SetupStats { n, wins }
    }

    fn evidence(lr: f64) -> EvidenceBundle {
        EvidenceBundle { lr_order_flow: lr }
    }

    fn regime(tod: f64, reg: f64) -> RegimeContext {
        RegimeContext {
            time_of_day_factor: tod,
            regime_match_factor: reg,
        }
    }

    fn psych(factor: f64) -> ReadinessScore {
        ReadinessScore {
            psychology_readiness_factor: factor,
        }
    }

    // ── logit / sigmoid round-trip ────────────────────────────────────────────

    #[test]
    fn logit_sigmoid_round_trip() {
        for p in [0.3, 0.5, 0.7, 0.85] {
            let lo = logit(p);
            let back = sigmoid(lo);
            assert!(
                (back - p).abs() < 1e-10,
                "round-trip failed for p={p}: got {back}"
            );
        }
    }

    #[test]
    fn logit_clamps_near_zero() {
        // Should not return -inf for p=0.0
        let lo = logit(0.0);
        assert!(lo.is_finite(), "logit(0) must not be -inf");
    }

    #[test]
    fn logit_clamps_near_one() {
        let lo = logit(1.0);
        assert!(lo.is_finite(), "logit(1) must not be +inf");
    }

    // ── beta_ci_half_width ───────────────────────────────────────────────────

    #[test]
    fn beta_ci_n0_returns_half() {
        assert!((beta_ci_half_width(0, 0) - 0.5).abs() < 1e-10);
    }

    #[test]
    fn beta_ci_shrinks_with_large_n() {
        let w100 = beta_ci_half_width(60, 100);
        let w10 = beta_ci_half_width(6, 10);
        assert!(
            w100 < w10,
            "CI should narrow with more data: w100={w100}, w10={w10}"
        );
    }

    #[test]
    fn beta_ci_bounded_zero_to_half() {
        for (wins, n) in [(0, 5), (5, 5), (3, 10), (50, 100)] {
            let h = beta_ci_half_width(wins, n);
            assert!(
                (0.0..=0.5).contains(&h),
                "CI half-width out of bounds: wins={wins} n={n} h={h}"
            );
        }
    }

    // ── NQ delta-divergence worked example from §2 ────────────────────────────
    //
    // The spec states "the NQ delta divergence example MUST produce P ≈ 0.85".
    // That example has:
    //   - base rate 62% (absorption+delta-div setup, §5 #2) → N=100, wins=62
    //   - LR_order_flow = 2.5 (strong composite signal)
    //   - time_of_day = 1.10 (AM trend window)
    //   - regime_match = 1.20 (trending, ATR expanding)
    //   - psychology = 1.00 (full readiness)
    //   - uncertainty_penalty small (N=100, CI narrow)
    //
    // Expected: P(win | signals) ≈ 0.85 (within ±0.05 of 0.85).

    #[test]
    fn nq_delta_divergence_worked_example_produces_p_approx_085() {
        let result = compute_confidence(
            &setup(62, 100),
            &evidence(2.5),
            &regime(1.10, 1.20),
            &psych(1.00),
        );
        let p = result.p_win;
        assert!(
            (p - 0.85).abs() < 0.05,
            "NQ delta-div example: expected P ≈ 0.85, got {p:.4}"
        );
    }

    // ── N=0 edge case ────────────────────────────────────────────────────────

    #[test]
    fn n_zero_returns_finite_result() {
        let result =
            compute_confidence(&setup(0, 0), &evidence(1.0), &regime(1.0, 1.0), &psych(1.0));
        assert!(result.p_win.is_finite());
        assert!(result.p_win >= 0.0 && result.p_win <= 1.0);
    }

    // ── all-LRs-1.0 returns base (no evidence update) ────────────────────────

    #[test]
    fn all_neutral_factors_returns_near_base_adjusted_for_uncertainty() {
        // With all LR factors = 1.0, log(LR) = 0 for each.
        // The only deviation from P_base comes from the uncertainty penalty.
        // So p_win should be < p_base (penalty pulls it down).
        let stats = setup(30, 50);
        let p_base = (30.0 + 0.5) / (50.0 + 1.0);
        let result = compute_confidence(&stats, &evidence(1.0), &regime(1.0, 1.0), &psych(1.0));
        // p_win must be strictly below p_base (uncertainty penalty applied)
        assert!(
            result.p_win < p_base,
            "With all neutral LRs, uncertainty penalty must reduce p_win below p_base. \
             p_base={p_base:.4} p_win={result.p_win:.4}"
        );
    }

    // ── extreme uncertainty: N=1 ─────────────────────────────────────────────

    #[test]
    fn extreme_uncertainty_large_penalty_lowers_confidence() {
        // N=1, wins=1: wide CI → large uncertainty penalty.
        let low_n =
            compute_confidence(&setup(1, 1), &evidence(2.0), &regime(1.0, 1.0), &psych(1.0));
        // N=100, wins=60: narrow CI → smaller penalty.
        let high_n = compute_confidence(
            &setup(60, 100),
            &evidence(2.0),
            &regime(1.0, 1.0),
            &psych(1.0),
        );
        assert!(
            low_n.uncertainty_penalty > high_n.uncertainty_penalty,
            "N=1 should have larger uncertainty penalty than N=100"
        );
        assert!(
            low_n.p_win < high_n.p_win,
            "N=1 should produce lower p_win than N=100 for identical signals"
        );
    }

    // ── psychology tilt penalty ───────────────────────────────────────────────

    #[test]
    fn tilt_factor_reduces_confidence() {
        let full = compute_confidence(
            &setup(40, 60),
            &evidence(1.5),
            &regime(1.0, 1.0),
            &psych(1.00),
        );
        let tilted = compute_confidence(
            &setup(40, 60),
            &evidence(1.5),
            &regime(1.0, 1.0),
            &psych(0.80), // 2 consec losses penalty from §2
        );
        assert!(
            tilted.p_win < full.p_win,
            "Tilt factor 0.80 must reduce p_win. full={:.4} tilted={:.4}",
            full.p_win,
            tilted.p_win
        );
    }

    // ── lunch penalty ────────────────────────────────────────────────────────

    #[test]
    fn lunch_window_factor_reduces_confidence() {
        let am_trend = compute_confidence(
            &setup(40, 60),
            &evidence(1.5),
            &regime(1.20, 1.0),
            &psych(1.0),
        );
        let lunch = compute_confidence(
            &setup(40, 60),
            &evidence(1.5),
            &regime(0.75, 1.0), // lunch window per §2 table
            &psych(1.0),
        );
        assert!(
            lunch.p_win < am_trend.p_win,
            "Lunch window must lower p_win vs AM trend"
        );
    }

    // ── grade_from_confidence boundary conditions ─────────────────────────────

    #[test]
    fn grade_thresholds_exact_boundaries() {
        assert_eq!(grade_from_confidence(0.80, 0.0, 0.0), Grade::APlus);
        assert_eq!(grade_from_confidence(0.799, 0.0, 0.0), Grade::A);
        assert_eq!(grade_from_confidence(0.65, 0.0, 0.0), Grade::A);
        assert_eq!(grade_from_confidence(0.649, 0.0, 0.0), Grade::B);
        assert_eq!(grade_from_confidence(0.50, 0.0, 0.0), Grade::B);
        assert_eq!(grade_from_confidence(0.499, 0.0, 0.0), Grade::C);
        assert_eq!(grade_from_confidence(0.43, 0.0, 0.0), Grade::C);
        assert_eq!(grade_from_confidence(0.429, 0.0, 0.0), Grade::Pass);
        assert_eq!(grade_from_confidence(0.0, 0.0, 0.0), Grade::Pass);
    }

    // ── sample_size_tier ─────────────────────────────────────────────────────

    #[test]
    fn sample_size_tier_boundaries() {
        assert_eq!(sample_size_tier(0), Tier::Hypothesis);
        assert_eq!(sample_size_tier(19), Tier::Hypothesis);
        assert_eq!(sample_size_tier(20), Tier::Developing);
        assert_eq!(sample_size_tier(49), Tier::Developing);
        assert_eq!(sample_size_tier(50), Tier::Validated);
        assert_eq!(sample_size_tier(99), Tier::Validated);
        assert_eq!(sample_size_tier(100), Tier::HighConfidence);
        assert_eq!(sample_size_tier(999), Tier::HighConfidence);
    }

    // ── position_size_multiplier ─────────────────────────────────────────────

    #[test]
    fn multiplier_hypothesis_caps_at_50pct() {
        // APlus grade but Hypothesis tier → capped at 0.50.
        let m = position_size_multiplier(Grade::APlus, Tier::Hypothesis);
        assert!((m - 0.50).abs() < 1e-10, "Hypothesis cap: {m}");
    }

    #[test]
    fn multiplier_developing_caps_at_75pct() {
        let m = position_size_multiplier(Grade::APlus, Tier::Developing);
        assert!((m - 0.75).abs() < 1e-10, "Developing cap: {m}");
    }

    #[test]
    fn multiplier_validated_aplus_full_size() {
        let m = position_size_multiplier(Grade::APlus, Tier::Validated);
        assert!((m - 1.00).abs() < 1e-10, "Validated A+: {m}");
    }

    #[test]
    fn multiplier_pass_always_zero() {
        for tier in [
            Tier::Hypothesis,
            Tier::Developing,
            Tier::Validated,
            Tier::HighConfidence,
        ] {
            let m = position_size_multiplier(Grade::Pass, tier);
            assert_eq!(m, 0.0, "Pass grade must always produce 0 multiplier");
        }
    }

    #[test]
    fn multiplier_a_grade_developing_caps_at_75pct() {
        // A grade = 0.75 frac, Developing tier cap = 0.75 → min(0.75, 0.75) = 0.75
        let m = position_size_multiplier(Grade::A, Tier::Developing);
        assert!((m - 0.75).abs() < 1e-10);
    }

    #[test]
    fn multiplier_b_grade_hypothesis_caps_at_50pct() {
        // B = 0.50, Hypothesis cap = 0.50 → 0.50
        let m = position_size_multiplier(Grade::B, Tier::Hypothesis);
        assert!((m - 0.50).abs() < 1e-10);
    }

    #[test]
    fn multiplier_c_grade_hypothesis_uses_grade_frac() {
        // C = 0.25, Hypothesis cap = 0.50 → min(0.25, 0.50) = 0.25
        let m = position_size_multiplier(Grade::C, Tier::Hypothesis);
        assert!((m - 0.25).abs() < 1e-10);
    }
}
