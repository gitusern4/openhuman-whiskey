# Execution Layer Research — Whiskey AI-Mediated Trade Execution

**Branch:** `execution-research` (off `whiskey` HEAD `045ee6cf`)
**Date:** 2026-05-12
**Audience:** Build agents implementing the Whiskey execution layer
**Status:** Pre-build research — spec, not implementation

---

## 1. Regulatory Posture

### The discretionary vs. automated line

For individual retail traders (not registered as CTAs, FCMs, or broker-dealers), the operative question is whether Whiskey's involvement constitutes the trader's own discretionary decision or an independent automated system.

**The key distinction under CFTC Regulation AT** (proposed 2015, never finalized in full, but guidance remains operative): Reg AT targeted "algorithmic trading" defined as systems that generate or route orders automatically without human intervention in each individual order decision. Retail individual traders were explicitly carved out from the registration requirements targeting AT Persons — those obligations apply to FCMs, floor brokers, swap dealers, CTAs, and certain proprietary traders operating on DCMs.

**CFTC December 2024 Staff Advisory (Letter No. 24-17)**: The CFTC's Divisions of Clearing and Risk, Data, Market Oversight, and Market Participants issued guidance reminding registered entities of their supervision obligations under CFTC Regulation 166.3. Critically, this advisory targets *registered* entities — it does not impose new obligations on unregistered retail traders trading their own accounts. The advisory flags "lack of explainability in black-box models that cannot be audited" as a concern for registered entities.

**Bottom line for Whiskey v1:**

| Pattern | Classification | Registration Risk |
|---|---|---|
| LLM proposes order, user clicks "Confirm" | Discretionary (user decides each order) | None for retail individual |
| LLM auto-submits under user-defined rules ("exit at -1R always") | Borderline automated | Still likely none for retail individual trading own account |
| System sells strategies to other accounts | CTA registration likely required | Out of scope for v1 |

**The confirmation click matters legally**: If the user must affirmatively confirm each order, the human is the decision-maker. The LLM is an analytical tool, not a trading system. This is the same legal standing as a Bloomberg terminal surfacing a trade idea — the terminal doesn't execute, the human does.

**FINRA Rule 3110** applies to broker-dealers and their associated persons supervising algorithmic strategies. It does not apply to individual retail traders. The registration as "Securities Trader" under FINRA Regulatory Notice 16-21 applies to persons at member firms *primarily responsible* for designing algorithmic strategies — not retail customers.

**SEC Form ATS / Regulation ATS** applies to systems that operate as exchanges or match multiple parties' orders. A single-user system routing only that user's orders to a registered exchange does not constitute an ATS.

**Prop firm constraint**: Apex and TopStep's account agreements may independently prohibit fully automated trading. Apex explicitly requires human discretion per their rules. This is a contractual, not regulatory, constraint — but it's breach-of-contract risk, not legal risk. For prop accounts, the confirmation dialog is *required* to comply with firm rules regardless of the regulatory picture.

---

## 2. Per-Broker Execution APIs for Futures

### 2.1 Tradovate (`api.tradovate.com`)

**Auth model**: OAuth2 / JWT. POST to `/auth/accesstokenrequest` with credentials → receive `accessToken` (short-lived) + `mdAccessToken` for market data. Tokens must be refreshed before expiry via `/auth/renewAccessToken`.

**Order placement**: `POST /order/placeOrder`

```json
{
  "accountSpec": "ACCOUNT_NAME",
  "accountId": 12345,
  "action": "Buy",
  "symbol": "MESH5",
  "orderQty": 1,
  "orderType": "Limit",
  "price": 5200.25,
  "isAutomated": true
}
```

The `isAutomated: true` flag is **mandatory** per CME Group regulations for API-submitted orders. Omitting it on CME-listed instruments is an exchange rule violation.

**Order status WebSocket**: `wss://md.tradovateapi.com/v1/websocket` for market data; `wss://live.tradovateapi.com/v1/websocket` for order/account events. Subscribe via `user/syncrequest` after auth. Fills arrive via `fill` entity updates; position changes via `position` updates.

