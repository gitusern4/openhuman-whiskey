# Order Flow Research Findings for TK's Mods Order Flow Card

_Research date: 2026-05-12. Sources verified. No marketing copy — vendor claims checked._

---

## 1. Order Flow Primitives 101

**Footprint Charts**
A footprint (cluster) chart shows bid volume and ask volume at every price tick within a bar instead of a single OHLCV candle. Layout: each row = one tick; each cell = `ask_vol × bid_vol`. Math: `delta_per_level = ask_vol - bid_vol`. Canonical pattern to look for: _stacked imbalance_ — three or more consecutive price levels where bid-to-ask ratio exceeds 3:1 on upticks, signaling aggressive buying with no opposing supply. Continuation signal, not reversal.

**Bid/Ask Volume Per Price Level**
`ask_vol` = volume that lifted the offer (aggressive buyers). `bid_vol` = volume that hit the bid (aggressive sellers). Source: exchange tick feed matched against the prevailing quote side. False positive risk: quote changes that are never matched are not trades and do not appear in this data.

**Delta (Per-Bar and CVD)**
`delta = ask_vol - bid_vol` per bar. Cumulative Volume Delta (CVD): `sum(delta)` from session open or an anchor bar. CVD slope tracks whether aggressive buyers or sellers are net dominant. Key pattern: a bullish price bar with deeply negative delta means sellers were aggressive but price held — selling was absorbed.

**Volume Profile (VPVR / VPFR / VPSV)**
Volume-at-Price histogram. Algorithm: bin all trades for the lookback period into tick-width price buckets, sum volume per bucket. VPVR = dynamic (recalculates on scroll). VPFR = user-anchored range. VPSV = per-session (resets daily). Display: horizontal histogram on price axis, longest bar = POC.

**Value Area (VAH / VAL / POC)**
POC = price bucket with highest volume. Value Area contains 70% of session volume, expanded iteratively outward from POC (each step adds the larger of the two-row blocks above vs. below until threshold is exceeded). VAH = upper bound; VAL = lower bound. Key patterns: price re-entering the value area from below is expected to find the POC as a magnet; three consecutive rejections at VAH with decreasing size = fade setup.

**Time-and-Sales Tape**
Chronological feed: `(timestamp, price, size, side)` for every matched trade. Patterns: rapid large-lot prints at the offer = initiative buying; alternating bid/ask prints frozen at one price = absorption at a level. Tape speed matters — a 200-lot print over 200 milliseconds carries more weight than the same size over 20 seconds.

**DOM (Depth of Market) Ladder**
Live snapshot of resting limit orders: each price level shows resting bid quantity and resting ask quantity. Refreshes with every order add, cancel, or modify. Key patterns: large static resting bid that holds when price tests it = institutional limit order defending a level. Same large bid disappearing as price approaches = spoofing / pulling liquidity (watch for traps).

**Liquidity Heatmap**
Time-series rendering of DOM depth across all price levels, scrolling left as time passes. Color intensity = resting order size. Bookmap's primary display. Uniquely preserves historical DOM state so traders can see where orders were placed and whether they were pulled or executed. Key pattern: persistent bright band at a price level that price bounced from = institutional passive interest.

**Absorption**
Price barely moves despite high aggressor volume. Detection rule: `|delta| > 1.5 × avg_bar_delta` AND `bar_range < 0.5 × avg_bar_range`. False positive risk: news spikes create high volume, small range temporarily before a break — always filter by known level proximity.

**Exhaustion**
End-of-move signal: final bar in a trend shows extreme delta and then closes in the opposite quarter of its range. Volume is typically climactic (1.5–3x average). Follow-through bar confirms. Mentor framing: "Climax bar — heaviest volume of the move on a rejection candle. Potential turn."

