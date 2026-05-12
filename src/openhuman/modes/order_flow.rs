//! Order-flow analysis — pure Rust logic layer.
//!
//! This module owns:
//!   - `OrderFlowConfig` — which features are enabled and at what sensitivity.
//!   - `OrderFlowState` — live per-session state (ring-buffer of bar deltas,
//!     cumulative delta, VAH/VAL/POC if known, in-memory tag list).
//!   - Pure computation functions: `compute_cumulative_delta`,
//!     `detect_delta_divergence`, `detect_absorption`, `tag_active_trade`.
//!   - Workspace preset table mapping preset names to TradingView indicator ids.
//!
//! Security invariants
//! -------------------
//! Tag values and preset names provided by the caller (which may be an
//! LLM-generated string from the frontend) are gated through const allowlists
//! before any state mutation or TV bridge call. Never extend by passing
//! arbitrary caller strings through.
//!
//! Polling rate is capped at 2 Hz in `OrderFlowConfig` — this matches
//! `polling_hz` validation in the setter.  Values above 2 are clamped.

use serde::{Deserialize, Serialize};

// ─── constants ───────────────────────────────────────────────────────────────

/// Maximum entries in the per-session tag list.
pub const TAG_CAP: usize = 500;

/// Maximum entries in the bar-delta ring buffer.
pub const BAR_RING_CAP: usize = 200;

/// Maximum polling rate in Hz. Enforced in `OrderFlowConfig` validation.
pub const MAX_POLLING_HZ: u8 = 2;

/// Allowed tag ids.  Every value the caller passes through
/// `tag_active_trade` / `order_flow_tag_active_trade` must appear here.
pub const ALLOWED_TAGS: &[&str] = &[
    "absorbed",
    "delta_div",
    "single_print",
    "value_area_reject",
    "responsive_buyer",
    "responsive_seller",
];

/// Workspace preset table.  Tuple: (preset_name, indicator_ids).
/// `order_flow_apply_preset` looks up by name from this table.
pub const WORKSPACE_PRESETS: &[(&str, &[&str])] = &[
    (
        "vwap_profile_anchored",
        &["VWAP", "Volume Profile (Visible Range)", "Anchored VWAP"],
    ),
    ("standard_orderflow", &["VWAP", "Volume Profile (Session)"]),
    ("delta_focused", &["VWAP", "Cumulative Volume Delta"]),
];

// ─── config ──────────────────────────────────────────────────────────────────

/// Persisted feature-flag + sensitivity configuration for order-flow tools.
///
/// Mirrors the TypeScript `OrderFlowConfig` type owned by the main builder
/// in `app/src/types/orderFlow.ts`.  If that type hasn't landed yet, this
/// struct is the authoritative shape — see `CONTRACT.md` at the branch root.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OrderFlowConfig {
    /// Show workspace preset switcher in the TK's Mods panel.
    #[serde(default = "default_true")]
    pub workspace_presets: bool,

    /// Show the manual cumulative-delta tracker widget.
    #[serde(default = "default_true")]
    pub manual_delta_tracker: bool,

    /// Show color-coded tag chips on the active-trade card.
    #[serde(default = "default_true")]
    pub tag_chips: bool,

    /// Fire desktop notifications on detected divergence / absorption events.
    #[serde(default)]
    pub detection_alerts: bool,

    /// How often the UI polls `order_flow_record_bar` in Hz.
    /// Clamped to [`MAX_POLLING_HZ`] on save/set.
    #[serde(default = "default_polling_hz")]
    pub polling_hz: u8,

    /// Minimum number of swing-high bars required to call divergence.
    #[serde(default = "default_divergence_lookback")]
    pub divergence_lookback: usize,

    /// Multiplier above EMA volume to call absorption (default 2.0).
    #[serde(default = "default_absorption_volume_multiplier")]
    pub absorption_volume_multiplier: f64,

    /// ATR fraction threshold: if bar range ÷ ATR is above this value the
    /// bar is NOT considered narrow — absorption requires a narrow range.
    #[serde(default = "default_absorption_range_fraction")]
    pub absorption_range_fraction: f64,
}