**Cancellation**: `POST /order/cancelOrder` with `{ "orderId": 123456 }`.

**Partial fill handling**: Position and fill entities update incrementally. The client must track cumulative filled qty against ordered qty. No built-in "partial fill alert" — monitor `position.netPos` changes against expected.

**Daily loss kill mechanism**: **Client-enforced only**. Tradovate does not provide a server-side daily P&L kill switch via the API. The `cashBalances` entity (delivered over WebSocket) reflects realized P&L only — open P&L must be computed client-side. This is a critical safety gap: **if Whiskey crashes, there is no server-side backstop on Tradovate**.

**Prop firm constraint (Apex, TopStep on Tradovate)**: **Prop accounts do not allow direct API access.** Confirmed by Tradovate support and community forums. Apex and Tradeify ship Tradovate as the execution platform but lock the API on evaluation and funded accounts. TopStep has migrated to its own TopStepX platform. **Tradovate API execution is only viable for Tradovate personal accounts, not prop firm funded accounts.**

**Market data**: CME real-time data via API WebSocket requires a CME Individual License Agreement sub-vendor registration ($290–500/month depending on tier). This cost must be disclosed to the user.

### 2.2 IBKR TWS API

**Auth model**: TWS (Trader Workstation) must be running locally. Connection via socket to localhost:7496 (live) or 7497 (paper). No cloud-native API key auth — requires local TWS instance with API enabled.

**Order placement**: `placeOrder(orderId, contract, order)` via EClient socket interface. Supports bracket orders (parent + two attached child orders with OCA group).

**Order status**: `openOrder()`, `orderStatus()`, `execDetails()` callbacks via EWrapper. Persistent TCP socket.

**Daily loss kill**: IBKR offers a **server-side "Triggered by Loss Restriction"** via Pre-Trade Compliance. When a daily loss limit is breached, IBKR can reject all new orders or allow only closing orders for the remainder of the session. **This is the only broker in this list with a documented server-side daily loss enforcement mechanism.** Risk Navigator provides real-time risk monitoring.

**Cancellation**: `cancelOrder(orderId)` via EClient.

**Limitations for v1**: Requires TWS running on the user's machine — Whiskey cannot connect to IBKR from a cloud process. Suitable if Whiskey runs as a desktop sidebar process alongside TWS.

### 2.3 NinjaTrader 8 ATM

Local-only. NT8 must be running. CrossTrade offers a REST adapter that proxies to a locally running NT8 instance. **v1 recommendation**: Low priority. Too many local dependencies.

### 2.4 Tastytrade

REST + WebSocket. Session token auth. Order placement `POST /accounts/{account_number}/orders`. No documented server-side daily loss kill via the API. Primarily an options platform — futures is secondary.

### 2.5 TopStepX (ProjectX Gateway)

**Auth model**: API key → Bearer token. `Authorization: Bearer {api_key}`. REST + WebSocket.

**Cost**: $29/month for API access (50% discount for active TopStep traders).

**Order placement**: REST with native bracket order support via `stopLossBracket` and `takeProfitBracket` parameters (in ticks). Cleanest bracket order implementation of any broker listed.

**Order status**: WebSocket streaming for real-time fills.

**Cancellation**: REST DELETE on order resource.

**Daily loss kill**: TopStep enforces drawdown rules server-side at the account level as a firm rule — if a trader breaches the daily loss limit, the firm's risk system locks the account. Not API-callable but always active.

**Critical finding**: TopStepX is the **recommended primary broker target for v1**. Built on ProjectX, specifically designed for prop firm trading. API access documented and available. Most prop-firm-aligned safety primitives.

---

## 3. Safety Primitives Professionals Use

Anything not code-enforced is a vibe, not a primitive.

### 3.1 Hard Daily-Loss Kill Switch

**Server-enforced (survives Whiskey crash):**
- IBKR Pre-Trade Compliance: configurable, rejects all new non-closing orders. Server-side. Survives client disconnect.
- TopStep firm-level drawdown: server-side by the prop firm, always active.
- Tradovate: **no server-side kill**. Must be client-enforced.