**Iceberg Detection**
An iceberg hides true size; the exchange refreshes a small displayed quantity as each slice is filled. Heuristic: if cumulative tape volume at price P over a rolling window exceeds the maximum DOM-displayed size at P by >5x, an iceberg is likely. ATAS uses TTW Iceberg Detector for systematic detection. False positive: algorithmic market-makers re-quoting small sizes repeatedly produce similar patterns.

---

## 2. What TradingView Exposes Natively (2025–2026)

**Available on Premium plan (confirmed):**

- Volume Profile Visible Range (VPVR) — Premium+
- Volume Profile Fixed Range (VPFR) — Premium+
- Session Volume Profile / VPSV — Essential and above
- Volume Footprint chart type (bid/ask per level, delta, bar-level POC) — Premium+; launched as a native chart type; Pine script access to footprint data added; alerts for footprint added January 2026
- Anchored VWAP (multi-anchor, up to 16 simultaneous) — Premium+
- CVD built-in indicator — available where footprint data is supported (Premium+)

**TV does NOT have (confirmed absent):**
DOM ladder, time-and-sales tape widget, liquidity heatmap, iceberg detection. These require dedicated order flow platforms.

**CDP Readability of TV Studies:**
The CDP bridge connects to `--remote-debugging-port=9222`. Injected JS can access the internal `tvWidget` object. Indicators loaded on the chart are accessible via `panes()[0].sources()` — each source object exposes `metaInfo()` (indicator name, input definitions) and per-bar series values via internal properties (`_series`, `getPlotValuesForBar()`). These paths are undocumented and may change across TV versions — treat as best-effort.

Volume Profile drawn as a native drawing tool (not a Pine study) is harder to read via CDP — it stores data in internal `_valueAreaRanges` objects not exposed as a standard series. Workaround: use a Pine-Script-based VP indicator that outputs POC/VAH/VAL as `plot()` values — these ARE readable as standard series via CDP.

The native footprint chart's per-bar delta IS exposed as a plottable series. This is the most reliable CDP read target for order flow data.

**Recommendation for builders:** Always load Pine-based VP indicators (not native drawing tools) when CDP readability is needed. Target the footprint chart's delta series as the primary CDP signal.

---

## 3. TradingView Third-Party Ecosystem

**Open-source Pine indicators (CDP-readable if loaded):**

- LuxAlgo Liquidity Structure & Order Flow — open-source; plots delta, liquidity voids, imbalance. Values readable via CDP.
- LuxAlgo Imbalance Detector — open-source; fair value gaps + volume imbalances.
- LuxAlgo Volume Delta Methods — open-source; multiple CVD calculation methods.
- LuxAlgo Institutional Order Flow Strength Classifier — open-source; initiative vs. responsive bar classification.
- AlgoAlpha Orderblock Footprints — open-source footprint emulation from OHLCV.
- Bjorgum Delta — community standard CVD proxy using tick-direction inferences.

**Paywalled / compiled (not CDP-readable):** LuxAlgo Premium toolkit scripts, AlphaTrends scripts. These use `import` from protected libraries. Computed values are not accessible via CDP because the plot series indices are not documented and vary by compilation.

**CDP readability rule:** Open-source Pine with `plot()` calls = values accessible after loading the indicator on the chart. Compiled/paywalled = black box, UI only.

---

## 4. External Order Flow Data Sources (No Broker API)

| Source | What It Gives | True Cost | Verdict |
|--------|--------------|-----------|---------|
| Databento | CME MDP 3.0 tick-by-tick, DOM snapshots, matched trades | $125 free credits; live NQ L2 ~$150+/mo thereafter | High quality, priced for institutional use |
| Polygon.io | Futures data on paid Futures plan only; free tier is delayed and equity-focused | Futures plan ~$199/mo | Not the right tool for futures scalpers |
| CME DataMine | Historical tick downloads | Per-dataset pricing | Historical only — no real-time streaming |
| IBKR TWS API | Real-time DOM (`reqMarketDepth`), quotes, trades via `reqMktData` | Free API if IBKR account exists; CME L2 data sub ~$10-30/mo | Best low-cost path if user has IBKR |
| Coinbase / Kraken | L2 order book + trade feed | Free | Crypto only — irrelevant for NQ/MES |
| Tradovate WebSocket | DOM, quotes, trade tape | See Section 5 | Best path for prop-firm Tradovate traders |