fn default_true() -> bool {
    true
}
fn default_polling_hz() -> u8 {
    1
}
fn default_divergence_lookback() -> usize {
    5
}
fn default_absorption_volume_multiplier() -> f64 {
    2.0
}
fn default_absorption_range_fraction() -> f64 {
    0.5
}

impl Default for OrderFlowConfig {
    fn default() -> Self {
        Self {
            workspace_presets: true,
            manual_delta_tracker: true,
            tag_chips: true,
            detection_alerts: false,
            polling_hz: default_polling_hz(),
            divergence_lookback: default_divergence_lookback(),
            absorption_volume_multiplier: default_absorption_volume_multiplier(),
            absorption_range_fraction: default_absorption_range_fraction(),
        }
    }
}

impl OrderFlowConfig {
    /// Clamp `polling_hz` to [`MAX_POLLING_HZ`] and return the validated copy.
    pub fn validated(mut self) -> Self {
        if self.polling_hz > MAX_POLLING_HZ {
            self.polling_hz = MAX_POLLING_HZ;
        }
        if self.polling_hz == 0 {
            self.polling_hz = 1;
        }
        self
    }
}

// ─── data types ──────────────────────────────────────────────────────────────

/// One bar's worth of order-flow data, provided by the frontend bridge.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BarDelta {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    /// Volume traded on the bid side.
    pub bid_vol: u64,
    /// Volume traded on the ask side.
    pub ask_vol: u64,
}

impl BarDelta {
    /// Ask minus bid for this bar.  Positive = net buying pressure.
    pub fn delta(&self) -> i64 {
        (self.ask_vol as i64).saturating_sub(self.bid_vol as i64)
    }

    /// High − Low range.
    pub fn range(&self) -> f64 {
        self.high - self.low
    }
}

/// A detected divergence between price swing-highs and CVD swing-highs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DivergenceSignal {
    /// Direction of price relative to the prior swing-high.
    /// `true` = bearish divergence (price higher, CVD lower).
    pub bearish: bool,
    /// Price high at the most recent swing-high.
    pub price_high: f64,
    /// CVD value at the most recent swing-high.
    pub cvd_high: i64,
    /// Price high at the prior swing-high.
    pub prior_price_high: f64,
    /// CVD value at the prior swing-high.
    pub prior_cvd_high: i64,
}

/// A trade tag applied by the user mid-session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OrderFlowTag {
    /// One of the ids in [`ALLOWED_TAGS`].
    pub id: String,
    /// Unix-ms timestamp at the moment the tag was applied.
    pub ts_ms: u64,
}

/// Live per-session order-flow state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderFlowState {
    /// Current bar delta (ask_vol − bid_vol of the most recent bar).
    pub current_bar_delta: i64,

    /// Cumulative session delta — updated on each `record_bar` call.
    pub cumulative_session_delta: i64,

    /// Ring buffer of the last N bar deltas (capped at [`BAR_RING_CAP`]).
    pub bar_deltas: Vec<i64>,

    /// Volume Area High from the most recent profile update (if known).
    pub vah: Option<f64>,

    /// Volume Area Low from the most recent profile update (if known).
    pub val: Option<f64>,

    /// Point of Control from the most recent profile update (if known).
    pub poc: Option<f64>,

    /// In-memory tag list, capped at [`TAG_CAP`].
    pub tags: Vec<OrderFlowTag>,

    /// Raw bar history (capped at [`BAR_RING_CAP`]) for detection functions.
    pub bars: Vec<BarDelta>,
}

impl Default for OrderFlowState {
    fn default() -> Self {
        Self {
            current_bar_delta: 0,
            cumulative_session_delta: 0,
            bar_deltas: Vec::new(),
            vah: None,
            val: None,
            poc: None,
            tags: Vec::new(),
            bars: Vec::new(),
        }
    }
}

// ─── pure functions ───────────────────────────────────────────────────────────

/// Sum per-bar deltas with overflow saturation at `i64::MAX` / `i64::MIN`.
///
/// Zero bars → 0.  Single bar → that bar's delta.  Overflow clamps rather
/// than wrapping.
pub fn compute_cumulative_delta(bars: &[BarDelta]) -> i64 {
    bars.iter()
        .fold(0_i64, |acc, bar| acc.saturating_add(bar.delta()))
}

