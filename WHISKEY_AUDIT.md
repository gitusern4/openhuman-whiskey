# Whiskey fork audit — 2026-05-11

## Scope

- Branch: `whiskey` at `33ebce69` (HEAD; one new commit landed mid-audit)
- Upstream baseline: `upstream/main` at `64e57e74`
- Commits audited: 24 (`git log --oneline upstream/main..whiskey`)
- Files inspected: ~40 of the 146 changed (focused on Whiskey-specific
  modules; upstream-merge churn skimmed only)
- Vitest suite: 1994 passed, 3 skipped (217 files), 250s wall — all green
- `cargo test` not run (tauri-cef vendored submodule unavailable locally,
  per instructions)
- Audit duration: ~50 minutes

## Severity ranking (read first)

1. **CRITICAL** — Whiskey persona prefix + persona-memory cache are not
   injected on agent-loop turns (the production hot path). They only run
   on `chat_with_system`, which the tool loop bypasses.
2. **CRITICAL** — Whiskey allowlist names `"memory_tree"` but no tool
   registers under that name. Memory Tree is silently disabled in
   Whiskey mode despite being explicit in the allowlist.
3. **HIGH** — `image_gen_pollinations` accepts a fully user/LLM-controlled
   `save_path`; arbitrary write to any path the process can reach.
4. **HIGH** — `WindowsMascotApp.tsx` calls `mascot_window_save_position`
   on every `tauri://move` event. With no throttling, dragging fires
   dozens of disk writes per second.
5. **HIGH** — `Mode::session_memory_write_path()` and
   `Mode::overlay_source()` are dead code outside tests; `WHISKEY.md`'s
   own end-to-end-flow diagram references behaviour neither produces.