For Tradovate accounts, Whiskey must maintain a persistent server-side process monitoring P&L and submitting flatten orders if daily loss is breached, independent of the UI process.

### 3.2 Per-Trade Confirmation Dialog

**Legal function**: Preserves discretionary trading classification.
**Safety function**: Catches LLM errors before fills.
**Implementation**: Modal with 3-second mandatory display before "Confirm" button becomes active. Show: instrument, direction, qty, entry type, stop ticks, target ticks, estimated R in dollars. User cannot confirm until countdown completes.

### 3.3 Cooldown Timer

After any loss (realized P&L decline), disable "Propose Trade" button for N seconds. N scales with session loss count.

### 3.4 Maximum Order Size Cap

Code-enforced. Session-scoped immutability. Cannot be raised mid-session.

### 3.5 Whitelisted Instruments

Stored in covenant config. Any order on non-whitelisted instrument silently rejected with log entry.

### 3.6 Pre-Trade Plausibility Check

Before sending any order:

1. Direction consistency: LLM's stated direction must match order `action`. Mismatch → abort + log.
2. Price sanity: limit price within 0.5% of last quote. Wider → abort + log.
3. Stop required: orders without a stop leg are rejected. No bypass.
4. Qty sanity: ≥ 1 and ≤ session cap.
5. Account state: confirmed connection, non-stale auth token.

### 3.7 Audit Log

Append-only. No edits. Schema in Section 7.

### 3.8 Kill Switch UI

1. Revoke broker session token (not just disable local UI).
2. Cancel all open orders before token revocation.
3. Flatten all positions before token revocation (market orders).
4. Set persistent `kill_engaged` flag (disk, not RAM).
5. Manual reset only — no automatic re-enable.

Red, large, no confirmation dialog. One click. Killing fast is the point.

---

## 4. AI-Gated Execution Patterns from the Field

### 4.1 Anthropic Computer Use Beta Guardrails

Anthropic's Computer Use documentation: "Ask a human to confirm decisions that may result in meaningful real-world consequences as well as any tasks requiring affirmative consent, such as accepting cookies, executing financial transactions, or agreeing to terms of service."

Agent loop implements `max_iterations` (default 10) to prevent runaway. Prompt injection classifiers run on screenshots — model steered to ask for confirmation when potential injections detected.

**Whiskey takeaway**: Anthropic's own guidance on financial transactions is "always confirm with a human." This is the design pattern.

### 4.2 Devin / Cognition Deployment Pattern

Devin requires human approval at two checkpoints: planning (before any action) and PR (before any code lands). Human review is mandatory for any "destructive or irreversible database operations."

**Whiskey**: Trade entry = planning checkpoint (LLM proposes, user confirms). Position close = either user-initiated or kill-switch-initiated. LLM should never autonomously close a position.

### 4.3 LLM-Mediated Execution in Production

**Composer.trade**: $200M+ daily volume by 2025. Proprietary trading language with LLM-built strategies, sub-second backtesting. Full automation — no per-trade human confirmation. Works for their use case (algorithmic strategies on equities/options) but legally distinct from Whiskey's discretionary futures model.

**TradingAgents (open source, research)**: Multi-agent LLM framework. Portfolio Manager "approves/rejects the transaction proposal." Closest public research implementation to Whiskey's model. No production deployment on prop firms documented.

**MQL5 / MetaTrader**: 340+ EAs with "AI" branding. Most are marketing — RSI crossovers with GPT logos.

**No documented prop firm production LLM execution systems found.** Field is pre-production.

### 4.4 The Two-Key Pattern

No existing platform implements OTP/PIN-based trade confirmation. Closest analogs:
- Dangerous hotkeys assigned to two-hand modifier combos
- IBKR TWS hotkey confirmation dialog (0.5-second enforced delay for flatten)

**Proposed implementation**: For orders above 2x normal size OR first order after a loss streak of 2+, require user to type a 4-digit session code (generated at session start, displayed in non-interactive UI). Code changes each session. Prevents "mindless confirm" muscle memory.