/// Detect bearish or bullish divergence between sequential price swing-highs
/// and the corresponding CVD values at those highs.
///
/// Algorithm:
///   1. Treat `price_highs[i]` and `cum_deltas[i]` as co-indexed points.
///   2. Scan backwards from the last element to find two consecutive
///      local maxima in `price_highs` separated by at least one lower bar
///      (simple swing-high detection: value > both neighbours).
///   3. If price made a higher swing-high while CVD made a lower swing-high
///      → bearish divergence.
///      If price made a lower swing-high while CVD made a higher swing-high
///      → bullish divergence.
///   4. Neither → `None`.
///
/// Returns `None` when fewer than 3 data points are supplied (can't
/// establish a swing-high).
pub fn detect_delta_divergence(
    price_highs: &[f64],
    cum_deltas: &[i64],
) -> Option<DivergenceSignal> {
    let n = price_highs.len().min(cum_deltas.len());
    if n < 3 {
        return None;
    }

    // Walk backwards collecting swing-high indices (local maxima).
    let mut swing_highs: Vec<usize> = Vec::new();
    for i in (1..n - 1).rev() {
        if price_highs[i] > price_highs[i - 1] && price_highs[i] > price_highs[i + 1] {
            swing_highs.push(i);
            if swing_highs.len() == 2 {
                break;
            }
        }
    }

    if swing_highs.len() < 2 {
        return None;
    }

    // swing_highs[0] is the more recent swing; swing_highs[1] is the prior.
    let recent = swing_highs[0];
    let prior = swing_highs[1];

    let price_recent = price_highs[recent];
    let price_prior = price_highs[prior];
    let cvd_recent = cum_deltas[recent];
    let cvd_prior = cum_deltas[prior];

    if price_recent > price_prior && cvd_recent < cvd_prior {
        // Higher price high, lower CVD high → bearish divergence.
        return Some(DivergenceSignal {
            bearish: true,
            price_high: price_recent,
            cvd_high: cvd_recent,
            prior_price_high: price_prior,
            prior_cvd_high: cvd_prior,
        });
    }

    if price_recent < price_prior && cvd_recent > cvd_prior {
        // Lower price high, higher CVD high → bullish divergence.
        return Some(DivergenceSignal {
            bearish: false,
            price_high: price_recent,
            cvd_high: cvd_recent,
            prior_price_high: price_prior,
            prior_cvd_high: cvd_prior,
        });
    }

    None
}

/// Return `true` when the bar exhibits absorption: unusually high volume
/// AND a narrow price range relative to the ATR.
///
/// Conditions (both must hold):
///   - total volume (`bid_vol + ask_vol`) ≥ `ema_volume × config.absorption_volume_multiplier`
///   - bar range ÷ ATR ≤ `config.absorption_range_fraction`
///
/// `atr` is the caller's rolling average true range estimate; passing 0.0
/// disables the range check (always narrow) — callers should guard on their
/// side.
pub fn detect_absorption(bar: &BarDelta, ema_volume: f64, atr: f64, cfg: &OrderFlowConfig) -> bool {
    let total_vol = bar.bid_vol.saturating_add(bar.ask_vol) as f64;
    let volume_ok = total_vol >= ema_volume * cfg.absorption_volume_multiplier;

    let range_ok = if atr <= 0.0 {
        true // degenerate: skip range check
    } else {
        bar.range() / atr <= cfg.absorption_range_fraction
    };

    volume_ok && range_ok
}

/// Append an `OrderFlowTag` to the state's in-memory list.
///
/// Returns `Err` when:
///   - `tag.id` is not in [`ALLOWED_TAGS`] (security gate)
///   - the tag list is already at [`TAG_CAP`] (back-pressure)
pub fn tag_active_trade(state: &mut OrderFlowState, tag: OrderFlowTag) -> Result<(), String> {
    if !ALLOWED_TAGS.contains(&tag.id.as_str()) {
        return Err(format!(
            "unknown tag id {:?}; allowed: {:?}",
            tag.id, ALLOWED_TAGS
        ));
    }
    if state.tags.len() >= TAG_CAP {
        return Err(format!(
            "tag list full ({TAG_CAP} entries); rotate session to clear"
        ));
    }
    state.tags.push(tag);
    Ok(())
}

