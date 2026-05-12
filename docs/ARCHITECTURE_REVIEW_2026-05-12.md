# Architecture Review — Whiskey-on-OpenHuman fork

_Reviewer: senior architect (cross-PR layer). Date: 2026-05-12._
_Trunk audited: `whiskey` at `045ee6cf`. Open PRs: #4, #5, #6, #7._

This review goes beyond per-PR checks. The fork is now four parallel
build threads (TV CDP bridge, TK's Mods suite, order-flow split into
TS/Rust halves, onboarding wizard) and the contract drift between
threads is the dominant risk.

---

## 0. Severity-ranked summary

### Do NOT ship until fixed (block list)

1. **PR #6 ↔ PR #7 contract drift is total.** They cannot run together.
   Preset IDs, command names, config shape, and return shapes all
   diverge. Either #6 or #7 must rewrite to match the other before
   either merges to whiskey.
2. **PR #5 onboarding `DoneStep` routes to `/settings/modes` with a
   TODO**. The `/settings/tks-mods` route now exists on whiskey HEAD
   (`app/src/components/settings/panels/TksModsPanel.tsx`); the CTA
   target must be updated before the wizard ships.
3. **`publish_attention` does a synchronous disk read of
   `tks_mods_config::load()` on every event** (`src/openhuman/overlay/bus.rs:52`).
   On a heartbeat-driven path this is a write-amplification + perf
   hazard. Must be cached (mtime-keyed or in-process `OnceLock` with
   a settings-change invalidation hook).
4. **`tv_cdp_eval` is registered as a Tauri invoke handler**
   (`lib.rs:2137`). Its doc claims it is gated by the WhiskeyMode tool
   allowlist; that is false — the allowlist filters LLM tool calls,
   not Tauri IPC. Any renderer code (including a future in-TV overlay
   panel writing through the planned outbox) can call it with arbitrary
   JS. Allowlist enforcement must move to the command itself.

### Ship now, fix in v2

- Module name mismatch: `tks_mods_config.rs` writes to `tks_mods.toml`
  (line 23) but the charter calls it `tks_mods_config.toml`. Either
  rename the file or the charter — a future migration will need to
  know which is canonical.
- TV CDP JS snippets: `JS_GET_ORDER_FLOW_STATE` in PR #6 returns
  silent nulls when TV moves an API path. Acceptable, but the `_probe`
  field needs to surface to the UI for "TV moved this" diagnosis.
- Test theatre: PR #4's tests verify the stub error message rather
  than real bridge behavior. Acceptable while the bridge is stubbed
  but add a single integration test gated on env var as soon as
  webview_apis lands the `tradingview.*` handlers.
- Several existing `mascot_summon_hotkey.rs` sites still `.unwrap()`
  on poisoned mutex (WHISKEY_AUDIT.md M9, never closed).

---

## 1. Cross-PR contract drift (PR #6 ↔ PR #7) — BLOCKER

This is the most important finding in this review. The order-flow
work is split in two PRs that **independently designed the
TS↔Rust contract**, and they do not agree on anything except the
six tag string IDs.

### Command name drift