Nothing is truly free for real-time CME NQ DOM. "Marketing-free" means you pay for either the exchange data or the platform. IBKR is the lowest-cost realistic option for existing IBKR users. For a prop-firm trader already on Tradovate, the Tradovate WebSocket is the correct integration target.

---

## 5. Tradovate API Specifically

**Authentication:** POST to `https://live.tradovateapi.com/v1/auth/accessTokenRequest` with credentials. Returns `accessToken` (60-min TTL) and `mdAccessToken` (market data token, used separately).

**Two WebSocket connections required:**

- Trading WS: `wss://live.tradovateapi.com/v1/websocket` — orders, fills, positions
- Market Data WS: `wss://md.tradovateapi.com/v1/websocket` — price feeds, DOM, tape

**Market data subscription methods (exact API method names):**

- `md/subscribeQuote` — top-of-book BBO + last price + session volume
- `md/subscribeDOM` — L2 depth, array of `{price, size}` per side (typically 10 levels per side)
- `md/subscribeHistogram` — per-price volume histogram (Volume Profile proxy)
- `md/subscribeTradeTape` — real-time time-and-sales: `{timestamp, price, size, side}` per matched trade
- `md/getChart` — historical OHLCV bar data via WebSocket

**Access cost breakdown (verified from forum threads and Tradovate support docs):**

- Base account: $0/mo (higher per-contract fees) or $99/mo flat-rate — neither includes API
- API add-on: ~$25-30/mo (forum consensus; Tradovate quotes this inconsistently)
- CME Level 2 data subscription (needed for `md/subscribeDOM`): ~$48/mo through Tradovate
- Without L2 sub: `md/subscribeDOM` returns error or empty data. You get BBO + tape only.
- Prop firm accounts (Apex, TopStep): API + orderflow tools require separate activation; not automatic. Verify with the prop firm before building.

**What you get for API fee alone (~$25-30/mo, no L2 sub):** real-time BBO quotes, trade tape, chart bars — sufficient for CVD and basic tape reading.

**What requires L2 ($48/mo additional):** full DOM depth streaming.

---

## 6. Best-in-Class Order Flow Desktop Tools

**ATAS**
Flagship feature: 400+ footprint chart variants with per-cell bid/ask volume and color-coded imbalance highlighting (cells where ask:bid ratio >3x shown prominently). The Absorption indicator marks levels where large volume executed without corresponding price movement. What makes it work: direct CME tick feed, not delayed or proxied data. V1 idea to borrow: imbalance cell coloring logic — apply to our footprint emulator. Any cell where `ask_vol / bid_vol > 3.0` gets a green highlight; reverse for red.