---

## 5. Order Types and Risk-Defined Entries

### v1 Order Type Support

**Must ship (v1 blocker):**

1. **Bracket order (DEFAULT)**: Entry + stop-market + limit target as atomic OCA group. Single-leg market orders disabled by default — user must explicitly enable in covenant and acknowledge risk.

2. **OCO**: Protective legs. If broker doesn't natively support OCA (Tradovate does, TopStepX does via bracket), simulate via WebSocket monitoring + client-side cancel of surviving leg on fill.

**Stop-market vs. stop-limit**: Use **stop-market** for protective stops. Stop-limit carries false-fill risk in fast markets. Stop-limit acceptable for targets in liquid instruments only.

**Defer to v2:**
- Trailing stops (broker support varies, complicates plausibility check)
- Conditional orders
- Scale-in / scale-out

---

## 6. Kill-Switch Architecture

### Trigger Conditions

| Trigger | Type | Authority |
|---|---|---|
| Manual button click | Immediate | User |
| Daily loss limit hit | Automatic | System |
| Consecutive loss count ≥ N | Automatic | System |
| Broker auth failure | Automatic | System |
| External webhook (future) | Automatic | System |

### Action Sequence

1. Set `kill_engaged = true` in persistent state (disk).
2. Cancel all open orders (5s max timeout per order).
3. Flatten all positions via market orders. Flag as kill-switch-initiated in audit log.
4. Revoke broker session token.
5. Disable order submission UI.
6. Write kill event to audit log: trigger condition, timestamp, positions at kill, P&L at kill.
7. Send notification (push/SMS if configured).

### Reset Authority

- Reset requires: 30 minutes elapsed AND user types confirmation phrase ("I am ready to trade"). No bypass.
- Session-end kill: resets automatically at next session start.
- Manual kill during losing streak: lockout duration scales with consecutive losses (2 = 15 min, 3 = 30 min, 4+ = until next session).

### Physical Equivalent

Broker token revocation IS the USB e-stop. Once revoked, no orders submittable regardless of UI state. Document a QR code / URL the user can open on their phone to trigger the kill endpoint if trading machine freezes.

---

## 7. Audit Trail Format

**File**: append-only JSONL. No deletes. No edits. Rotated daily.

**Schema**:

```json
{
  "timestamp_utc": "2026-05-12T14:32:01.423Z",
  "actor": "whiskey|user|system",
  "action": "proposal|confirm|send|fill|partial_fill|cancel|kill|session_start|session_end|covenant_check",
  "instrument": "MESH5",
  "qty": 1,
  "price": 5200.25,
  "stop": 5195.50,
  "target": 5210.00,
  "R_estimate": 125.00,
  "confidence_pct": 78,
  "playbook_match_id": "breakout-orb-v2",
  "idempotency_key": "uuid-v4-per-order-attempt",
  "broker_response": {"orderId": 98765, "status": "Working"},
  "session_loss_count": 1,
  "daily_pnl_at_action": -312.50,
  "kill_engaged": false,
  "notes": "free text, LLM reasoning summary, max 500 chars"
}
```

**Immutability**: File opened in append mode only. No seek-and-write. Separate process or OS-level ACL prevents trading process from modifying existing lines.

---

## 8. UX Patterns That Prevent Revenge Trading

**Must all be visible in the primary trading UI.**

1. **Consecutive losses counter**: "Losses today: 2 / 3 max" prominent. Red at 2, locked at max.
2. **Cooldown timer after each loss**: 3-second base, +1 second per consecutive loss. At 3 consecutive losses: 6-second confirm + mandatory 5-minute walk-away.
3. **Daily risk gauge**: "Today's P&L: -$312 / -$500 daily max" as horizontal bar. Orange at 60%, red at 80%, locked at 100%.
4. **Forced walk-away**: Single loss > 0.75R of daily budget → 5-minute UI lockout with countdown.
5. **Cooldown escalation on confirm**: Base 3s, +1s per consecutive session loss.
6. **Session P&L always visible**: Not hidden. Not collapsible. Unrealized included.
7. **Post-loss review prompt**: After stop hit, before re-enabling order entry: "What happened? (a) Setup was invalid, (b) Execution was fine, setup failed, (c) I deviated from the plan." Response logged.