| Frontend (PR #6) invoke call | Rust (PR #7) registered name |
|------------------------------|------------------------------|
| `order_flow_get_config` | `order_flow_get_config` ✓ |
| `order_flow_save_config` (`useOrderFlow.ts:206`) | `order_flow_set_config` ✗ |
| `tv_cdp_get_order_flow_state` | only registered by PR #6 itself in `tradingview_cdp.rs:+904` — **PR #7 has no equivalent**; PR #7 uses `order_flow_record_bar` instead |
| `order_flow_apply_preset` | `order_flow_apply_preset` ✓ (name match, shape mismatch — see below) |
| `order_flow_tag_active_trade` | `order_flow_tag_active_trade` ✓ (name match, allowed-values mismatch) |

`order_flow_save_config` does not exist Rust-side. Every alert-toggle
or config save from `OrderFlowCard.tsx:548` (the `toggleAlert`
checkbox handler) will hit a Tauri "no such command" error.

### Preset ID drift

PR #6 `app/src/components/settings/panels/OrderFlowCard.tsx:24-49`:

```
'vwap_vpvr_avwap'
'delta_cvd'
'full_order_flow'
```

PR #7 `src/openhuman/modes/order_flow.rs:413-420`:

```
'vwap_profile_anchored'
'standard_orderflow'
'delta_focused'
```

**No overlap.** Every call from the UI's preset selector through
`applyPreset()` will hit Rust's `lookup_preset()`, return `None`,
and the user gets `Err("unknown preset")`. The feature is dead on
arrival.

### Config shape drift

PR #6 `useOrderFlow.ts:25-30` and the test fixture at
`OrderFlowCard.test.tsx:54-63` model the config as:

```ts
{ enabled, poll_interval_ms, active_preset, alert_toggles:
  { delta_divergence, absorption, single_print_rejection } }
```

PR #7 `src/openhuman/modes/order_flow.rs:445-477` models it as:

```rust
{ workspace_presets, manual_delta_tracker, tag_chips,
  detection_alerts, polling_hz, divergence_lookback,
  absorption_volume_multiplier, absorption_range_fraction }
```

These are not the same struct with renamed fields; they are
**different abstractions of "what is order-flow config"**. PR #6
models per-pattern alert toggles; PR #7 models per-section
enable flags + detection sensitivity. Both views are defensible.
**One of them must be deleted.**

### Apply-preset return shape drift

PR #6's `OrderFlowCard.test.tsx:71` mocks the response as
`{ ok: true, preset_id, indicators_added, error: null }`. PR #7's
`order_flow_apply_preset` (`app/src-tauri/src/order_flow_commands.rs:+148`)
returns `Result<(), String>` — `()` on success, no preset_id, no
indicators_added. The UI's "Applied: X" success path
(`OrderFlowCard.tsx:380`) reads from the local `WORKSPACE_PRESETS`
array, not from the response, so it would survive the drift for
that path — but only because it doesn't trust the backend at all.

### Recommendation

**Block merge of both PRs.** Pick one as the authoritative contract
(I recommend PR #7's Rust `CONTRACT.md` — it is more disciplined,
const-allowlist-enforced, with input validation at the boundary) and
have PR #6 rewrite the TS types and command call sites to match. The
TS preset IDs are the most consequential rename: every persisted
preset id in user installs already on a pre-merge build would be
unknown post-merge, but there are no such installs yet.

---

## 2. State machine race conditions — TV CDP session lifecycle

The CDP session is held in `TvCdpState(Arc<Mutex<Option<TvCdpSession>>>)`
(`tradingview_cdp.rs:86`). Three present-or-imminent code paths touch
this state machine:

1. The manual `tv_cdp_attach` Tauri command (existing, line 428).
2. The CDP auto-attach supervisor (PR #8/#9, not yet on origin —
   review forward-looking).
3. The in-TV overlay's MutationObserver (also PR #8/#9).

**Already a race today, even before #8/#9:** `tv_cdp_attach` at line
437-441 does:

```
let mut guard = state.0.lock().await;
if let Some(mut old) = guard.take() {
    let _ = crate::cdp::detach_session(&mut old.conn, &old.session_id).await;
}
```

The detach runs **inside** the guard. If two `attach` calls land
concurrently (user double-clicks the Attach button; or the auto-attach
fires while the user is mid-click) the second one blocks on the
guard while the first one's `detach_session` awaits a CDP response.
Then the second runs and does the discovery+attach. Fine — except
that `discover_browser_ws` (line 444) runs **after** the guard is
released between two attaches (line 441-444 — no, actually the guard
is held only in the inner block, then released). Re-reading: lines
437-442 take the guard, drop the old session, release the guard.
Then lines 444-455 do the discovery + handshake with NO guard. Then
lines 472-479 re-take the guard and write the new session. **Window:
between line 442 and line 472 a second attach can complete and
install its own session, then this attach overwrites it.** The
older session lives on with a leaked WebSocket connection.

**Fix:** Hold the guard across the entire attach lifecycle, or
serialize via a separate `attach_in_progress: AtomicBool` flag and
fast-fail the second concurrent attach with "attach already running".

When the auto-attach supervisor lands, the surface widens: any
heuristic that calls `tv_cdp_attach` from a background loop can race
the user's manual button. Recommendation: route both through a single
`AttachCoordinator` actor with a request channel — the supervisor and
the UI both `send(AttachIntent {...})` and the actor sequences them.

---

## 3. Tauri command surface audit — power tools

The LLM-controllable surface (from PR #4's `tool_allowlist`) is small:
`tv_chart_get_state`, `tv_chart_set_symbol`. The TS-controllable
surface (anything Tauri exposes to the renderer) is large.

### Commands that take arbitrary string args and reach external systems

| Command | LLM-reachable? | External effect | Argument validation |
|---------|---------------|------------------|---------------------|
| `tv_cdp_eval` | **NO via Tool**, **YES via renderer IPC** | runs JS in TV's logged-in renderer | none |
| `tv_cdp_set_symbol` | YES via Tool | switches TV chart | length-capped at 64 chars + JSON-encoded (good) |
| `tv_cdp_draw_sltp` | NO (not in allowlist) | draws on TV | f64 inputs, no clamp — `f64::NAN` would be JSON-encoded as `null`, snippet would no-op |
| `tv_cdp_launch_tv` | NO | spawns a process | searches well-known paths only — safe |
| `order_flow_tag_active_trade` | NO | mutates state | const-allowlist enforced ✓ |
| `order_flow_apply_preset` | NO | runs JS in TV | const-allowlist enforced ✓ |
| `compute_position_size` | NO | pure math | spec_id falls back to STOCK — safe |
| `lockout_set_config` | NO | writes lockout TOML | no validation on `cooldown_minutes` — a UI bug could set 0 |

**Critical finding:** `tv_cdp_eval` is the highest-power Tauri command
in the entire app. It runs arbitrary JavaScript inside TradingView's
logged-in renderer, where TV's broker integration UI lives. Today it
is reachable from:

- Any React component that calls `invoke('tv_cdp_eval', { expression })`.
  Nothing in the type system or the runtime enforces "only the TV
  bridge settings panel can call this."
- Once the in-TV overlay outbox bridge (PR #8/#9) lands, **any
  TV-page script can write to the outbox** and a permissive outbox
  drainer would forward to `tv_cdp_eval`. See section 4 below.

**Required change:** Add a **process-internal** allowlist inside
`tv_cdp_eval` itself that matches the `expression` against a const
slice of expected expressions (the JS snippets we control), and
return `Err("expression not in allowlist")` for anything else.
Today the snippets that need this are `JS_GET_CHART_STATE`,
`JS_SET_SYMBOL`, `JS_DRAW_SLTP`, `JS_CLEAR_SLTP`,
`JS_GET_ORDER_FLOW_STATE`. Match on a **hash** of the snippet so the
allowlist updates don't have to copy the full string.

Alternative: split `tv_cdp_eval` into a private `tv_cdp_eval_internal`
helper used by sibling commands, and remove it from the
`invoke_handler!` macro entirely. The renderer never needs to call
arbitrary JS — only specific commands.

---

## 4. The outbox bridge problem (PR #8/#9, forward-looking)

The planned in-TV overlay panel runs **inside the TV page** as an
injected DOM widget. Per the user's description, it writes commands
to `window.__WHISKEY_OVERLAY_OUTBOX` and a Rust poll drains them.

This is a **trust boundary violation in waiting**. The TV page is:

- Authenticated to TV with the user's login (read-write).
- Loaded with TV's first-party JavaScript, Pine indicator scripts
  (any of which can be user-installed third-party Pine), URL preview
  loaders, ad iframes, future TV features.
- Any of those can call `window.__WHISKEY_OVERLAY_OUTBOX.push(...)`.

**Threat model:** a malicious Pine indicator (TV publishes a public
script library) writes to the outbox to trigger `tv_cdp_eval` with
a crafted expression that exfiltrates the user's session cookie or
submits an order through TV's broker integration. The user did not
load the agent's overlay panel — they installed a Pine indicator.

**Required mitigations for PR #8/#9:**

1. **Allowlist outbox commands by exact match.** The outbox accepts
   only one of a small set of command names (`get_chart_state`,
   `tag_trade`, `apply_preset` — never `eval`).
2. **Nonce/secret per session.** On overlay injection, Rust gives
   the panel a random nonce; the panel includes it in every outbox
   message; Rust drops messages with a missing or wrong nonce.
   The nonce lives in a closure (capture in the inject script) so
   page-level scripts can't read `window.__WHISKEY_OVERLAY_NONCE`.
   Better: use `postMessage` to a registered MessagePort instead of
   a `window` global — only the injected script can hold the port
   reference, and there is no global to read.
3. **Never expose `tv_cdp_eval` through the outbox.** Document this
   as an architectural invariant in the bridge module's header.

---

## 5. Persistence layer audit

Four TOML files at `<openhuman_dir>/`:

| File | Writer | Atomic? | Defaults on parse fail? |
|------|--------|---------|--------------------------|
| `active_mode.toml` | `modes/persistence.rs:85` | **NO** (direct `fs::write`) | YES (returns `None`, registry falls back) |
| `tks_mods.toml` | `tks_mods_config.rs:134` | **NO** | YES (`TksModsConfig::default()`) |
| `tks_lockout.toml` | `lockout.rs:153` (inferred from grep) | **NO** | YES |
| `onboarding.toml` | `modes::onboarding::*` (PR #5) | **NO** | YES |

**Atomicity gap:** All four use `std::fs::write(&path, raw)` — if the
process crashes mid-write the file is left truncated or partially
written. The load path treats malformed TOML as "use defaults", which
is correct from a liveness perspective but **silently destroys user
preferences**.

**Fix pattern:** write to `path.with_extension("toml.tmp")`, then
`std::fs::rename(tmp, path)` (atomic on POSIX, near-atomic on Windows
via `MoveFileEx` with `MOVEFILE_REPLACE_EXISTING`). Build a single
helper `atomic_write(path, contents)` in `modes::persistence` and
use it from all four sites.

**Concurrent process gap:** Nothing in the load/save pair holds a
filesystem lock. Two app instances starting at the same time can
both `load()` defaults, both `save()`, and one of them wins. The
loser's user-set theme/lockout disappears. Mitigate with an advisory
lockfile (`active_mode.toml.lock`) using `fs2::FileExt::try_lock_exclusive`.

**Read-only home dir:** Today, `mkdir_all` failure is warn-logged
and swallowed (`persistence.rs:66-72`). That is the right liveness
choice, but the UI has no surfacing — the user toggles the theme,
sees it apply for the session, and on next launch it's gone with no
indication why. Surface "config could not be persisted" as a one-shot
toast.

---

## 6. The Tradovate API gap

Section 9 of `docs/ORDER_FLOW_RESEARCH.md` describes Tradovate-dependent
features (DOM ladder, iceberg detector, auto-tag-at-fill). None of
the merged code touches Tradovate. **Good** — the discipline held.

Sanity check on PR #6 for false-positive Tradovate claims:

- `OrderFlowCard.tsx` only refers to "TV bridge", "CDP live", or
  "manual entry" as data sources. No mention of Tradovate, DOM, or
  iceberg detection in the UI strings.
- `useOrderFlow.ts` only invokes `tv_cdp_get_order_flow_state` and
  the `order_flow_*` commands. No Tradovate WebSocket client.
- PR #7 `order_flow.rs::detect_absorption` operates on a single
  `BarDelta` passed in by the caller. The caller (the Tauri command
  `order_flow_record_bar`) accepts `bid_vol, ask_vol` as parameters
  from the frontend — these come from manual entry or from
  `tv_cdp_get_order_flow_state`'s CDP read. Neither is "real" L2 data,
  but neither is the UI claiming so. Acceptable.

**Caveat:** The "single_print" and "value_area_reject" tag chips
imply value-area data the bridge cannot reliably provide today. TV's
VPVR drawn as a native tool is not CDP-readable per the research doc
(section 2). The user tagging these manually is fine; the UI just
shouldn't promise auto-detection of them as Whiskey alerts. Today
it doesn't (alerts cover only delta_divergence, absorption,
single_print_rejection — and the last of those is in PR #6's TS
config but has no corresponding Rust detector function in PR #7).

---

## 7. WhiskeyMode allowlist completeness

`src/openhuman/modes/whiskey.rs:34-60` is well-curated. Post-PR-#4
the additions `tv_chart_get_state` and `tv_chart_set_symbol` are the
**only** TV-related tools exposed. **`tv_cdp_eval` is NOT in the
allowlist** — correct, must stay that way.

The allowlist scope is "tools the LLM can call". It does **not**
restrict Tauri IPC, which means an LLM that successfully escapes the
prompt could still issue a JS prompt-injection that asks the user to
paste a magic string into the chat... which the agent then could not
execute because `tv_cdp_eval` is not in `ALLOWED_TOOLS`. Good. The
defense-in-depth chain holds — for now.

**Open risk:** if PR #4's stubs are unstubbed by wiring through
`webview_apis`, the `TvSetSymbolTool::execute()` will eventually call
something that **calls** `tv_cdp_eval` Rust-side. That is fine — the
allowlist gate is at the **tool** level, not at the underlying primitive.

---

## 8. Test theatre vs. real coverage

Spot-checks on the new test files:

- **`order_flow.rs::tests`** (PR #7) — Genuine. The divergence
  detection tests construct multi-bar fixtures and assert
  bullish/bearish/no-signal across them. `detect_absorption` is
  tested with ATR=0 degenerate case. `tag_active_trade` is tested
  for unknown-tag rejection. Cumulative delta is tested with empty,
  single, and overflow-saturating inputs. **This is real coverage.**
- **`tv_chart.rs::tests`** (PR #4) — All tests verify the stub's
  error message contains "core_rpc bridge not yet wired". Acceptable
  because the tool **is** a stub; not theater because the assertion
  matches the contract. Add an integration test gated on
  `OPENHUMAN_WHISKEY_TV_LIVE=1` once the bridge wires.
- **`OrderFlowCard.test.tsx`** (PR #6) — Eight `it()` blocks, all
  happy-path. The "shows an error alert when order_flow_apply_preset
  returns an error" block (line ~263) does exercise the error path.
  However the mock at line 71 returns the contract-drifted shape
  (`{ ok, preset_id, indicators_added, error }`) that does not match
  PR #7's Rust return type. **The mock locks in the drift** — when
  PR #7's actual Rust runs, the response shape will be different and
  the UI's success path (which reads from local data) will still work
  but no test catches that the contract is wrong. Add a contract
  test that pins the shape Rust actually returns.

---

## 9. TV CDP fragility budget

Every JS snippet in `tradingview_cdp.rs` is one TV release away from
breaking. Audit of failure modes:

- `JS_GET_CHART_STATE` (line 288): wraps in try/catch, returns
  `null` per field on missing path. **Surfaces silently** to the
  caller — the UI's `state.indicator_count` becomes `None` and
  renders as a dash. **Fragility budget: OK.** But the `_probe`
  field is captured (`has_chartWidget`, `has_activeChart`, etc.) and
  never bubbled to the UI. Add a "TV bridge: degraded" indicator
  driven by `_probe`.
- `JS_SET_SYMBOL`: returns `{ ok: false, error }` on missing
  `setSymbol`. **Loud.** Good.
- `JS_DRAW_SLTP` (PR #2): each `drawLine` is wrapped in its own
  try/catch and returns `null`. The outer return reports `{ok:true,
  ids:{entry:null, stop:null, target:null}}` — **claims success
  while drawing nothing.** Failure mode discipline broken: a stale
  "SL/TP drawn" toast appears while the user sees no lines. **Fix:**
  if any of the three IDs is null, return `ok:false` with a specific
  error.
- `JS_GET_ORDER_FLOW_STATE` (PR #6): the `model.panes()._series.data.bars()`
  path is the deepest CDP introspection in the codebase. Every step
  is `?.` guarded, but on TV breakage every field comes back as
  `null` and the UI's `cdpLive` flag stays true (the snippet didn't
  throw). **Result: "CDP live" indicator shown, all fields blank.**
  Add a sentinel: if every order-flow field is null, treat as
  `cdpLive: false`.

---

## 10. Architectural debt — consolidation candidates

Two patterns are forking visibly:

### Three persistence styles

1. `modes::persistence::save/load` (private to module, env-var override
   per-file, returns `None` on parse fail).
2. `modes::tks_mods_config::save/load` (public, env-var override,
   returns defaults on parse fail).
3. `modes::lockout::save/load` (public, env-var override, returns
   defaults).

All three duplicate the same boilerplate (state_path resolver,
mkdir_all, toml::to_string_pretty, fs::write, warn-log on error).
**Consolidate into `modes::persistence::AtomicTomlStore<T>`** —
generic over `T: Serialize + Deserialize`, takes the file name
and an env var name. Eliminates 100+ lines of duplicated logic and
fixes atomicity in one place (#5 above).

### Three event channels

Today: `overlay::publish_attention` (broadcast Tokio channel), Tauri
IPC events (`AppHandle::emit`), `core_rpc` (the Tauri→core JSON-RPC),
and the planned outbox poll. Four mechanisms by which Rust talks to
the UI. Some unification is unavoidable (CDP must be async, overlay
is broadcast, outbox is poll-driven by trust constraints). But the
**core_rpc outbound vs. webview_apis** split — flagged in PR #4's
module-doc as the bridge gap that blocks TvChartStateTool — is a
real architectural fork that needs resolution before more tools land.
Pick one outbound channel from core→Tauri (I recommend webview_apis
since it already runs and has reconnect logic) and route all future
Whiskey tools through it.

---

## 11. The "Whiskey never executes trades" invariant

The covenant text is in `WHISKEY_SYSTEM_PREFIX` (`whiskey.rs:261-263`).
Code enforcement: **none**. The tool allowlist happens to not include
any trade-execution tool because no such tool exists. The CDP bridge
**can** execute trades — `tv_cdp_eval` against TV's broker UI could
synthesize button clicks.

This invariant lives or dies by:

1. **`tv_cdp_eval` never reaching the LLM** (today: true, must stay
   true — section 3).
2. **No future Tauri command being added that submits an order**
   (no static check today — add a `// WHISKEY-INVARIANT: this
   command must not place orders` comment policy and a CI grep).

**Recommendation:** Add a top-of-file comment in `tradingview_cdp.rs`
that says:

> WHISKEY-INVARIANT: No command in this module may submit an order,
> modify an order, or interact with TV's broker integration UI. The
> bridge is read+chart-state only. New commands MUST be reviewed
> against this invariant; the LLM cannot ask the user to bypass it.

Also: when the in-TV overlay panel ships, the click handlers in the
panel must be hardcoded to non-trade UI elements. A CI test that
greps the injected panel source for `submitOrder`, `placeOrder`,
`broker`, etc. would catch regressions.

---

## 12. Onboarding wizard ↔ TK's Mods route

`PR #5 DoneStep` (diff line 522-523):

```ts
// TODO: change to `/settings/tks-mods` once TK's Mods branch lands.
navigate('/settings/modes');
```

The `/settings/tks-mods` route exists on whiskey HEAD via
`TksModsPanel.tsx`. The TODO is stale. Update to
`navigate('/settings/tks-mods')` before merging PR #5. This is the
one-line UX fix the charter requested.

---

## Recommendation roll-up

### Do NOT ship until fixed

1. PR #6/#7 contract drift — block both PRs, pick PR #7 as
   authoritative, rewrite PR #6 TS types + command calls.
2. PR #5 onboarding `DoneStep` route → change to `/settings/tks-mods`.
3. `publish_attention` config disk-read on every event → cache it
   (mtime-keyed or change-event-invalidated).
4. `tv_cdp_eval` must validate `expression` against a const allowlist
   inside the command itself.
5. Add `WHISKEY-INVARIANT: never executes orders` comment to
   `tradingview_cdp.rs`.

### Ship now, fix in v2

6. Atomic-write helper for all four TOML persistence sites.
7. Lockfile to serialize two-app-instance startup.
8. `_probe` field surfacing in TV CDP get_chart_state → degraded
   indicator in UI.
9. `JS_DRAW_SLTP` partial-failure detection (don't return ok:true
   if any line is null).
10. `OrderFlowCard` should treat all-null order-flow fields as
    `cdpLive: false`.
11. Consolidate `persistence.rs` + `tks_mods_config.rs` +
    `lockout.rs` save/load boilerplate.
12. Resolve the core_rpc vs. webview_apis outbound channel fork
    before TvChartStateTool is unstubbed.
13. Pre-PR-#8/#9: design the outbox nonce/MessagePort scheme before
    that code lands, not after.

---

_End of review. Cross-referenced files: `src/openhuman/modes/whiskey.rs`,
`risk_sanitizer.rs`, `persistence.rs`, `tks_mods_config.rs`,
`lockout.rs`, `position_sizer.rs`, `order_flow.rs`,
`src/openhuman/overlay/bus.rs`, `app/src-tauri/src/lib.rs`,
`tradingview_cdp.rs`, PR-#4..#7 diffs, `docs/ORDER_FLOW_RESEARCH.md`,
`WHISKEY_AUDIT.md`._
