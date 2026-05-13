# How to use Whiskey

The user-facing manual for the Whiskey-on-OpenHuman fork.
Last updated: 2026-05-13.

---

## What this is

A desktop AI trading mentor that attaches to TradingView Desktop, reads your chart state, scores setups against your playbook with calibrated probability math, runs psychology + risk checks before letting you trade, and (optionally) executes bracket orders through TopStepX with hard safety gates.

Whiskey is a mentor, not a robot. Every trade goes through a confirm step. Every safety primitive is code-enforced, not advisory.

---

## One-time setup

### 1. Install + launch

The app installs via the MSI built from `cargo tauri build --target aarch64-pc-windows-msvc`. First launch opens the **onboarding wizard** — a 4-step flow that walks you through:

1. **Pick your mode** — Default (stock OpenHuman) or Whiskey (trading mentor). Pick Whiskey.
2. **TradingView bridge** — see step 2 below.
3. **Customize summon hotkey** — default `Ctrl+Shift+Space` works fine; change if it clashes with anything.
4. **Done** — takes you to TK's Mods.

You can re-open the wizard from Settings → Onboarding if you skip it.

### 2. Connect TradingView Desktop

Whiskey reads your chart via Chrome DevTools Protocol. TV doesn't expose this by default — you have to launch it with one extra flag.

**Easiest path:** open Whiskey → Settings → **TK's Mods** → TradingView Bridge → click **"Launch TV"**. Whiskey searches the common install paths (`%LOCALAPPDATA%\Programs\TradingView\TradingView.exe` and three siblings) and spawns TV with `--remote-debugging-port=9222` for you.

**Manual path:**

1. Quit TradingView Desktop.
2. Right-click your TV shortcut → Properties → in the Target field, append a space then `--remote-debugging-port=9222`. Save.
3. Relaunch TV. Open a chart.

Once TV is up, back in TK's Mods → TradingView Bridge:
- Click **Probe** — should show "reachable" with the TV page URL listed.
- Click **Attach** — should show "attached" with a green status dot.
- Toggle **Auto-attach** — Whiskey reconnects automatically if TV reloads or the chart switches.

### 3. Configure your covenant

The covenant (`<openhuman_dir>/covenant.toml`) is your commitment device. Whiskey reads it once at session start; you cannot change it mid-session.

Default location: `%APPDATA%/openhuman/covenant.toml` on Windows, `~/.openhuman/covenant.toml` on macOS.

Minimum required fields (Whiskey rejects startup if any are missing or unsafe):

```toml
[covenant]
version = "1.0"
signed_at = "2026-05-13T00:00:00Z"

[limits]
daily_max_loss_usd = 500.0
max_position_size_contracts = 2
max_consecutive_losses = 3
no_trading_after = "20:00"           # local time
no_trading_before = "06:30"

[instruments]
whitelist = ["MESH5", "MNQH5", "MES", "MNQ"]

[confirmation]
require_per_trade_confirm = true     # forced true; setting false rejected at startup
confirm_countdown_seconds = 3
single_leg_market_orders_allowed = false

[cooldown]
base_cooldown_seconds = 3
per_loss_additional_seconds = 1
walk_away_trigger_loss_fraction = 0.75

[session]
reset_kills_at_session_start = true
```

Edit it the night before, not in the heat of a session. Whiskey hashes the file at session start and writes the hash to the audit log so any mid-session tampering is detectable post-hoc.

### 4. (Optional) Connect TopStepX for execution

If you want Whiskey to actually submit orders (not just suggest them):

1. Get a TopStepX API key from your TopStep account ($29/month add-on; 50% off for active TopStep traders).
2. Open Settings → TK's Mods → Execution → click **Authenticate TopStepX**.
3. Paste the API key. Whiskey stores it in memory only (never written to disk).

Without authentication, all execution commands return "broker not connected" — you can still use Whiskey for setup grading and journaling.

---

## Daily workflow

### Pre-session (5 minutes)

1. Launch Whiskey, then launch TV with the debug flag (or use the auto-launch button).
2. Open TK's Mods. Verify TV bridge shows **attached** (green dot).
3. **Pre-trade readiness check** — Whiskey asks 5 questions (sleep, emotional state, session state, setup validity, invalidation clarity). Score under 2.5 = trade blocked. Under 3.5 = position size halved.
4. Glance at the Walk-away Lockout card. Are you locked out from yesterday? Did you trip a daily limit? Whiskey will tell you. If the banner is red, do not trade — the lockout was set by you in a calmer state.

