# Whiskey Intelligence Synthesis

**Audience:** WhiskeyMode system prompt + playbook + plausibility/confidence engine.
**Sources:** four parallel deep-research passes (statistical edge, order flow, psychology, decision theory) — 2026-05-12.
**Goal:** evolve Whiskey toward maximum model accuracy on setup grading.

This doc is the **operational distillation**. Each section ends with a concrete change to make. Detailed source citations live in the four sibling docs `INTELLIGENCE_{EDGE,ORDERFLOW,PSYCHOLOGY,DECISION}.md` (to be landed as follow-ups). 100% winrate is impossible; this brings the model as close as the math + market structure allow.

---

## 1. The hard floor: break-even math must be on screen

Every Whiskey trade proposal must surface the break-even win rate explicitly. Most retail traders take setups with negative expectancy after commissions and never know.

**Formula:** `W_breakeven = 1 / (1 + R_multiple)`

At 2R target on MNQ with $50 risk + $4.20 RT commission ($0.084R drag):

- Raw break-even = 33.3%
- Commission-adjusted break-even = 36.0%
- Minimum threshold for trade-worthy edge = **43%** (7-point cushion above commission-adjusted floor)

**Whiskey enforcement:** if estimated P(win | signals) ≤ 43% on a 2R setup, respond "Pass — insufficient edge above base rate" with the math shown.

---

## 2. Bayesian confidence formula (replaces vibes)

Whiskey's confidence score is computed as a sequential log-odds update, not a feel:

```
log_odds = logit(P_base)
         + log(LR_order_flow)
         + log(time_of_day_factor)
         + log(regime_match_factor)
         + log(psychology_readiness_factor)
         - log(1 / (1 - bayesian_uncertainty_penalty))

P(win | signals) = 1 / (1 + exp(-log_odds))
```

**Components:**

| Factor | Source | Typical range |
|---|---|---|
| `P_base` | Laplace-smoothed historical win rate `(wins + 0.5) / (N + 1.0)` | 0.3–0.7 |
| `LR_order_flow` | empirical `P(evidence|win)/P(evidence|loss)` from playbook | 0.3–3.5 |
| `time_of_day` | 0.85 open / 1.20 am trend / 0.75 lunch / 1.10 pm trend / 0.80 close noise / 1.15 close ramp | 0.5–1.2 |
| `regime_match` | ADX, VIX percentile, realized/implied vol ratio | 0.65–1.30 |
| `psychology_readiness` | 0.80 after 2+ consec losses; 0.85 after 4+ wins; 0.70 if daily P&L < -1.5R; 0.75 if sleep < 6h | 0.40–1.00 |
| `bayesian_uncertainty_penalty` | half-width of 95% credible interval on win rate (Beta posterior) | 0.08–0.50 |

**Grade mapping:**

| Confidence | Grade | Size multiplier |
|---|---|---|
| ≥ 0.80 | A+ | 1.00 |
| 0.65–0.79 | A | 0.75 |
| 0.50–0.64 | B | 0.50 |
| 0.43–0.49 | C | 0.25 |
| < 0.43 | Pass | 0.00 |

---

## 3. Variance-aware sizing (Kelly is too aggressive)

Pure Kelly blows up retail traders due to estimation error. **Half-Kelly** captures ~75% of Kelly's expected geometric growth at ~25% of the variance.

```
f*_continuous = μ / σ²              # Kelly fraction (continuous case)
f_half_kelly  = f*_continuous / 2.0
f_shrink      = f_half_kelly × sqrt(min(N, 30) / 30)   # small-N shrinkage
risk_fraction = min(f_shrink, account_max_risk = 0.02)
```

**Implication: 2% per-trade risk is the hard ceiling regardless of computed Kelly.** A setup with 8 instances and a 75% win rate gets a sample-size scalar of `sqrt(8/30) = 0.52`, halving its position even if the math says size up.

---

## 4. Sample-size tiers (N matters)

A 60% win rate with N=4 is statistically indistinguishable from 50%. The 95% credible interval on Beta(2.5, 2.5) is [0.118, 0.882] — width 0.76.