---

## 9. The Covenant Problem

### Covenant Clauses for v1 (Code-Enforced)

Previous "Whiskey never executes" clause retired. Replaced with bounded execution authority via `covenant.toml`:

```toml
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
```

**Code enforcement**:
- Loaded once at session start. Any modification requires app restart.
- Session start writes covenant hash to audit log. Mid-session changes detectable.
- Trading process reads values into immutable constants. No runtime override.
- `no_trading_after` / `no_trading_before` enforced at order submission layer, not just UI.
- `require_per_trade_confirm = true` hardcoded in v1. The config key exists but `false` rejected with startup error.

**Self-override friction**: User can edit `covenant.toml` between sessions (their computer). The covenant works as a friction device — edits require restart (cooling-off period). A losing trader in the heat of the moment will not close the app, edit a config file, and restart. A calm trader the night before can adjust thoughtfully.

**Proposed `whiskey_covenant.md` update**:

> **Execution authority granted (v1, effective 2026-05-12)**: Whiskey may propose and (with user confirmation) submit orders to connected broker accounts. All submissions are subject to the covenant config enforced at code level. Whiskey will never submit an order without a confirmed stop-loss. Whiskey will never override a kill switch. The user acknowledges that removing safety primitives from `covenant.toml` is a breach of the spirit of this covenant, even if technically possible.

---

## 10. Top 5 Features for the Execution Layer

### Feature Ranking Matrix

| Rank | Feature | Broker | Data Sources | Est. Build | Safety Primitives Required |
|---|---|---|---|---|---|
| 1 | Bracket Order Submission (TopStepX) | TopStepX | Market quote, covenant | 3–5 days | All |
| 2 | Kill Switch (client-side, Tradovate) | Tradovate | P&L WS, position state | 2–3 days | Audit, persistent flag, token revoke |
| 3 | Audit Trail + Covenant Loader | Both | Local filesystem | 1–2 days | Foundation |
| 4 | Revenge-Trading UX | Both | Session state, daily P&L | 2–3 days | Audit, kill switch |
| 5 | LLM Proposal → Confirm Dialog | Both | LLM output, plausibility check | 2–3 days | Covenant, whitelist, plausibility |

### Build Queue (10 items, implementation order)

1. `covenant.toml` schema + loader + startup hash log
2. Append-only JSONL audit writer
3. Broker connection layer (TopStepX auth + WebSocket)
4. Kill switch backend (cancel all + flatten + token revoke + persistent flag)
5. Kill switch UI (red button, always visible, no confirm)
6. Plausibility check module (direction, price, stop required, qty cap, whitelist)
7. Bracket order submission (TopStepX `stopLossBracket` + `takeProfitBracket`)
8. Confirm dialog with countdown
9. Revenge-trading UX (loss counter, cooldown, daily P&L gauge, walk-away)
10. LLM proposal renderer (structured JSON → confirm card with R-estimate)

---

## Sources

