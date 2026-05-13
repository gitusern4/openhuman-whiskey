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

// ─── intelligence-layer additions (§6 + §7 of INTELLIGENCE_SYNTHESIS.md) ────
//
// These functions implement the formalized order-flow detection rules from §6.
// All are pure functions with zero side-effects; they consume slices / value
// types and return Option/named-result types. Existing public APIs are NOT
// modified.

/// A single bar of OHLCV data (simplified from `BarDelta` — no bid/ask split
/// required for absorption and opening-drive detection).
#[derive(Debug, Clone, PartialEq)]
pub struct BarSample {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

impl BarSample {
    pub fn range(&self) -> f64 {
        self.high - self.low
    }
}

/// Delta-divergence signal produced by [`delta_divergence_at_swing`].
#[derive(Debug, Clone, PartialEq)]
pub struct DivergenceSignalV2 {
    /// `true` = bearish (price higher high, delta lower high).
    pub bearish: bool,
    /// Confidence 0.0–1.0 from the §6 formula.
    pub confidence: f64,
    /// Ratio of delta at recent swing to delta at prior swing.
    pub delta_ratio: f64,
}

/// Absorption signal produced by [`absorption_at_level`].
#[derive(Debug, Clone, PartialEq)]
pub struct AbsorptionSignal {
    /// Confidence 0.0–1.0 capped at 0.75.
    pub confidence: f64,
    /// Volume ratio (bar vol / avg_vol_20).
    pub vol_ratio: f64,
    /// Range ratio (bar range / atr).
    pub range_ratio: f64,
}

/// Output of [`naked_poc_magnet`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NakedPocProbability {
    /// Distance in ATR units between current price and the prior POC.
    pub distance_atr: f64,
    /// P(touch within session) per §6 table.
    pub probability: f64,
}

/// Opening type classification produced by [`opening_drive_classifier`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpeningType {
    DriveUp,
    DriveDown,
    Responsive,
    Neutral,
}

/// Composite order-flow score combining all §6 detection signals.
#[derive(Debug, Clone, PartialEq)]
pub struct CompositeScore {
    /// Raw weighted sum before time-of-day adjustment (0.0–1.0).
    pub raw: f64,
    /// Final score after time-of-day multiplier (0.0–1.0).
    pub adjusted: f64,
    /// True if a post-news blackout is in effect.
    pub news_blackout_active: bool,
}

// ── time-of-day constants (§6 + §7) ──────────────────────────────────────────

/// Time-of-day score multiplier table per §6.
/// Each entry: (hour_et_start, hour_et_end_exclusive, multiplier).
/// Gaps between windows use 1.0 (neutral).
pub const TIME_OF_DAY_MULTIPLIER: &[(f64, f64, f64)] = &[
    (9.5, 10.5, 1.10),  // ORB window (09:30–10:30 ET)
    (11.5, 13.5, 0.50), // Lunch window (11:30–13:30 ET)
    (15.0, 16.0, 1.15), // Close-ramp window (15:00–16:00 ET)
];

/// Return the time-of-day multiplier for `hour_et` (e.g. 9.5 = 09:30 ET).
/// Falls back to 1.0 for hours not covered by any window.
pub fn time_of_day_factor(hour_et: f64) -> f64 {
    for &(start, end, mult) in TIME_OF_DAY_MULTIPLIER {
        if hour_et >= start && hour_et < end {
            return mult;
        }
    }
    1.0
}

/// Return `true` if a 60-second post-news blackout is still active.
///
/// `is_high_impact_news` should be `true` for FOMC/NFP/CPI events.
/// Timestamps are Unix seconds.
pub fn news_blackout_until(last_news_utc: i64, now_utc: i64, is_high_impact_news: bool) -> bool {
    if !is_high_impact_news {
        return false;
    }
    (now_utc - last_news_utc).abs() < 60
}

// ── §6 detection functions ────────────────────────────────────────────────────