| Instances logged | Tier | Position size cap | Max grade |
|---|---|---|---|
| 0–19 | Hypothesis | 50% of base risk | B |
| 20–49 | Developing | 75% of base risk | A |
| 50–99 | Validated | 100% of base risk | A+ |
| 100+ | High-confidence | full conviction sizing | A+ |

Whiskey never promotes a setup to A+ until N ≥ 50.

---

## 5. The 10 actual edges Whiskey treats as base setups

Ranked by `edge × frequency × decay-risk`. Sources cite peer-reviewed work or replicated practitioner research; spurious "edges" (harmonic patterns, Gann angles, lunar cycles, Fibonacci ratios as standalone, RSI(2) face-value, Elliott Wave) are explicitly excluded.

1. **Close-ramp momentum** (Gao et al. 2018 JoF; Da, Dong, Linnainmaa JFE 2021) — by 15:00 ET, if session is trending >60% in one direction, target pullback to VWAP or 5min EMA. **Sharpe 1.16 on SPX futures**. Most academically robust intraday edge. **Hit rate 62–68%**.

2. **Absorption + delta divergence at structure** (Hasbrouck 1991; Tradingstats single-print backtest 72%) — composite signal at VAH/VAL/POC/prior-day-high/low. **Hit rate 60–65%**. Highest-frequency edge.

3. **Opening drive + IB extension** (Crabel 1990; SMB Capital opening-drive day-type) — first 5-min bar drives directionally with volume; enter pullback to IB boundary after 10:00 ET. **Hit rate 58–65% when drive is unambiguous**.

4. **Single-print rejection from prior-day value area** (Tradingstats 3,847-zone backtest) — price enters single-print TPO zone, fails to develop second TPO in 3 bars, volume below 20-bar average. **Hit rate 65–72%**.

5. **Naked POC magnet** (practitioner consensus ~80% revisit in 10 sessions) — use as TARGET signal, not entry, unless absorption confirms at POC. Within 0.5×ATR: 70% within-session touch.

6. **Gap fill on small gaps (<0.3%)** (MDPI stat-arb 2019) — small gaps fill 92%; medium (0.35%) fill 69%. **Edge +0.10–0.20R**.

7. **VWAP as regime classifier** (Zarattini & Aziz SSRN 2023) — NOT a mechanical entry. Price-vs-VWAP defines trend regime that other entries are filtered against.

8. **Volatility regime sizing** (Carver Systematic Trading; Engle GARCH) — size down in low-vol, up in high-vol. Risk management edge, not directional.

9. **Time-series momentum (multi-day)** (Quantpedia; Moskowitz et al. JFE 2012) — robust since 1880s data. Best as a regime filter on intraday setups.

10. **Limit-order entry vs market-order** (Glosten-Milgrom 1985 + empirical) — capturing half the spread on MNQ saves $0.50 RT = 10% of $50 risk. The simplest documented edge available to any retail trader.

---

## 6. Order-flow detection rules (formalized)

```python
# Delta divergence at swing
def delta_divergence_at_swing(price_series, delta_series, lookback=20):
    """N-bar new price extreme + CVD not confirming + delta_ratio drives confidence."""
    base_confidence = min(0.35 + delta_ratio * 0.25, 0.60)
    if 11.5 <= current_hour_ET <= 13.5:
        base_confidence *= 0.50   # lunch window penalty
    return (detected, base_confidence)

# Absorption at level
def absorption_at_level(bar_volume, bar_range, level_distance_ticks, avg_vol_20bar, atr):
    """2× avg volume + range < 0.5×ATR + within 2 ticks of level."""
    base_confidence = min(0.40 + (vol_ratio - 2.0) * 0.05 + (0.5 - range_ratio) * 0.20, 0.75)
    if at_level: base_confidence = min(base_confidence + 0.10, 0.75)
    return (detected, base_confidence)

# Naked POC magnet
def naked_poc_distance(current_price, prior_poc, atr):
    distance_in_atr = abs(current_price - prior_poc) / atr
    if distance_in_atr <= 0.5: return 0.70   # strong magnet
    if distance_in_atr <= 1.0: return 0.55
    if distance_in_atr <= 1.5: return 0.45
    if distance_in_atr <= 3.0: return 0.30
    return 0.15

# Opening type
def opening_drive_classifier(open, high, low, close, volume, avg_vol_5min):
    """drive_up | drive_down | responsive | neutral"""
```