**Sierra Chart**
Flagship feature: Numbers Bars — a footprint variant that highlights stacked imbalances in color and shows per-bar delta in a panel below. Runs on a C++ native engine at $36/mo. The Delta Divergence alert (price new high, delta lower than prior high's delta) is built-in. V1 idea to borrow: the delta divergence detection rule — implementable in pure math from TV's footprint series delta values read via CDP.

**Bookmap**
Flagship feature: liquidity heatmap that refreshes 125x/second, scrolling left as time passes, showing resting DOM as a color gradient. Uniquely retains historical DOM snapshots — traders see where large orders were placed and whether they were pulled before execution (revealing spoofing). V1 idea to borrow: the mentor commentary concept of "persistent vs. pulled liquidity" — teachable as a concept even without the heatmap data. When tape shows a level absorbed, the mentor can reference whether that level's DOM "held" or "disappeared."

**MotiveWave**
Flagship feature: Volume Imprint (footprint) combined with Elliott Wave and Fibonacci automation. More useful for swing traders than NQ scalpers. V1 idea to borrow: the bid/ask volume histogram bar below each candle — total bid vol and total ask vol as a two-bar mini-chart. Constructable from Tradovate tape data once wired.

**Volfix**
Eastern European cluster chart platform. Similar feature set to ATAS but less market penetration in US futures community. No distinctive features worth borrowing that ATAS doesn't also offer.

---

## 7. Order Flow Patterns a Mentor Should Detect and Surface

**Delta Divergence at Swing High/Low**
Detection rule: `price[0] > max(price, N, 1)` AND `delta[0] < delta[index_of_prior_N_bar_high]`. N = 5-10 bars typical for NQ scalping. False positive risk: in strongly trending markets, buyers absorb at progressively higher prices — each high may have lower absolute delta simply because fewer sellers are present, not because the move is exhausted. Filter: only flag if delta[0] is also negative (sellers net-dominant on that bar). Mentor output: "Delta diverged at 21350 — buyers pushed price to a new high but with less aggressive buying than the last push. Watch for a reversal if we fail to hold 21340."

**Absorption at Key Level**
Detection rule: price within 3 ticks of VAH/VAL/POC/VWAP AND `|delta| > 1.5 × session_avg_delta` AND `bar_range < 0.5 × session_avg_range`. False positive risk: news events create high volume with small range temporarily before a breakout. Add filter: require 2 consecutive absorption bars before flagging. Mentor output: "Heavy volume absorbed at the VAH (21400) — price couldn't break through despite aggressive selling. Bullish if we see a green follow-through."

**Iceberg at a Level (Tape-Based, Tradovate API Required)**
Detection rule: over a 10-second rolling window, sum tape volume at price P. If `tape_vol_at_P > 5 × max_DOM_displayed_at_P_in_window`, flag iceberg. Requires live tape + DOM feeds simultaneously. False positive risk: high — algorithmic market-makers produce similar patterns. Add size threshold: only flag if `tape_vol_at_P > 2 × avg_print_size × 20`. Mentor output: "Possible iceberg defending 21380 ask — tape volume at that level is 8x the displayed size. That ask may be much larger than shown."

**Low-Volume Node Break**
Detection rule: price transitions through a price range where session VP shows volume below the 5th percentile of the distribution. Requires VP data. False positive: overnight session VP thin zones don't always translate to regular session behavior. Filter: use RTH-only session profile. Mentor output: "Broke through a thin zone — low-volume nodes tend to produce fast moves. Thin air above until 21500 where volume clusters again."

**Single-Print / Naked POC**
Detection rule: record prior session POC; flag when current price is within 2 ticks. Factual level, not a predictive signal on its own — use as context for setups, not as a standalone trade. Mentor output: "Approaching yesterday's naked POC at 21320 — unvisited POCs act as price magnets. High probability of a test."

**Opening Drive vs. Responsive Trade**
Detection rule: if RTH first-5-minute bar closes in top 25% of its range (bullish drive) AND `|delta| > 1.5 × avg_delta` AND `volume > 1.5 × pre-open 5-bar avg volume` → "Opening Drive." Otherwise classify as "Responsive" or "Neutral." False positive risk: opening prints are often gapped/manipulated in futures — use only on days with normal gap-fill behavior. Mentor output: "Opening drive confirmed — buyers in control early. Bias long above 21300, target VAH."

**Finishing-Move Exhaustion**
Detection rule: 3+ consecutive bars in one direction; last bar has highest volume of the sequence AND closes in the opposite 25% of its range (bearish close on an up-move). Mentor output: "Climax bar — heaviest volume of the move with a rejection close. Weak longs may be flushing. Look for a long entry on the next green bar with confirming delta."

---

## 8. What We Can Realistically Build in 1 Week (No Broker API)

Ranked by feasibility × value:

**Rank 1 — Setup Playbook Expansion (10/10 × 9/10)**
Add order flow setup templates to `whiskey_playbook.md`: delta divergence, absorption at VA, opening drive, naked POC magnet, exhaustion bar. No code. WhiskeyMode scoring quality improves immediately. Build time: 2-4 hours.

**Rank 2 — CDP Indicator Value Reader (7/10 × 9/10)**
Extend `tradingview_cdp.rs` to enumerate loaded Pine indicators by name, find the footprint chart's delta series and/or Volume Profile Pine indicator's POC/VAH/VAL plots, and return those values to the Rust side. Inject into WhiskeyMode's context window when scoring setups. Build time: 1-2 days. Risk: TV internal API path changes.

**Rank 3 — Order Flow Workspace Preset (8/10 × 8/10)**
A button in TK's Mods that injects Anchored VWAP (from session open), Session Volume Profile, and a LuxAlgo Order Flow indicator via CDP `chart.setStudy()`. Saves 3-5 minutes of setup per session. Build time: 1 day. Fragility: TV API surface changes can break silently — log failures, degrade gracefully.

**Rank 4 — Trade Journal Order Flow Tags (9/10 × 8/10)**
Add selectable tags to each trade entry: "absorption at key level", "delta divergence", "VA reject", "opening drive follow-through", "naked POC magnet", "exhaustion bar", "iceberg suspected". Display tag win-rate breakdown in playbook stats. Build time: 1 day. Zero dependencies beyond existing journal UI.

**Rank 5 — Manual Delta Tracker (9/10 × 4/10)**
Form where user enters bid/ask volume per bar; app computes delta and running CVD sparkline. Useful fallback if CDP fails. In practice, TV Premium's built-in CVD is superior. Ship only if CDP read fails. Build time: 4-6 hours.

**Deprioritize Week 1:**

- Screenshot annotator for VP (high complexity, fragile OCR)
- Footprint emulator from OHLCV only (no tick data = misleading output)

---

## 9. What We Could Build After Wiring Tradovate API

**Real DOM Ladder Card:** 10-level live bid/ask DOM in TK's Mods sidebar. Refresh on every `md/subscribeDOM` WebSocket message. Color-code levels by size relative to session average. Show DOM imbalance (total bid depth vs. total ask depth across top 5 levels). Requires L2 subscription ($48/mo).

**Real CVD:** Accumulate `md/subscribeTradeTape` prints: `delta += (side === 'buy') ? size : -size`. Render as a sparkline. Resets at session open. No L2 required — trade tape is included in the base API add-on.

**Real Per-Bar Footprint:** Bin tape prints into price buckets matching the current bar's time range. At bar close, render a footprint grid with delta per level. Accurate because it uses actual matched print data, not inferences.

**Iceberg Detector:** Cross-reference `md/subscribeDOM` displayed size at P vs. cumulative `md/subscribeTradeTape` volume at P over a rolling 10-second window. Flag when tape volume exceeds 5x max displayed DOM size. Requires both DOM + tape feeds simultaneously.

**Auto-Tag Trades at Fill:** On fill event from the trading WebSocket, snapshot: current CVD direction, DOM imbalance (bid-heavy vs. ask-heavy), nearest key level (POC/VAH/VAL), delta of the fill bar. Auto-populate trade journal tag with this context. This is the highest-value Tradovate API use case — zero manual work from the trader.

---

## 10. Top 5 Features for the Order Flow Card in TK's Mods

**1. CDP Order Flow State Reader**
Reads which order flow studies are loaded on the active TV chart, extracts their last-bar values (footprint delta, CVD level, POC price, VWAP deviation), and injects them into WhiskeyMode's pre-trade scoring context. Data: TV CDP bridge (existing infrastructure). Build time: 1-2 days. Dependencies: `tradingview_cdp.rs`, stable Pine indicator names.

**2. Trade Journal Order Flow Tags**
Adds a curated set of order flow context tags to every trade entry. Tags feed into playbook win-rate breakdown per setup type. Data: manual user selection at trade entry. Build time: 1 day. Dependencies: existing journal UI.

**3. One-Click Order Flow Layout Preset**
Single button press injects Anchored VWAP + Session Volume Profile + LuxAlgo Order Flow indicator into TV via CDP. Saves daily setup time and ensures consistent order flow context on every chart. Data: TV CDP bridge. Build time: 1 day. Dependencies: CDP bridge + TV chart API stability.

**4. Key Level Surface Card**
Reads loaded Pine-based Volume Profile indicator to extract POC, VAH, VAL and displays them as a compact card in TK's Mods. Updates on symbol/timeframe change. Alerts when price is within 3 ticks of any level. Data: TV CDP + Pine series values. Build time: 1-2 days. Dependencies: Pine-based VP indicator must be loaded on TV chart.

**5. Tradovate Live Delta Sidebar (Phase 2)**
Streams per-tick CVD from `md/subscribeTradeTape` and live DOM imbalance from `md/subscribeDOM` as a sidebar mini-widget. Auto-tags fills with delta direction and DOM state. Data: Tradovate WebSocket API (~$25-30/mo add-on; L2 sub ~$48/mo for DOM). Build time: 3-5 days after API skeleton is wired. Dependencies: Tradovate API connectivity, prop firm permission check.

### Build Queue

1. Order flow setup templates added to `whiskey_playbook.md` — content only, no code
2. Trade journal order flow tags — UI + storage extension
3. CDP indicator detection + last-value extraction for loaded TV studies
4. One-click order flow layout preset button via CDP
5. Key Level Surface Card (POC/VAH/VAL from CDP Pine series)
6. Tradovate API WebSocket client — auth flow + reconnection logic skeleton
7. CVD sparkline from Tradovate `md/subscribeTradeTape`
8. DOM Ladder card (requires L2 subscription confirmation from prop firm)
9. Per-bar footprint grid from tape accumulation
10. Iceberg detector + auto-trade-tagging at fill event

---

## Sources

- [TradingView Volume Footprint Complete Guide](https://www.tradingview.com/support/solutions/43000726164-volume-footprint-charts-a-complete-guide/)
- [TradingView Volume Footprint Launch Blog](https://www.tradingview.com/blog/en/new-chart-type-volume-footprint-44399/)
- [TradingView Volume Footprints in Pine Scripts](https://www.tradingview.com/blog/en/volume-footprints-in-pine-scripts-56908/)
- [TradingView Footprint Alerts (Jan 2026)](https://www.tradingview.com/blog/en/alerts-for-volume-footprint-charts-55183/)
- [TradingView VWAP Support](https://www.tradingview.com/support/solutions/43000502018-volume-weighted-average-price-vwap/)
- [TradingView VPVR](https://www.tradingview.com/support/solutions/43000703076-visible-range-volume-profile/)
- [TradingView Session Volume Profile](https://www.tradingview.com/support/solutions/43000703072-session-volume-profile/)
- [TradingView Volume Profile Concepts](https://www.tradingview.com/support/solutions/43000502040-volume-profile-indicators-basic-concepts/)
- [Pine Script v6 Reference Manual](https://www.tradingview.com/pine-script-reference/v6/)
- [Pine Script v6 Launch](https://www.tradingview.com/blog/en/pine-script-v6-has-landed-48830/)
- [LuxAlgo Liquidity Structure & Order Flow](https://www.tradingview.com/script/FjoX7GeT-Liquidity-Structure-Order-Flow-LuxAlgo/)
- [LuxAlgo Imbalance Detector](https://www.tradingview.com/script/C0cC294Q-Imbalance-Detector-LuxAlgo/)
- [LuxAlgo Volume Delta Methods](https://www.tradingview.com/script/OhLE0vnH-Volume-Delta-Methods-Chart-LuxAlgo/)
- [LuxAlgo Institutional Order Flow Strength Classifier](https://www.tradingview.com/script/gesy3qTZ-Institutional-Order-Flow-Strength-Classifier-LuxAlgo/)
- [Tradovate Partner API — Market Data](https://partner.tradovate.com/overview/core-concepts/web-sockets/market-data/market-data)
- [Tradovate API Access Support](https://support.tradovate.com/s/article/Tradovate-API-Access)
- [Tradovate Market Data Subscriptions](https://support.tradovate.com/s/article/Subscribing-to-Tradovate-Market-Data)
- [Tradovate Forum — md/subscribeQuote not found](https://community.tradovate.com/t/not-found-md-subscribequote/4451)
- [Tradovate Forum — Bid-Ask Volume](https://community.tradovate.com/t/bid-ask-volume/4862)
- [Tradovate Forum — Time & Sales subscription](https://community.tradovate.com/t/how-to-subscribe-to-time-sale-data/6103)
- [Tradovate TypeScript SDK (community)](https://github.com/cgilly2fast/tradovate-typescript)
- [IBKR TWS API Market Depth](https://interactivebrokers.github.io/tws-api/market_depth.html)
- [IBKR TWS API Streaming Market Data](https://interactivebrokers.github.io/tws-api/market_data.html)
- [IBKR Market Data Subscriptions](https://www.interactivebrokers.com/campus/ibkr-api-page/market-data-subscriptions/)
- [ATAS Platform](https://atas.net/)
- [ATAS Footprint Charts](https://atas.net/footprint-charts/)
- [ATAS Heatmap](https://atas.net/blog/heatmap/)
- [Bookmap Features](https://bookmap.com/en/features)
- [Bookmap Footprint Add-on](https://bookmap.com/knowledgebase/docs/Addons-Footprint)
- [Bookmap Iceberg Orders](https://bookmap.com/blog/how-to-read-and-trade-iceberg-orders-hidden-liquidity-in-plain-sight)
- [Bookmap CVD Strategy](https://bookmap.com/blog/how-cumulative-volume-delta-transform-your-trading-strategy)
- [Bookmap Stop Runs with CVD and Iceberg](https://bookmap.com/blog/detecting-stop-runs-using-cvd-and-iceberg-absorption-for-strategic-trading)
- [Best Order Flow Platforms 2026 Comparison](https://tradingtoolshub.com/blog/best-order-flow-trading-platforms-2026-bookmap-vs-sierra-chart-vs-jigsaw/)
- [QuantVPS Footprint Chart Platforms](https://www.quantvps.com/blog/analyzing-footprint-charts)
- [QuantVPS Quantower Alternatives](https://www.quantvps.com/blog/quantower-alternatives)
- [Databento Futures Data](https://databento.com/futures)
- [Databento Pricing](https://databento.com/pricing)
- [Polygon.io Pricing](https://polygon.io/pricing)
- [Delta Divergence — Trade The Matrix](https://www.tradethematrix.net/post/delta-divergence)
- [Trader Dale — Absorption and Delta Analysis (May 2025)](https://www.trader-dale.com/order-flow-analysis-how-to-use-absorption-delta-to-confirm-trade-entry-13th-may-25/)
- [TradeRiot — Orderflow Delta vs Liquidity](https://tradingriot.com/orderflow-trading)
- [TradingView MCP Bridge (reference CDP pattern)](https://github.com/tradesdontlie/tradingview-mcp)
- [Value Area Guide](https://howtotrade.com/trading-strategies/value-area/)
- [Volume Profile POC and Value Area — GoCharting](https://gocharting.com/docs/orderflow/volume-profile-charts)
- [Apex Trader Funding — Tradovate Orderflow Tools](https://apextraderfunding.com/help-center/tradovate/tradovate-trading-tools-add-ons-orderflow-level-2-data/)