/// Record a new bar into `state`, advancing the ring buffer and cumulative
/// delta.  Returns the updated state (convenience: the Tauri command handler
/// can return it directly to the frontend).
pub fn record_bar(state: &mut OrderFlowState, bar: BarDelta) {
    let d = bar.delta();
    state.current_bar_delta = d;
    state.cumulative_session_delta = state.cumulative_session_delta.saturating_add(d);

    if state.bar_deltas.len() >= BAR_RING_CAP {
        state.bar_deltas.remove(0);
    }
    state.bar_deltas.push(d);

    if state.bars.len() >= BAR_RING_CAP {
        state.bars.remove(0);
    }
    state.bars.push(bar);
}

/// Look up a preset by name.  Returns `Some(&[indicator_id])` or `None`.
pub fn lookup_preset(name: &str) -> Option<&'static [&'static str]> {
    WORKSPACE_PRESETS
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, ids)| *ids)
}

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn bar(bid: u64, ask: u64) -> BarDelta {
        BarDelta {
            open: 100.0,
            high: 101.0,
            low: 99.0,
            close: 100.5,
            bid_vol: bid,
            ask_vol: ask,
        }
    }

    fn bar_with_range(bid: u64, ask: u64, high: f64, low: f64) -> BarDelta {
        BarDelta {
            open: low,
            high,
            low,
            close: (high + low) / 2.0,
            bid_vol: bid,
            ask_vol: ask,
        }
    }

    // ── compute_cumulative_delta ──────────────────────────────────────────

    #[test]
    fn zero_bars_returns_zero() {
        assert_eq!(compute_cumulative_delta(&[]), 0);
    }

    #[test]
    fn single_bar_delta() {
        let bars = vec![bar(100, 150)];
        // ask 150 - bid 100 = +50
        assert_eq!(compute_cumulative_delta(&bars), 50);
    }

    #[test]
    fn multi_bar_sums_correctly() {
        let bars = vec![bar(100, 150), bar(200, 100), bar(50, 50)];
        // +50 + (-100) + 0 = -50
        assert_eq!(compute_cumulative_delta(&bars), -50);
    }

    #[test]
    fn overflow_saturates_at_i64_max() {
        // Both bars push ask to max; saturating_add must not wrap.
        let huge_ask = u64::MAX / 2;
        let bars = vec![bar(0, huge_ask), bar(0, huge_ask), bar(0, huge_ask)];
        let result = compute_cumulative_delta(&bars);
        assert_eq!(result, i64::MAX, "should saturate at i64::MAX");
    }

    #[test]
    fn overflow_saturates_at_i64_min() {
        let huge_bid = u64::MAX / 2;
        let bars = vec![bar(huge_bid, 0), bar(huge_bid, 0), bar(huge_bid, 0)];
        let result = compute_cumulative_delta(&bars);
        assert_eq!(result, i64::MIN, "should saturate at i64::MIN");
    }

    // ── detect_delta_divergence ───────────────────────────────────────────

    #[test]
    fn divergence_fewer_than_3_points_is_none() {
        assert_eq!(detect_delta_divergence(&[1.0, 2.0], &[10, 20]), None);
    }

    #[test]
    fn divergence_bearish_detected() {
        // Construct 5 bars: two swing-highs at indices 1 and 3.
        // Price: 100 → 105 → 102 → 107 → 104  (higher high: 105→107)
        // CVD:    0  →  50 →  40 →  30 →  20  (lower cvd:  50→30)
        let ph = [100.0, 105.0, 102.0, 107.0, 104.0];
        let cv = [0_i64, 50, 40, 30, 20];
        let sig = detect_delta_divergence(&ph, &cv);
        assert!(sig.is_some(), "expected bearish divergence");
        let s = sig.unwrap();
        assert!(s.bearish, "expected bearish=true");
        assert_eq!(s.price_high, 107.0);
        assert_eq!(s.prior_price_high, 105.0);
    }

    #[test]
    fn divergence_none_when_monotonic() {
        // Price and CVD both rising monotonically — no swing-highs with mismatch.
        let ph = [100.0, 101.0, 102.0, 103.0, 104.0];
        let cv = [0_i64, 10, 20, 30, 40];
        assert_eq!(detect_delta_divergence(&ph, &cv), None);
    }

    #[test]
    fn divergence_bullish_detected() {
        // Price lower high, CVD higher high → bullish divergence.
        // Price: 100 → 105 → 102 → 103 → 100  (lower high: 105→103)
        // CVD:    0  →  30 →  20 →  50 →  40  (higher cvd: 30→50)
        let ph = [100.0, 105.0, 102.0, 103.0, 100.0];
        let cv = [0_i64, 30, 20, 50, 40];
        let sig = detect_delta_divergence(&ph, &cv);
        assert!(sig.is_some(), "expected bullish divergence");
        let s = sig.unwrap();
        assert!(!s.bearish, "expected bearish=false");
    }

    // ── detect_absorption ─────────────────────────────────────────────────

    #[test]
    fn absorption_true_on_high_volume_narrow_range() {
        let cfg = OrderFlowConfig::default(); // multiplier=2.0, fraction=0.5
                                              // bar with total vol 1000, ema=400 (1000 ≥ 400*2), range=0.3, atr=1.0 (0.3/1.0=0.3 ≤ 0.5)
        let b = bar_with_range(400, 600, 100.3, 100.0);
        assert!(detect_absorption(&b, 400.0, 1.0, &cfg));
    }

    #[test]
    fn absorption_false_on_quiet_bar() {
        let cfg = OrderFlowConfig::default();
        // low vol bar: total 100, ema=400 (100 < 800), range irrelevant
        let b = bar_with_range(50, 50, 101.0, 100.0);
        assert!(!detect_absorption(&b, 400.0, 1.0, &cfg));
    }

    #[test]
    fn absorption_false_when_range_wide() {
        let cfg = OrderFlowConfig::default();
        // high vol but wide range: total=1000≥800, range=2.0, atr=2.0 → 1.0 > 0.5
        let b = bar_with_range(400, 600, 102.0, 100.0);
        assert!(!detect_absorption(&b, 400.0, 2.0, &cfg));
    }

    // ── tag_active_trade ──────────────────────────────────────────────────

    #[test]
    fn tag_append_valid_id() {
        let mut state = OrderFlowState::default();
        let tag = OrderFlowTag {
            id: "absorbed".to_string(),
            ts_ms: 0,
        };
        assert!(tag_active_trade(&mut state, tag).is_ok());
        assert_eq!(state.tags.len(), 1);
    }

    #[test]
    fn tag_rejects_unknown_id() {
        let mut state = OrderFlowState::default();
        let tag = OrderFlowTag {
            id: "lol_injection".to_string(),
            ts_ms: 0,
        };
        assert!(tag_active_trade(&mut state, tag).is_err());
    }

    #[test]
    fn tag_cap_enforced() {
        let mut state = OrderFlowState::default();
        for i in 0..TAG_CAP {
            let tag = OrderFlowTag {
                id: "absorbed".to_string(),
                ts_ms: i as u64,
            };
            assert!(tag_active_trade(&mut state, tag).is_ok());
        }
        // One more should fail.
        let tag = OrderFlowTag {
            id: "absorbed".to_string(),
            ts_ms: TAG_CAP as u64,
        };
        assert!(tag_active_trade(&mut state, tag).is_err());
    }

    // ── workspace presets ─────────────────────────────────────────────────

    #[test]
    fn preset_lookup_known_name() {
        let ids = lookup_preset("delta_focused");
        assert!(ids.is_some());
        assert!(ids.unwrap().contains(&"Cumulative Volume Delta"));
    }

    #[test]
    fn preset_lookup_unknown_name() {
        assert!(lookup_preset("not_a_real_preset").is_none());
    }

    // ── polling_hz clamping ───────────────────────────────────────────────

    #[test]
    fn polling_hz_clamped_to_max() {
        let cfg = OrderFlowConfig {
            polling_hz: 10,
            ..Default::default()
        }
        .validated();
        assert_eq!(cfg.polling_hz, MAX_POLLING_HZ);
    }

    #[test]
    fn polling_hz_zero_becomes_one() {
        let cfg = OrderFlowConfig {
            polling_hz: 0,
            ..Default::default()
        }
        .validated();
        assert_eq!(cfg.polling_hz, 1);
    }
}