### During a setup

You spot something on TV. Either:

**A. Ask Whiskey directly:** "Should I take this NQ long at 21350 — A+ catalog match?" Whiskey reads your chart state via CDP, scores the setup using the Bayesian confidence formula, runs the plausibility check, and responds with either:
- A proposal (instrument, qty, entry, stop, target, R-estimate, confidence %, sample-size tier) — you click Confirm in the dialog that pops up (3-second countdown + 1s per consecutive loss in the session)
- A pass with reasoning ("expected win rate 38% on 2R target, breakeven 36% + commission, edge too thin for size")
- A block ("you've had 2 consecutive losses, session paused")

**B. Use TK's Mods directly:**
- **Position size calculator** — type entry / stop / risk-$ → contracts (rounded down)
- **SL/TP overlay** — draws horizontal lines on your TV chart for entry / stop / target so you can see them even on prop firms that hide order lines
- **Symbol favorites** — one-click switches between your common contracts
- **Pre-trade checklist** — runs the 5-item list (catalog-match, stop-defined, size-calc, fits-budget, not-revenge)
- **Order flow card** — pull live delta + drawn shapes + alert count from TV; tag the active position with absorbed / delta-div / single-print-reject / VA-reject / responsive-buyer / responsive-seller

### Confirming an order (if TopStepX is wired)

When Whiskey proposes a bracket order:

1. The confirm dialog displays for at least 3 seconds — you cannot click Confirm during the countdown.
2. Re-check the proposal one more time. Especially direction and stop placement.
3. Click Confirm. Whiskey re-validates covenant + kill switch + plausibility (in case anything changed in the 3-second window), then submits the bracket via TopStepX with `isAutomated: true` (CME rule).
4. The trade is journaled. Both the proposal and the send are audit-logged with full schema (timestamp_utc, actor, action, instrument, qty, price, stop, target, R_estimate, confidence_pct, playbook_match_id, idempotency_key, broker_response, session_loss_count, daily_pnl_at_action, kill_engaged, notes).

### After a trade closes

Whiskey prompts the **process-grade** review BEFORE showing P&L:

- Setup quality (1–5)
- Entry timing (1–5)
- Size discipline (1–5)
- Stop discipline (1–5)
- Exit discipline (1–5)
- Journal completeness (1–5)

Average score:
- 4.5+: elite process. Position size eligible to scale on next conviction trade.
- 3.5–4.4: solid. Standard risk unit.
- 2.5–3.4: developing. 75% risk unit cap.
- <2.5: process failure. No new trades until you write a review.

Whiskey then shows P&L. If it was a loss with process ≥ 4: **"Best-loss execution. Process was correct. Edge doesn't guarantee this instance. Log it as a process win."**

### When things go wrong

**Kill switch.** Red button, fixed top-right of TK's Mods. One click:
1. Cancels all open orders
2. Flattens all positions (market orders)
3. Revokes the TopStepX session token
4. Sets `kill_engaged: true` persistently (survives app restart)
5. Disables order entry
6. Writes a `Kill` audit entry

To reset: wait 30 minutes minimum, then type the reset phrase exactly: **"I am ready to trade"**.

**Walk-away lockout.** Triggered automatically by:
- Daily loss limit hit (covenant `daily_max_loss_usd`)
- Max consecutive losses hit (covenant `max_consecutive_losses`)
- Single-trade loss > 0.75R of daily budget (5-minute forced break)
- Manual "Trip lockout now" button

Differs from kill switch: lockout is a discipline pause, doesn't touch broker state. Reset requires arming (5-minute server-side cooldown — you cannot bypass via DevTools console even if you try).

**Risk display.** Toggle **"Hide risk %"** in TK's Mods. When on, every Whiskey message has $-amounts replaced with "risk unit" and %-figures replaced with "small position" / "a portion". R-multiples preserved. Whiskey caches the flag for hot-path performance; the cache invalidates on save.

---

## What Whiskey does NOT do

- **No autonomous trade execution.** Every order requires the user-confirm click. This is a hard invariant in the covenant (`require_per_trade_confirm = true` cannot be set to false).
- **No financial advice.** Whiskey is a mentor + calculator + journal. The user decides.
- **No cloud uploads.** All audit logs, covenant config, kill-switch state, and TOML persistence files live locally under `<openhuman_dir>`. Only outbound HTTP is to TopStepX (when authenticated) and TradingView (your existing TV connection, unchanged).
- **No backtesting.** Whiskey scores live setups against your logged playbook; it doesn't simulate historical performance.
- **No magic.** The expectancy ceiling for a disciplined retail futures scalper running this framework is +0.15R to +0.25R per trade net. 75–125R annual on 500 trades. Achieving 2× that puts you in the top 3% of retail. 100% winrate is mathematically impossible — variance is structural.