/// Detect delta divergence at a price swing.
///
/// Scans `prices` and `deltas` over the trailing `lookback` bars,
/// finds the most recent and prior swing-high or swing-low, and
/// compares the directional delta reading at each.
///
/// Returns `None` when there are fewer than 3 bars in the lookback or
/// no pair of swing-highs/lows is found.
///
/// Confidence formula (§6):
/// ```text
/// delta_ratio = |delta_at_recent| / (|delta_at_prior| + 1)
/// base_confidence = min(0.35 + delta_ratio * 0.25, 0.60)
/// lunch_penalty: ×0.50 when hour_et ∈ [11.5, 13.5)
/// ```
pub fn delta_divergence_at_swing(
    prices: &[f64],
    deltas: &[i64],
    lookback: usize,
) -> Option<DivergenceSignalV2> {
    let n = prices.len().min(deltas.len()).min(lookback);
    if n < 3 {
        return None;
    }
    let prices = &prices[prices.len() - n..];
    let deltas = &deltas[deltas.len() - n..];

    // Collect swing-highs (local maxima in prices).
    let mut swing_highs: Vec<usize> = Vec::new();
    for i in (1..n - 1).rev() {
        if prices[i] > prices[i - 1] && prices[i] > prices[i + 1] {
            swing_highs.push(i);
            if swing_highs.len() == 2 {
                break;
            }
        }
    }

    if swing_highs.len() < 2 {
        return None;
    }

    let recent = swing_highs[0];
    let prior = swing_highs[1];
    let price_recent = prices[recent];
    let price_prior = prices[prior];
    let delta_recent = deltas[recent];
    let delta_prior = deltas[prior];

    let bearish = price_recent > price_prior && delta_recent < delta_prior;
    let bullish = price_recent < price_prior && delta_recent > delta_prior;
    if !bearish && !bullish {
        return None;
    }

    // delta_ratio: how strongly delta diverges.
    let delta_ratio = (delta_recent.abs() as f64) / (delta_prior.abs() as f64 + 1.0);
    let base_confidence = (0.35 + delta_ratio * 0.25).min(0.60);

    Some(DivergenceSignalV2 {
        bearish,
        confidence: base_confidence,
        delta_ratio,
    })
}

/// Detect absorption at a price level.
///
/// Conditions (§6):
/// - `bar.volume >= avg_vol_20 × 2.0` (high volume)
/// - `bar.range() / atr_ticks <= 0.5` (narrow range)
/// - Within `near_level_ticks` of a structural level
///
/// Confidence formula (§6):
/// ```text
/// vol_ratio = bar.volume / avg_vol_20
/// range_ratio = bar.range() / atr_ticks
/// base = min(0.40 + (vol_ratio - 2.0)*0.05 + (0.5 - range_ratio)*0.20, 0.75)
/// if at_level: base = min(base + 0.10, 0.75)
/// ```
pub fn absorption_at_level(
    bar: &BarSample,
    near_level_ticks: i32,
    avg_vol_20: f64,
    atr_ticks: f64,
) -> Option<AbsorptionSignal> {
    if avg_vol_20 <= 0.0 || atr_ticks <= 0.0 {
        return None;
    }
    let vol_ratio = bar.volume / avg_vol_20;
    let range_ratio = if atr_ticks > 0.0 {
        bar.range() / atr_ticks
    } else {
        1.0
    };

    let volume_ok = vol_ratio >= 2.0;
    let range_ok = range_ratio <= 0.5;
    if !volume_ok || !range_ok {
        return None;
    }

    let at_level = near_level_ticks.abs() <= 2;
    let mut base = (0.40 + (vol_ratio - 2.0) * 0.05 + (0.5 - range_ratio) * 0.20).min(0.75);
    if at_level {
        base = (base + 0.10).min(0.75);
    }

    Some(AbsorptionSignal {
        confidence: base,
        vol_ratio,
        range_ratio,
    })
}