6. **HIGH** — `Mode::additional_memory_roots()` doc claim that it feeds
   `memory::ingestion` is false. Only `modes::memory_cache::resolve` reads
   it (and only on the `chat_with_system` path — see CRITICAL #1).
7. **HIGH** — `WHISKEY_HEARTBEAT_PROMPT` instructs the model to read
   "screen_intelligence snapshots" — `screen_intelligence::windows::engine`
   and `capture` are explicit STUBS that return `NotImplementedYet`. The
   prompt invites confabulation.
8. **MEDIUM** — `mascot_summon_hotkey::register` rollback path can leave
   on-disk state inconsistent with the actually-registered shortcuts on
   partial failure. State Vec is updated only on full success.
9. **MEDIUM** — Every `state.0.lock().unwrap()` in
   `mascot_summon_hotkey.rs` (9 sites) panics on poison. Same Tauri
   command can never recover.
10. **MEDIUM** — `pub mod windows` in
    `screen_intelligence/mod.rs` exposes the entire scaffold as crate-public
    while every sibling module is private.
11. **MEDIUM** — The `register` Tauri command holds the mutex across no
    IO, but `unregister_all` calls `app.global_shortcut().unregister()`
    *after* dropping the guard, while `register` calls `install_handler`
    while holding the guard (in `register_default`). Inconsistent
    discipline; see findings.
12. **LOW** — Several "smoke" tests (`whiskey_mode_resolve_is_callable_without_panicking`,
    `engine_config_defaults_match_research_recommendations`, etc.) only
    assert "doesn't panic" / "constants equal constants" — coverage
    theatre.
13. **LOW** — `WHISKEY.md` says "PR #1 against upstream, 17/17 PR checks
    green on the head commit `620573a5`" — head is now `33ebce69`.
14. **LOW** — `image_gen_tool::permission_level()` returns `Write`; the
    tool also makes outbound network requests. The permission system
    has no `Network` variant, but the doc string mistakenly says
    "Network-only call, no host writes other than under workspace's
    generated_images subdir" — and that's only true if `save_path` is
    NOT set (see HIGH #3).

## Findings (severity-ordered)

### CRITICAL

#### C1 — Whiskey persona + memory cache silently bypassed by agent loop

- **Files**:
  - `src/openhuman/providers/router.rs:122-159` (only `chat_with_system`
    assembles the persona/memory)
  - `src/openhuman/providers/router.rs:174-183` (`chat` just delegates,
    no persona injection)
  - `src/openhuman/agent/harness/tool_loop.rs:320-330` (agent loop calls
    `provider.chat(...)`, NOT `chat_with_system`)
- **What's broken**: `RouterProvider::chat_with_system` is the only path
  that calls `assemble_system_prompt(prefix, memory, caller_system_prompt)`.
  Every other Provider trait method (`chat`, `chat_with_history`,
  `chat_with_tools`) just resolves the route and delegates straight to
  the inner provider with no Whiskey context added. The agent's tool
  loop in `tool_loop.rs::run_tool_call_loop` calls `provider.chat(...)`
  on every iteration. Result: the Whiskey persona prefix and the persona
  memory block (whiskey_playbook.md, covenant, pattern_log, etc.) are
  **never seen by the LLM** during normal interactive agent turns.
- **Why it matters**: This is the headline feature of the fork. The
  end-to-end flow diagrammed in `WHISKEY.md` lines 22–60 shows persona
  injection on "Next user message arrives → RouterProvider::chat_with_system",
  but the production agent loop doesn't take that path. Whiskey mode is
  active on paper (registry flips, tool allowlist filters tools, mode
  pill renders) but the LLM gets stock OpenHuman prompts.
- **Fix**: Inject `assemble_system_prompt(...)` into the system message
  inside `RouterProvider::chat`, `chat_with_history`, and
  `chat_with_tools` — find the existing `role: "system"` `ChatMessage` in
  the message slice (or prepend one if absent) and replace its `content`
  with the assembled prompt before delegating. The memory_cache layer is
  cheap enough (mtime-keyed) to consult on every call.

#### C2 — Whiskey allowlist references nonexistent `memory_tree` tool

- **Files**:
  - `src/openhuman/modes/whiskey.rs:38` (`"memory_tree"` in allowlist)
  - `src/openhuman/tools/impl/memory/tree/drill_down.rs:13` (actual name
    is `memory_tree_drill_down`)
  - `src/openhuman/tools/impl/memory/tree/fetch_leaves.rs:19` (actual
    name is `memory_tree_fetch_leaves`)
- **What's broken**: `WhiskeyMode::ALLOWED_TOOLS` lists `"memory_tree"`,
  but no tool registers under that exact name. The construction-time
  filter (`filter_tools_by_active_mode`) and the per-dispatch check
  (`is_tool_allowed_in_active_mode`) both use exact string equality, so
  the Memory Tree tools are silently excluded whenever Whiskey mode is
  active.
- **Why it matters**: Whiskey is supposed to read the user's curated
  Memory Tree (the system that distills cross-session learnings — exactly
  what a trading mentor wants). Today Whiskey can recall and store but
  can't drill down or fetch leaves.
- **Fix**: Replace `"memory_tree"` with `"memory_tree_drill_down"` and
  `"memory_tree_fetch_leaves"` in `ALLOWED_TOOLS`. The existing test
  (`whiskey_mode_tool_allowlist_excludes_shell`) only asserts
  "starts with memory_" so it passed; add an integration-style test that
  fails when an allowlisted name isn't a real tool.

### HIGH

#### H1 — Pollinations tool accepts arbitrary `save_path` from LLM args

- **File**: `src/openhuman/tools/whiskey/image_gen_pollinations.rs:60-63,
  129-136`
- **What's broken**: `ImageGenRequest::save_path` is fully user/LLM
  controlled. `generate(...)` writes the downloaded bytes verbatim with
  `tokio::fs::write(&saved_path, &bytes)`. There's no path canonicalisation,
  no check that `save_path` stays under the workspace, no rejection of
  absolute paths or `..` segments, and no extension allowlist.
- **Why it matters**: The image-gen tool is in Whiskey's allowlist. A
  prompt-injection in the user's playbook (or just the LLM hallucinating
  path traversal) can write arbitrary bytes anywhere the process can
  reach — including overwriting the user's
  `whiskey_playbook.md`, `active_mode.toml`, or shell init files. The
  fact that the body is a PNG isn't a safety property because the model
  controls the *prompt* that becomes the body.
- **Fix**: When `save_path` is `Some`, validate that the canonicalised
  path is a descendant of the configured `default_save_dir`. Reject
  absolute paths, paths containing `..`, and any non-`.png` extension.
  Or simpler: drop the `save_path` field entirely — the tool is more
  predictable with a single managed sink.

#### H2 — Mascot drag fires `mascot_window_save_position` on every move event

- **File**: `app/src/mascot/WindowsMascotApp.tsx:54-70`
- **What's broken**: `tauri://move` fires on every position change during
  a drag (typically tens to hundreds per second on modern displays). The
  callback invokes `mascot_window_save_position`, which routes to
  `mascot_windows_window::save_current_position`, which calls
  `mascot_windows_state::save_state`, which writes
  `mascot_windows_state.toml` to disk. No throttling, no debounce.
- **Why it matters**: Dragging the mascot generates a hot disk-write
  loop. On HDD-backed machines this is visible. On SSDs it's a
  measurable wear-amplification path. Plus, every `serde::to_string +
  fs::write` blocks the JS event loop briefly.
- **Fix**: Wrap `savePosition` in a trailing-edge debounce (300ms
  works), or move the save to a `tauri://moved` (drag-end) event if the
  Tauri version exposes one. Either is < 10 LOC.

#### H3 — `session_memory_write_path` + `overlay_source` are dead code

- **Files**:
  - `src/openhuman/modes/mod.rs:88-91, 102-104` (defined)
  - `src/openhuman/modes/whiskey.rs:135-137, 143-145` (overridden)
  - `src/openhuman/modes/default.rs:54` (test)
  - **No production caller anywhere in `src/`**
- **What's broken**: Both methods are part of the `Mode` trait surface,
  Whiskey overrides them, tests exercise them, but no consumer ever
  reads them outside tests. The doc comment on `mod.rs` line 88 says
  "Optional path the mode wants to *write* session-scoped memories
  into..." — no code does that. WHISKEY.md acknowledges this for
  session_memory_write_path (line 202–207); overlay_source has no such
  caveat.
- **Why it matters**: Misleading API surface. A future contributor will
  add a Mode that overrides `overlay_source` and silently get nothing.
  Either wire them or delete.
- **Fix**: Either land a real consumer (the upstream
  `overlay::publish_attention` should use `mode.overlay_source()`; the
  memory store's session-log path should consult
  `mode.session_memory_write_path()`), or remove the methods and the
  Whiskey overrides until you do.

#### H4 — Doc claim "memory::ingestion consumes additional_memory_roots" is false

- **Files**:
  - `src/openhuman/modes/mod.rs:23-24` (the false claim)
  - `src/openhuman/modes/memory_cache.rs:73-113` (the only real consumer)
  - `src/openhuman/providers/router.rs:144` (only call site)
- **What's broken**: The trait doc says `additional_memory_roots()` is
  consumed by `memory::ingestion`. It isn't. The only consumer is
  `memory_cache::resolve`, called from `RouterProvider::chat_with_system`
  — and per CRITICAL #1, that path is bypassed by the agent loop.
- **Why it matters**: Compounds CRITICAL #1. A reader of
  `WHISKEY.md` and the trait doc reasonably believes the Whiskey memory
  files are folded into the global Memory Tree on boot ingestion. They
  aren't. They're prefix-pasted into a system prompt that the agent loop
  doesn't see.
- **Fix**: Update the doc to match reality OR (better) add a hook in
  `memory::ingestion::start` (whatever the boot sweep entrypoint is
  called) that pulls `active_mode().additional_memory_roots()` and folds
  them into the same `.md` ingestion path used for the upstream vault.

#### H5 — Heartbeat prompt instructs model to read screen_intelligence stubs

- **Files**:
  - `src/openhuman/modes/whiskey.rs:282-292` (`WHISKEY_HEARTBEAT_PROMPT`)
  - `src/openhuman/screen_intelligence/windows/engine.rs:85-87`
    (`start(...)` returns `Err(EngineError::NotImplementedYet)`)
  - `src/openhuman/screen_intelligence/windows/capture.rs:36-50` (capture
    is a stub unit-struct + an `enumerate_trading_windows` that returns
    `Vec::new()`)
- **What's broken**: The heartbeat prompt opens with "Scan recent
  screen_intelligence snapshots and Memory Tree updates for: 1. New trades
  the screen-watch detected..." — but the screen-watch engine is hard-wired
  to never start. There's no producer for the snapshots the prompt asks the
  model to inspect.
- **Why it matters**: LLMs faced with "scan X for Y" and given no X
  reliably hallucinate Y. This will produce confident reports of
  un-logged trades that never happened, polluting the user's
  whiskey_playbook.md if the agent acts on its own output.
- **Fix**: Either land a real engine (a known multi-day project per
  WHISKEY.md §11) OR rewrite the heartbeat prompt to drop the
  screen_intelligence references until the engine ships.

### MEDIUM

#### M1 — Hotkey re-register rollback path can leak inconsistent state

- **File**: `app/src-tauri/src/mascot_summon_hotkey.rs:99-169`
- **What's broken**: The `register()` flow:
  1. Snapshot old shortcuts
  2. Unregister each old one (rollback restores those already unregistered
     on failure here)
  3. Install handler for each new one (rollback unregisters those
     already installed AND tries to restore old ones on failure here)
  4. Update state Vec to expanded_shortcuts on full success only
  Two issues: (a) rollback in step 3 *re-installs old handlers* but the
  state Vec was last updated at step 0, so on a partial failure the
  in-memory state still says "old shortcuts" even though those handlers
  may or may not have been restored. (b) `install_handler` failures
  during rollback are only logged at warn — the user can end up with no
  working hotkey and no surfaced error.
- **Fix**: After a rollback, set the state Vec to whatever shortcuts
  actually have live handlers. Best approach: track a single
  `currently_installed: Vec<String>` and update it after every successful
  install/unregister, so it always matches reality.

#### M2 — `lock().unwrap()` in production hotkey paths

- **File**: `app/src-tauri/src/mascot_summon_hotkey.rs:76, 104, 160, 176`
- **What's broken**: 4 production sites + 5 test sites all use
  `state.0.lock().unwrap()`. If any path inside the closure passed to
  `on_shortcut` panics while holding the guard, the mutex poisons and
  every subsequent `register`/`unregister`/boot call panics. Tauri
  command panics aren't friendly.
- **Why it matters**: The dictation hotkey almost certainly has the
  same pattern (so it's "consistent with precedent"), but precedent
  isn't safety. A poisoned mutex bricks the hotkey UI permanently
  until process restart.
- **Fix**: `lock().unwrap_or_else(|p| p.into_inner())` everywhere — same
  pattern used by the test code in `user_filter.rs:161` and
  `registry.rs:187`.

#### M3 — `pub mod windows` exposes scaffold across the crate

- **File**: `src/openhuman/screen_intelligence/mod.rs:23-24`
- **What's broken**: Every other submodule in `screen_intelligence/`
  (`capture`, `engine`, `helpers`, `image_processing`, `input`, `limits`,
  `permissions`, `processing_worker`, `state`, `types`, `vision`) is
  private with `pub use` selectivity. The Whiskey-fork `windows` module
  is `pub`, exposing `Frame`, `TradingEvent`, `Roi`, `Anchor`, `Rect`,
  `OcrError`, `EngineError`, `EngineConfig`, etc. as crate-public.
- **Why it matters**: This locks in a public API for code that's
  explicitly STUBS and "will land in a follow-up commit." Future churn
  will be visible to any binary that consumes openhuman_core.
- **Fix**: `pub(crate) mod windows`, then `pub use windows::{
  TradingEvent, subscribe_trading_events, publish_trading_event }` if
  external consumers need them. Currently nothing outside the module
  references them anyway (the only callers are tests inside `mod.rs`).

#### M4 — Mutex held across plugin call in `register_default`

- **File**: `app/src-tauri/src/mascot_summon_hotkey.rs:75-89`
- **What's broken**: `register_default` takes `state.0.lock().unwrap()`
  on line 76 then iterates calling `install_handler(app, ...)` on line
  79 — `install_handler` calls `app.global_shortcut().on_shortcut(...)`
  which talks to a Tauri plugin. The lock is held across that call.
  By contrast `unregister_all` (line 173) drops the guard before
  calling unregister. Mixed discipline.
- **Why it matters**: If any other thread is also trying to read
  `MascotSummonHotkeyState` (e.g. the in-flight UI rebind), it blocks
  on plugin IO. Not a deadlock today (no second reader path exists),
  but a footgun the moment one is added.
- **Fix**: Mirror `unregister_all`'s pattern — install all handlers
  first, lock + push at the end.

#### M5 — Session-restoration test `set_active_mode_persists_to_disk` mutates global env var without sibling-test guard

- **File**: `src/openhuman/modes/registry.rs:227-238`
- **What's broken**: The test relies on `TEST_LOCK` to serialise — fine
  — but the env-var override `OPENHUMAN_ACTIVE_MODE_FILE` is global
  process state. If `cargo test` is invoked with a non-default thread
  count and another module reads the persistence path concurrently
  outside the lock (e.g. an unrelated module accidentally calling
  `persistence::load()`), the test silently writes to the user's real
  `~/.openhuman/active_mode.toml`. The PersistenceRedirect guard sets
  the var and clears it on drop, so a panic between set + drop also
  leaks the override into the next test.
- **Fix**: Use `tempfile`'s `with_env` patterns or pass the path as a
  function arg threaded through registry::ACTIVE init. Acceptable as-is
  for now, but flag it.

#### M6 — `whiskey.rs` env-var resolution test mutates global state

- **File**: `src/openhuman/modes/whiskey.rs:401-485`
- **What's broken**: `EnvVarGuard` sets `OPENHUMAN_WHISKEY_MEMORY_ROOT`
  globally. Tests in this module don't share a `TEST_LOCK` with
  registry/persistence's lock. If `cargo test` runs whiskey tests
  concurrently with registry tests, race possible.
- **Fix**: Add a single shared crate-level test lock for "anything that
  touches env vars".

#### M7 — `subscribe_trading_events`/`publish_trading_event` have no production consumer

- **File**: `src/openhuman/screen_intelligence/windows/mod.rs:67-78`
- **What's broken**: Public broadcast bus with zero downstream
  subscriber. The two tests in this file are the only exercises. Per
  WHISKEY.md §12 a "Whiskey-trader hookup" is supposed to subscribe;
  that module doesn't exist yet.
- **Why it matters**: Channel capacity 128 + a static `Lazy` sender =
  permanent allocation for a channel that nothing reads from. Cheap,
  but dead.
- **Fix**: Defer the bus until the first consumer lands.

#### M8 — Mascot summon hotkey card lives inside `ModesPanel`

- **File**: `app/src/components/settings/panels/ModesPanel.tsx:39-44, 210-252`
- **What's broken**: The "summon hotkey" card lives inside the Modes
  panel, which is reasonable UX but conflates two state machines. The
  `error` setter is shared between mode switching and hotkey saves —
  rendering one error clobbers a previous unrelated one. Also no
  client-side validation of hotkey strings before invoking
  `register_mascot_summon_hotkey` (Tauri side validates, but the user
  has to wait for the round-trip).
- **Fix**: Either move to its own `MascotPanel`, or split the error
  state into `modeError` + `hotkeyError`. Five-line change.

### LOW

#### L1 — Coverage-only smoke tests

Tests that assert "doesn't panic" or "constants equal constants",
contributing to the test count without verifying behaviour:

- `src/openhuman/modes/memory_cache.rs:389-395` —
  `whiskey_mode_resolve_is_callable_without_panicking` discards the
  result and asserts nothing.
- `src/openhuman/screen_intelligence/windows/engine.rs:93-99` —
  `engine_config_defaults_match_research_recommendations` asserts the
  default constants equal hard-coded numbers (rename or merge).
- `src/openhuman/screen_intelligence/windows/engine.rs:101-105` —
  `start_returns_not_implemented_in_stub` asserts a stub is a stub. Will
  be deleted-or-rewritten the moment the real engine lands.
- `app/src-tauri/src/mascot_windows_window.rs:228-244` — both tests
  assert constants equal constants.
- `app/src-tauri/src/mascot_summon_hotkey.rs:245-249, 273-302` —
  several tests construct a fresh `MascotSummonHotkeyState` and exercise
  Vec push/clear; you're testing `Mutex<Vec<String>>`, not your code.

These are fine in moderation but should be flagged so coverage metrics
aren't read as behaviour-coverage.

#### L2 — `WHISKEY.md` head-commit + "17/17 green" claim is stale

- **File**: `WHISKEY.md:11-12`
- WHISKEY.md says head is `620573a5`. Real head: `33ebce69`. Six
  Whiskey commits land after that (mode persistence, config-driven
  memory root, per-call tool allowlist enforcement, customizable
  hotkey UI). The "What's done" / "What's left" sections have likewise
  moved on — items the doc still calls Phase 2 (persistent active mode,
  hotkey customisation) are now in `main` of the branch. Refresh once
  the in-flight agents finish.

#### L3 — `image_gen_tool` doc on `permission_level` overstates safety

- **File**: `src/openhuman/tools/whiskey/image_gen_tool.rs:132-136`
- The comment claims "no host writes other than under the workspace's
  generated_images subdir." That's only true if the args don't supply
  `save_path` — see HIGH #1. Either fix the doc or fix the tool.

#### L4 — Mascot transparency check doc says "Phase-2 fix" but no issue link

- **File**: `app/src-tauri/src/mascot_windows_window.rs:7-14`
- Module doc enumerates a known transparency limitation and mentions a
  "Phase-2 fix" without a tracking issue. Add an inline `// TODO(#xxx)`
  so it doesn't get lost.

#### L5 — Resolution-chain test #4 silently passes when nothing exists

- **File**: `src/openhuman/modes/whiskey.rs:487-503`
- `resolution_fallback_returns_openhuman_path_even_when_missing` only
  asserts the resolved path is non-empty and has a leaf. It doesn't
  pin the leaf to `whiskey_memory` — a future refactor that returns
  `~/.openhuman` (no subdir) would still pass.

#### L6 — `info!` on every successful hotkey press

- **File**: `app/src-tauri/src/mascot_summon_hotkey.rs:51`
- `log::info!("[mascot-hotkey] {binding_for_log:?} pressed — toggling
  mascot visibility");` — info-level on a per-key-press path. Probably
  fine because the binding requires Shift+Space, but in dev where the
  user might map it to single keys, it's spammy. Demote to `debug!`.

#### L7 — `subconscious::prompt::build_evaluation_prompt` matches a
2-tuple where 1 arm uses neither — mild code smell

- **File**: `src/openhuman/subconscious/prompt.rs:38-41`
- The `match (mode.heartbeat_prompt_override(), upstream_preamble)`
  with `(Some(custom), _)` and `(None, upstream)` is harder to read
  than `mode.heartbeat_prompt_override().map(str::to_owned).unwrap_or_else(|| upstream_preamble.to_string())`.
  Cosmetic.

#### L8 — Hotkey UI lets user save an empty trim'd string before Tauri rejects

- **File**: `app/src/components/settings/panels/ModesPanel.tsx:88-91`
- Already handled (rejects on `next.length === 0`), but the `Save`
  button is disabled only on `hotkeyDraft.trim().length === 0`. A user
  who pastes an obviously-bad shortcut (e.g. `"asdf"`) gets a
  round-trip-then-error UX. Trivial polish.

#### L9 — `mascot_summon_hotkey` import-style inconsistency

- **File**: `app/src-tauri/src/mascot_summon_hotkey.rs:241`
- `use super::{MascotSummonHotkeyState, DEFAULT_MASCOT_SUMMON_BINDING};`
  in test mod orders the type before the const; project convention
  elsewhere is alphabetical or const-before-type. Cosmetic.

## What's well done

- **`tool_loop.rs:620-656` per-dispatch allowlist enforcement** — the
  "the registry-time filter goes stale on mode switch" reasoning in the
  comment is correct and rare to see articulated. The implementation is
  clean and the rejection ToolResult plumbs through the existing
  early-exit completion path.
- **`modes/persistence.rs`** — exemplary best-effort persistence layer.
  Save and load both swallow every error class with a warn-log; `load`
  returns `None` for missing/corrupt. The `TEST_OVERRIDE_ENV` +
  `TEST_LOCK` pattern is reusable. Tests actually exercise the
  malformed-TOML and missing-file branches.
- **`modes/memory_cache.rs`** — correct mtime-based caching, drop-lock-
  before-IO discipline, UTF-8-safe truncation, deterministic
  alphabetical file ordering for cache-friendly prompt prefixes. The
  `MAX_PER_FILE_BYTES` / `SKIP_FILES_OVER_BYTES` / `MAX_TOTAL_BYTES`
  cap stack is well-thought-out.
- **`assemble_system_prompt` (router.rs:17-31)** — pure function with
  4 unit tests covering all the segment-omission branches. Easy to
  extend.
- **`mascot_summon_hotkey::install_handler`** — refactor that pulled
  the handler-install logic out of the for-loops in `register_default`
  / `register` made the rollback flow tractable. Right call.

## Test coverage gaps

- **No integration test of the agent-loop persona injection.** A test
  that builds a real RouterProvider, switches to Whiskey, runs an agent
  loop turn against a mock provider, and asserts the system message
  contains `WHISKEY_SYSTEM_PREFIX`. This would have caught CRITICAL #1.
- **No test that walks the registered tool list and verifies every
  Whiskey allowlist entry corresponds to a real `Tool::name()`.** This
  would have caught CRITICAL #2. ~10 LOC.
- **No test of `mascot_window_save_position` throttling.** Easy: a
  vitest test that fires `tauri://move` 50 times within 100ms and
  asserts `invoke` was called ≤ N times.
- **`WHISKEY_HEARTBEAT_PROMPT` text isn't tested for "doesn't reference
  features that don't exist."** A simple assertion that the prompt only
  mentions features whose engines return `Ok(...)` would catch HIGH #5.
- **No test for the `image_gen_pollinations` save_path traversal
  rejection** (because there's no rejection — see HIGH #1).
- **`screen_intelligence::windows::ocr` tests use `.unwrap()` on results
  that depend on test-data shape** (line 236, 271). If the test data
  changes the panic message will be useless; prefer `.expect("...")`.

## Architectural inconsistencies

- **Two persistence patterns side-by-side.** `mascot_windows_state.rs`
  uses `cef_profile::default_root_openhuman_dir`. `modes/persistence.rs`
  uses `crate::openhuman::config::default_root_openhuman_dir`. Both
  resolve to roughly the same place but via different subsystems. A
  shared "openhuman_dir()" helper would be cleaner.
- **Mutex-poison handling.** Three patterns coexist: `lock().unwrap()`
  in mascot_summon_hotkey production code, `lock().unwrap_or_else(|p|
  p.into_inner())` in user_filter / registry tests, and `lock().ok()?`
  in memory_cache. Pick one (the unwrap_or_else recovery pattern) and
  apply uniformly.
- **System-prompt assembly happens at TWO layers.** The agent harness
  builds a `ChatMessage::system` upstream and the router would build
  one in `assemble_system_prompt`. Today only the harness's wins
  because the router's only fires on `chat_with_system`. Either pick
  one assembly point or document the precedence rule.
- **Tauri command surface conflates "persona switching" with "mascot
  hotkey rebinding".** The `ModesPanel.tsx` UI hosts both; the
  `lib.rs` `invoke_handler` macro lists them adjacent. Functionally
  unrelated — the hotkey doesn't care about the active mode and vice
  versa. Splitting would make future Whiskey-only persona settings
  easier to add without polluting the global panel.

## Recommended next-step priorities (4-hour budget)

1. **Fix CRITICAL #1 (persona/memory injection on agent-loop turns).**
   ~1 hr. The fix is to have `RouterProvider::chat`, `chat_with_history`,
   `chat_with_tools` find/replace/prepend the system message with
   `assemble_system_prompt(prefix, memory, existing_system_content)`.
   Then add the integration test from "Test coverage gaps" #1 so this
   never regresses.

2. **Fix CRITICAL #2 (memory_tree allowlist names).** ~5 min. Change
   `"memory_tree"` to `"memory_tree_drill_down"` and
   `"memory_tree_fetch_leaves"` in `whiskey.rs:38`. Add the
   "every allowlist name maps to a real tool" test (~30 min) so future
   tools can't quietly disappear from Whiskey mode.

3. **Fix HIGH #1 (image_gen save_path traversal).** ~30 min. Either
   delete the field or canonicalise + ancestor-check.

4. **Fix HIGH #2 (mascot save throttling).** ~15 min. Trailing-edge
   debounce on `savePosition`.

5. **Update WHISKEY.md** to reflect actual head, actual end-to-end
   flow (including the agent-loop bypass acknowledged as "not yet
   wired"), and accurate state of the Phase-2 items. ~30 min.

Total: ~3 hours. The remaining hour buys you HIGH #3 (decide whether
to wire or delete `session_memory_write_path` / `overlay_source`) — a
yes/no call worth making before any more code piles on top.