**Composite scorer:** combine via weighted sum with time-of-day multiplier:
- Lunch window (11:30–13:30 ET): 0.50× multiplier
- ORB window (09:30–10:30 ET): 1.10× multiplier
- Close-ramp window (15:00–16:00 ET): 1.15× multiplier
- 60-second blackout after FOMC/NFP/CPI

---

## 7. Psychological readiness check (mandatory pre-trade)

Score 0–5 across five questions. Below 2.5 → trade blocked.

| Q | Pass (1.0) | Partial (0.5) | Fail (0) |
|---|---|---|---|
| Sleep + physical | >6h sleep, normal | 5–6h or mild stressor | <5h or significant |
| Emotional baseline (1–5) | 1–2, no stressor | 3 | 4–5 or active stressor |
| Session state | flat/positive, <2 consec losses, not at limit | 1 loss, within 50% of daily | **2+ consec losses or at daily limit → trade BLOCKED** |
| Setup validity | in playbook + name + stop + risk stated | marginal conditions | not in playbook OR can't state stop |
| Invalidation clarity | technical + time/price defined | one dimension only | can't state |

**Score → size haircut:**
- 4.5–5.0: full size
- 3.5–4.4: 75% size
- 2.5–3.4: 50% size, mandatory post-trade review
- <2.5: trade blocked

Q3=0 (tilt) blocks regardless of total score. **2 consecutive losses is the empirical 50% tilt-risk threshold** (Lo & Repin 2002 NBER; Steenbarger TraderFeed). Minimum 15-min physical displacement before re-engagement.

---

## 8. Ten Whiskey prompt rules (system-prompt additions)

1. **Pre-trade readiness gate.** Run the 5-question check. Q3=0 → "Two consecutive losses. Session paused. Process review or break — your choice."
2. **Pre-acceptance enforcement** (Douglas): trader states stop level, dollar risk, invalidation condition BEFORE size is discussed.
3. **Best-loss acknowledgment** (Steenbarger): on a loss with process score ≥4 → "Best-loss execution. Process was correct. Log it as a process win."
4. **Sunk-cost interrupt** (Kahneman): on "getting back to even" language → "Would you enter this position right now at the current price with the current size?"
5. **Recency-bias interrupt** (Tversky): after 3+ wins or 3+ losses → surface the 30-instance base rate for the setup.
6. **Setup-selection friction** (Csikszentmihalyi + Kahneman System 2): during setup review ask "What condition would tell you this is NOT the setup you think it is?"
7. **Low-N size cap** (Grimes): setups with <50 instances auto-capped at 50–75% of base risk.
8. **Daily hard-stop enforcement** (Market Wizards consensus): at session open, confirm hard stop. Non-revisitable mid-session. Proactive warning at 80% of limit.
9. **Tilt cooldown enforcement** (Lo/Repin + Steenbarger): after threshold trigger, 15-min minimum, no entry discussions, only process review or break.
10. **Outcome-grade correction** (Steenbarger): trader evaluates by P&L only → "What was your process score? P&L tells us what happened. Process tells us whether we're getting better."

---

## 9. The one decision that most increases accuracy

**The log-odds sequential Bayesian update.**

```
log_odds_posterior = logit(P_base) + Σ log(LR_i)
```

This single formula replaces subjective feel-based confidence with an auditable computed number. Each piece of evidence (order flow, time of day, regime, psychology, sample uncertainty) contributes a measurable log-likelihood ratio derived from the playbook. The gain over naive intuition is largest when multiple weak signals combine: three 1.3× LR factors compound to a 2.2× total lift, which intuition systematically underestimates.