/// Estimate P(naked POC touch within session) from distance in ATR units.
///
/// Per §6 table (derived from ~80% revisit in 10-session practitioner data):
/// | distance_atr | probability |
/// |-------------|-------------|
/// | ≤ 0.5       | 0.70        |
/// | ≤ 1.0       | 0.55        |
/// | ≤ 1.5       | 0.45        |
/// | ≤ 3.0       | 0.30        |
/// | > 3.0       | 0.15        |
pub fn naked_poc_magnet(current_price: f64, prior_poc: f64, atr: f64) -> NakedPocProbability {
    let distance_atr = if atr > 0.0 {
        (current_price - prior_poc).abs() / atr
    } else {
        f64::MAX
    };
    let probability = if distance_atr <= 0.5 {
        0.70
    } else if distance_atr <= 1.0 {
        0.55
    } else if distance_atr <= 1.5 {
        0.45
    } else if distance_atr <= 3.0 {
        0.30
    } else {
        0.15
    };
    NakedPocProbability {
        distance_atr,
        probability,
    }
}

/// Classify the opening drive type from the first 5-minute bar.
///
/// A drive is "unambiguous" (§5 #3, Crabel 1990) when:
/// - directional close (close > midpoint for DriveUp, < midpoint for DriveDown)
/// - volume >= avg_5min_vol × 1.3 (above-average participation)
///
/// Falls back to Responsive or Neutral when conditions aren't met.
pub fn opening_drive_classifier(
    open: f64,
    first_5min: &BarSample,
    avg_5min_vol: f64,
) -> OpeningType {
    let midpoint = (first_5min.high + first_5min.low) / 2.0;
    let vol_ok = avg_5min_vol > 0.0 && first_5min.volume >= avg_5min_vol * 1.3;

    if first_5min.close > midpoint && vol_ok {
        // Bullish drive: closed in upper half on above-average volume.
        return OpeningType::DriveUp;
    }
    if first_5min.close < midpoint && vol_ok {
        return OpeningType::DriveDown;
    }

    // Responsive: price opened above/below prior close and reversed.
    // Simplified: open vs. bar midpoint divergence signals responsive action.
    if open > midpoint && first_5min.close < midpoint {
        return OpeningType::Responsive;
    }
    if open < midpoint && first_5min.close > midpoint {
        return OpeningType::Responsive;
    }

    OpeningType::Neutral
}