---

## TK's Mods sections, in display order

1. **AI Mode** — pick Default vs Whiskey; configure mascot summon hotkey
2. **TradingView Bridge** — probe/attach/detach; auto-attach toggle; status pill; chart-state readout
3. **Order Flow** — workspace presets, live delta tracking, 6 tag chips, detection alerts
4. **TradingView Overlay** — inject a Whiskey panel directly into TV's window (drag handle + minimize + survives TV reloads via MutationObserver + outbox nonce against TV-page script forgery)
5. **SL/TP Overlay** — draw stop/target horizontal lines on the chart
6. **Position Size Calculator** — 9 baked-in futures specs (MNQ/MES/NQ/ES/MYM/M2K/CL/GC/STOCK)
7. **Pre-trade Checklist** — editable 5-item list; Confirm button gates on all-checked
8. **Symbol Favorites** — quick-switch list, cap of 20
9. **Walk-away Lockout** — server-side 3-phase arm-then-reset gate
10. **Theme** — Default or ZETH (black + neon green)
11. **Risk-hide toggle** — $/% redaction in Whiskey output (R-multiples preserved)

Plus (post-intelligence-impl):
12. **Pre-trade readiness check** — 5-question rubric scored 0–5 with hard-block path
13. **Confidence display** — Bayesian P(win) + grade + sample-size tier on every proposal

---

## Where to find things

| What | Where |
|---|---|
| Covenant config | `%APPDATA%/openhuman/covenant.toml` |
| Audit log | `%APPDATA%/openhuman/audit/audit-YYYY-MM-DD.jsonl` |
| Kill switch state | `%APPDATA%/openhuman/kill_switch.toml` |
| TK's Mods config | `%APPDATA%/openhuman/tks_mods.toml` |
| Lockout state | `%APPDATA%/openhuman/tks_lockout.toml` |
| Onboarding state | `%APPDATA%/openhuman/onboarding.toml` |
| CDP auto-attach state | `%APPDATA%/openhuman/cdp_auto_attach.toml` |
| Active mode | `%APPDATA%/openhuman/active_mode.toml` |
| Logs | `%APPDATA%/openhuman/logs/` |

All persistence files use atomic writes (write-to-tmp + rename) so a crash mid-write can't truncate them.

---

## Troubleshooting

**TV bridge shows "unreachable"** — TV isn't running with the debug flag. Either click "Launch TV" or relaunch manually with the flag. Then click Probe.

**Auto-attach keeps retrying but never connects** — the supervisor's backoff caps at 30 seconds; if it's stuck, your TV process probably died. Check that TV Desktop is actually open and showing a chart.

**Whiskey says "broker not connected"** — TopStepX session expired or never authenticated. Re-paste the API key.

**Kill switch reset rejected** — either the 30-minute cooldown isn't up (Whiskey tells you seconds remaining) or you typed the reset phrase wrong (must be exactly: `I am ready to trade`).

**Confidence score on a setup feels too low** — Whiskey is using sample-size tier as a multiplier. A setup with < 20 logged instances caps at B grade and 50% position size, regardless of how the math evaluates. Log more trades.

**Whiskey refuses to discuss a trade** — Q3 of the readiness check (session state) returned 0. You've either hit 2 consecutive losses or your daily loss limit. The session is paused. Either review the losing trades or take a break.

**Order didn't submit even after I clicked Confirm** — the kill switch may have engaged between proposal and confirm (e.g. another trade tripped the daily limit). Re-check status. The audit log has the full trail.

---

## Philosophy

Whiskey's accuracy ceiling is bounded by:
1. Variance (no model can eliminate it)
2. Slippage + commissions (positive expectancy per trade gets eaten by friction)
3. The user's own discipline (a probabilistic edge requires hundreds of trades to express)

What Whiskey buys you is a tight feedback loop on **process**, not a guarantee on **outcome**. Process grades 4-5 across 100 trades + a documented playbook = compounding edge. Process grades 2-3 + outcome-chasing = a quick way to learn nothing.

Trade well. Log everything. Whiskey will tell you what the math says, but the human still picks the chair.