- [CFTC Staff Advisory on AI (December 2024, Letter No. 24-17)](https://www.cftc.gov/csl/24-17/download)
- [CFTC Regulation AT Federal Register](https://www.cftc.gov/sites/default/files/idc/groups/public/@newsroom/documents/file/federalregister112415.pdf)
- [CFTC Press Release on Reg AT Approval](https://www.cftc.gov/PressRoom/PressReleases/7283-15)
- [CFTC AI Advisory Press Release 2024](https://www.cftc.gov/PressRoom/PressReleases/9013-24)
- [Greenberg Traurig: Reviewing 2024 CFTC AI Initiatives](https://www.gtlaw.com/en/insights/2025/1/reviewing-2024-cftc-ai-initiatives-and-looking-ahead)
- [Sidley Austin: AI in US Financial Markets 2025](https://www.sidley.com/en/insights/newsupdates/2025/02/artificial-intelligence-us-financial-regulator-guidelines-for-responsible-use)
- [FINRA Rule 3110](https://www.finra.org/rules-guidance/rulebooks/finra-rules/3110)
- [FINRA Regulatory Notice 16-21](https://www.finra.org/rules-guidance/notices/16-21)
- [Tradovate API Official Docs](https://api.tradovate.com/)
- [Tradovate API Access Support](https://support.tradovate.com/s/article/Tradovate-API-Access)
- [Tradovate Forum: Prop Account API Access](https://community.tradovate.com/t/api-access-for-propfirm-accounts/10348)
- [Tradovate Forum: Placing and Tracking Orders](https://community.tradovate.com/t/placing-and-tracking-orders/5162)
- [Tradovate Risk Settings](https://support.tradovate.com/s/article/Risk-Settings-Tradovate)
- [TopStepX API Access Help](https://help.topstep.com/en/articles/11187768-topstepx-api-access)
- [ProjectX Gateway API Documentation](https://gateway.docs.projectx.com/)
- [ProjectX Python SDK Docs](https://project-x-py.readthedocs.io/en/latest/)
- [IBKR TWS API Documentation](https://www.interactivebrokers.com/campus/ibkr-api-page/twsapi-doc/)
- [IBKR Triggered by Loss Restriction](https://www.ibkrguides.com/pretradecompliance/trigger-by-loss-restriction.htm)
- [IBKR Setting Trading Restrictions via Pre-Trade Compliance](https://www.interactivebrokers.com/campus/trading-lessons/setting-trading-restrictions/)
- [IBKR Risk Navigator Introduction](https://www.interactivebrokers.com/campus/trading-lessons/introduction-to-ibkrs-risk-navigator/)
- [IBKR TWS API Bracket Orders](https://interactivebrokers.github.io/tws-api/bracket_order.html)
- [Tastytrade Open API](https://tastytrade.com/api/)
- [Tastytrade API Overview](https://developer.tastytrade.com/api-overview/)
- [NinjaTrader ATM Help Guide](https://ninjatrader.com/support/helpguides/nt8/advanced_trade_management_atm.htm)
- [CrossTrade NT8 REST API](https://crosstrade.io/crosstrade-api)
- [Anthropic Computer Use Documentation](https://docs.anthropic.com/en/docs/build-with-claude/computer-use)
- [Anthropic: Building Effective Agents](https://www.anthropic.com/research/building-effective-agents)
- [Cognition: Devin Annual Performance Review 2025](https://cognition.ai/blog/devin-annual-performance-review-2025)
- [Composer Trade With AI](https://www.composer.trade/ai)
- [Composer $200M Daily Volume Announcement](https://www.businesswire.com/news/home/20251021050436/en/Composer-Supercharges-Investing-Platform-with-New-Trade-With-AI-Tool)
- [TradingAgents Multi-Agent LLM Framework (arXiv)](https://arxiv.org/abs/2412.20138)
- [Bookmap: Think Before You Trade](https://bookmap.com/blog/think-before-you-trade-how-pre-planning-every-click-improves-execution)
- [Bookmap: The 3-Legged Stool of Trade Quality](https://bookmap.com/blog/the-3-legged-stool-of-trade-quality-context-execution-and-behavior)
- [CrossTrade: Understanding Tradovate API Rate Limits](https://crosstrade.io/blog/understanding-tradovate-api-rate-limits)
- [CrossTrade: Daily Loss Limits](https://crosstrade.io/learn/risk-management/daily-loss-limits)
- [MQL5 Blog: Prop Firm Kill-Switch Engineering](https://www.mql5.com/en/blogs/post/767321)
- [NYIF: Trading System Kill Switch Analysis](https://www.nyif.com/articles/trading-system-kill-switch-panacea-or-pandoras-box)
- [Optimus Futures: OCO and Bracket Orders Explained](https://learn.optimusfutures.com/oco-bracket-orders)
- [LLM Trading Bots Comparison (FlowHunt)](https://www.flowhunt.io/blog/llm-trading-bots-comparison/)