/// Combine all §6 detection signals into a weighted composite score.
///
/// Weights:
/// - Delta divergence: 0.40
/// - Absorption: 0.35
/// - Naked POC magnet: 0.25 (use probability directly)
///
/// Final score is multiplied by `time_of_day_factor(hour_et)`.
/// If `news_blackout_active` is true, score is forced to 0.
///
/// Returns a `CompositeScore` with raw and adjusted values.
pub fn composite_order_flow_score(
    divergence: Option<&DivergenceSignalV2>,
    absorption: Option<&AbsorptionSignal>,
    poc_prob: Option<&NakedPocProbability>,
    hour_et: f64,
    is_high_impact_news: bool,
    last_news_utc: i64,
    now_utc: i64,
) -> CompositeScore {
    let blackout = news_blackout_until(last_news_utc, now_utc, is_high_impact_news);
    if blackout {
        return CompositeScore {
            raw: 0.0,
            adjusted: 0.0,
            news_blackout_active: true,
        };
    }

    let div_score = divergence.map(|d| d.confidence).unwrap_or(0.0);
    let abs_score = absorption.map(|a| a.confidence).unwrap_or(0.0);
    let poc_score = poc_prob.map(|p| p.probability).unwrap_or(0.0);

    // Weighted sum — only include component if signal is present.
    let (total_weight, weighted_sum) = {
        let mut w = 0.0_f64;
        let mut s = 0.0_f64;
        if divergence.is_some() {
            w += 0.40;
            s += div_score * 0.40;
        }
        if absorption.is_some() {
            w += 0.35;
            s += abs_score * 0.35;
        }
        if poc_prob.is_some() {
            w += 0.25;
            s += poc_score * 0.25;
        }
        (w, s)
    };

    let raw = if total_weight > 0.0 {
        weighted_sum / total_weight
    } else {
        0.0
    };

    let tod = time_of_day_factor(hour_et);
    // Cap at 1.0 after time adjustment.
    let adjusted = (raw * tod).min(1.0);

    CompositeScore {
        raw,
        adjusted,
        news_blackout_active: false,
    }
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

    // ── intelligence-layer additions ──────────────────────────────────────

    fn bar_sample(open: f64, high: f64, low: f64, close: f64, volume: f64) -> BarSample {
        BarSample {
            open,
            high,
            low,
            close,
            volume,
        }
    }

    // ── delta_divergence_at_swing ─────────────────────────────────────────

    #[test]
    fn delta_div_at_swing_too_few_bars_is_none() {
        let prices = [100.0, 101.0];
        let deltas = [10_i64, 20];
        assert!(delta_divergence_at_swing(&prices, &deltas, 20).is_none());
    }

    #[test]
    fn delta_div_at_swing_bearish_detected() {
        // Price: higher high; delta: lower high → bearish divergence.
        let prices = [100.0, 105.0, 102.0, 107.0, 104.0];
        let deltas = [0_i64, 500, 400, 300, 200];
        let sig = delta_divergence_at_swing(&prices, &deltas, 20);
        assert!(sig.is_some(), "expected divergence signal");
        let s = sig.unwrap();
        assert!(s.bearish);
        assert!(s.confidence > 0.0 && s.confidence <= 0.60);
    }

    #[test]
    fn delta_div_at_swing_no_divergence_is_none() {
        // Monotonic — no swing-high pair with divergence.
        let prices = [100.0, 101.0, 102.0, 103.0, 104.0];
        let deltas = [0_i64, 10, 20, 30, 40];
        assert!(delta_divergence_at_swing(&prices, &deltas, 20).is_none());
    }

    // ── absorption_at_level ───────────────────────────────────────────────

    #[test]
    fn absorption_detected_at_level() {
        // 2× avg vol, narrow range, within 2 ticks of level.
        let b = bar_sample(100.0, 100.3, 100.0, 100.15, 2000.0);
        let sig = absorption_at_level(&b, 1, 800.0, 2.0);
        assert!(sig.is_some(), "should detect absorption");
        let s = sig.unwrap();
        assert!(s.confidence <= 0.75);
    }

    #[test]
    fn absorption_not_detected_low_volume() {
        // Only 1× avg vol — below threshold.
        let b = bar_sample(100.0, 100.3, 100.0, 100.15, 800.0);
        let sig = absorption_at_level(&b, 1, 800.0, 2.0);
        assert!(sig.is_none());
    }

    #[test]
    fn absorption_not_detected_wide_range() {
        // High vol but wide range (> 0.5 × atr).
        let b = bar_sample(100.0, 102.0, 100.0, 101.0, 2000.0);
        let sig = absorption_at_level(&b, 1, 800.0, 2.0);
        // range = 2.0, atr = 2.0 → range_ratio = 1.0 > 0.5 → no signal
        assert!(sig.is_none());
    }

    // ── naked_poc_magnet ──────────────────────────────────────────────────

    #[test]
    fn naked_poc_close_distance_high_probability() {
        // Within 0.5 ATR → 0.70
        let r = naked_poc_magnet(100.0, 100.3, 1.0);
        assert!((r.probability - 0.70).abs() < 1e-10);
        assert!(r.distance_atr <= 0.5);
    }

    #[test]
    fn naked_poc_medium_distance_moderate_probability() {
        // 1.2 ATR → 0.45
        let r = naked_poc_magnet(100.0, 101.2, 1.0);
        assert!((r.probability - 0.45).abs() < 1e-10);
    }

    #[test]
    fn naked_poc_far_distance_low_probability() {
        // 4 ATR → 0.15
        let r = naked_poc_magnet(100.0, 104.0, 1.0);
        assert!((r.probability - 0.15).abs() < 1e-10);
    }

    // ── opening_drive_classifier ──────────────────────────────────────────

    #[test]
    fn opening_drive_up_on_bullish_close_high_volume() {
        // Close > midpoint, volume > 1.3× avg.
        let bar = bar_sample(100.0, 102.0, 99.0, 101.5, 1400.0);
        let ot = opening_drive_classifier(100.0, &bar, 1000.0);
        assert_eq!(ot, OpeningType::DriveUp);
    }

    #[test]
    fn opening_drive_down_on_bearish_close_high_volume() {
        let bar = bar_sample(100.0, 102.0, 99.0, 99.5, 1400.0);
        let ot = opening_drive_classifier(100.0, &bar, 1000.0);
        assert_eq!(ot, OpeningType::DriveDown);
    }

    #[test]
    fn opening_drive_neutral_on_low_volume() {
        // Even if close is bullish, low volume → not an unambiguous drive.
        let bar = bar_sample(100.0, 102.0, 99.0, 101.5, 900.0);
        let ot = opening_drive_classifier(100.0, &bar, 1000.0);
        // Low volume but no open/close reversal → Neutral.
        assert_eq!(ot, OpeningType::Neutral);
    }

    // ── time_of_day_factor ────────────────────────────────────────────────

    #[test]
    fn time_of_day_orb_window_returns_1pt10() {
        assert!((time_of_day_factor(9.8) - 1.10).abs() < 1e-10);
    }

    #[test]
    fn time_of_day_lunch_window_returns_0pt50() {
        assert!((time_of_day_factor(12.0) - 0.50).abs() < 1e-10);
    }

    #[test]
    fn time_of_day_close_ramp_returns_1pt15() {
        assert!((time_of_day_factor(15.5) - 1.15).abs() < 1e-10);
    }

    #[test]
    fn time_of_day_neutral_window_returns_1pt0() {
        assert!((time_of_day_factor(14.0) - 1.0).abs() < 1e-10);
    }

    // ── news_blackout_until ───────────────────────────────────────────────

    #[test]
    fn news_blackout_within_60s_active() {
        assert!(news_blackout_until(1000, 1059, true));
    }

    #[test]
    fn news_blackout_after_60s_not_active() {
        assert!(!news_blackout_until(1000, 1060, true));
    }

    #[test]
    fn news_blackout_non_high_impact_always_false() {
        assert!(!news_blackout_until(1000, 1010, false));
    }

    // ── composite_order_flow_score ────────────────────────────────────────

    #[test]
    fn composite_score_blackout_forces_zero() {
        let div = DivergenceSignalV2 {
            bearish: true,
            confidence: 0.55,
            delta_ratio: 0.8,
        };
        let result = composite_order_flow_score(
            Some(&div),
            None,
            None,
            10.0,
            true,
            1000,
            1010, // within 60s blackout
        );
        assert_eq!(result.raw, 0.0);
        assert_eq!(result.adjusted, 0.0);
        assert!(result.news_blackout_active);
    }

    #[test]
    fn composite_score_lunch_penalty_reduces_adjusted() {
        let div = DivergenceSignalV2 {
            bearish: true,
            confidence: 0.55,
            delta_ratio: 0.8,
        };
        let outside_lunch = composite_order_flow_score(
            Some(&div),
            None,
            None,
            10.0, // ORB window 1.10×
            false,
            0,
            0,
        );
        let at_lunch = composite_order_flow_score(
            Some(&div),
            None,
            None,
            12.0, // lunch 0.50×
            false,
            0,
            0,
        );
        assert!(
            at_lunch.adjusted < outside_lunch.adjusted,
            "lunch should produce lower adjusted score"
        );
    }

    #[test]
    fn composite_score_all_signals_present() {
        let div = DivergenceSignalV2 {
            bearish: true,
            confidence: 0.55,
            delta_ratio: 1.0,
        };
        let abs = AbsorptionSignal {
            confidence: 0.65,
            vol_ratio: 3.0,
            range_ratio: 0.3,
        };
        let poc = NakedPocProbability {
            distance_atr: 0.4,
            probability: 0.70,
        };
        let result = composite_order_flow_score(
            Some(&div),
            Some(&abs),
            Some(&poc),
            14.0, // neutral window → 1.0×
            false,
            0,
            0,
        );
        assert!(result.raw > 0.0);
        // neutral window so adjusted ≈ raw
        assert!((result.adjusted - result.raw).abs() < 1e-10);
        assert!(!result.news_blackout_active);
    }
}