---

## 10. Realistic expectancy ceiling

After honest deflation for survivorship + overfitting + commissions + slippage, a disciplined retail futures scalper running this framework should target:

- **Net expectancy per trade:** +0.15R to +0.25R
- **Trade count:** ~500/year (≈2/day, 250 sessions)
- **Annual R:** 75–125R
- **Dollar return on $50k account, $50 risk unit:** $3,750–$6,250/year

Achieving 2× that (+0.30R to +0.40R per trade consistently) places you in the top 3% of retail futures traders. Requires constant playbook calibration + rigorous logging. 100% winrate is mathematically impossible — variance is a structural fact, not a problem to solve.

---

## 11. Build queue for next Whiskey iteration

Priority-ordered changes to WhiskeyMode prompts + playbook + plausibility:

1. **Break-even win rate gate as a hard rule** in the prompt — every proposal surfaces `W_breakeven = 1/(1+R) + commission_drag`. Below the 7-point cushion: Pass with the math.
2. **Mandate regime classification before setup grading** — three inputs (price vs VWAP, ATR vs 20-day ATR, time-of-day bucket) before any setup scores above B.
3. **Bayesian log-odds confidence formula** wired into the plausibility check module (replace any current vibes-grading).
4. **Sample-size tiering** (Hypothesis/Developing/Validated/High-confidence) enforced at the position-sizing layer.
5. **Pre-trade readiness check** as a 5-question Tauri command output `psychology_readiness_factor`.
6. **Order-flow detection rules** (`delta_divergence_at_swing`, `absorption_at_level`, `naked_poc_distance`, `opening_drive_classifier`, `composite_orderflow_score`) implemented as pure Rust functions in `src/openhuman/modes/order_flow.rs`.
7. **Time-of-day + post-news 60s blackout** as a filter on all order-flow-derived confidence boosts.
8. **DSR-awareness disclosure** in playbook header — any setup imported from external backtesting must state N parameter variants tested + out-of-sample period.
9. **Commission-drag display** in every proposal: "Commission drag = $X.XX / $Y risk = ZR. Required gross expectancy: >0.184R to net +0.1R."
10. **Outcome-grade → process-grade reframing** in every post-trade summary. P&L second, process score first.

---

## Sources (full citations in sibling docs)

- Bailey, Borwein, López de Prado, Zhu — "Pseudo-Mathematics and Financial Charlatanism" (AMS 2014); "The Deflated Sharpe Ratio" (JPM 2014)
- Carver, R. — Systematic Trading (Harriman House 2015)
- Cover & Thomas — Elements of Information Theory (2nd ed. 2006)
- Crabel, T. — Day Trading with Short-Term Price Patterns and Opening Range Breakout (1990)
- Da, Dong & Linnainmaa — "Hedging demand and market intraday momentum" (JFE 2021)
- Douglas, M. — Trading in the Zone (2000)
- Gao, Han, Li, Zhou — "Market Intraday Momentum" (JoF 2018)
- Grimes, A. — The Art and Science of Technical Analysis (2012)
- Hasbrouck, J. — Empirical Market Microstructure (OUP 2007); JoF 1991 "Measuring the Information Content of Stock Trades"
- Kahneman, D. — Thinking, Fast and Slow (2011); Kahneman & Tversky "Prospect Theory" (Econometrica 1979)
- Lee & Ready — JoF 1991 (tick test)
- Lo, A. — Adaptive Markets (Princeton 2017); Lo & Repin J. Cognitive Neuroscience 2002 (trader physiology)
- López de Prado, M. — Advances in Financial Machine Learning (Wiley 2018)
- Steenbarger, B. — Trading Psychology 2.0 (Wiley 2015); TraderFeed blog
- Sutton & Barto — Reinforcement Learning: An Introduction (2nd ed. 2018)
- Thaler & Johnson — Management Science 1990 (house-money effect)
- Tradingstats.net — 3,847 single-print zone backtest
- Van Tharp — Trade Your Way to Financial Freedom (1998)
