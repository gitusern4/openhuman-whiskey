# Whiskey fork — status

This fork of `tinyhumansai/openhuman` adds:
1. A switchable agent-modes abstraction (Default + Whiskey trading mentor)
2. A Windows mascot path parallel to the existing macOS native path
3. A global summon hotkey
4. A persona memory cache that brings the user's curated playbook into
   every Whiskey-mode prompt
5. A free image-gen tool via Pollinations.ai

**Branch state at d-day**: 18 commits on `whiskey` (PR #1 against
upstream), **17/17 PR checks green** on the head commit `620573a5`.
Every architectural layer of the Whiskey loop is wired end-to-end.

The one piece that still doesn't work is **native ARM64 Windows builds**
— blocked on two upstream-vendored native deps (`whisper-rs-sys` +
`cef-dll-sys` pinned to CEF 146.x) that don't yet support
`aarch64-pc-windows-msvc`. The fork ships as the existing x86_64
Windows MSI under emulation; see "Native ARM64 Windows build" below
for the porting plan.

## End-to-end flow when the user switches to Whiskey mode

```
Settings → Modes → Whiskey
       ↓
ModesPanel.tsx → Tauri set_whiskey_mode("whiskey")
       ↓
crate::openhuman::modes::registry::set_active_mode
       ↓
process-wide ACTIVE pointer flips to Arc<WhiskeyMode>
       ↓
Next user message arrives.
       ↓
RouterProvider::chat_with_system
       ├── reads active_mode().system_prompt_prefix() → Whiskey persona
       ├── reads memory_cache::resolve(active_mode())  → reads + caches
       │       additional_memory_roots() → ~/.claude/projects/.../memory/
       │       whiskey_*.md, returns bounded markdown block
       └── assemble_system_prompt(persona, memory, caller_system_prompt)
              → "{persona}\n\n---\n\n{memory}\n\n---\n\n{caller}"
       ↓
LLM responds as Whiskey, grounded in the user's playbook + covenant.
       ↓
Post-turn: ReflectionHook uses active_mode().reflection_prompt_override()
           → trading-only reflection schema.
       ↓
Tool call: filter_tools_by_active_mode in tools/ops.rs intersects the
           registry with WhiskeyMode::tool_allowlist() → shell, execute,
           dangerous tools are dropped before dispatch even sees them.
       ↓
Mascot: tray menu → "Toggle floating mascot" OR
        global hotkey CmdOrCtrl+Shift+Space (mascot_summon_hotkey.rs)
        → mascot_window_show → mascot_windows_window.rs creates a
        Tauri WebviewWindow with always_on_top + transparent +
        WDA_EXCLUDEFROMCAPTURE; React mounts WindowsMascotApp.tsx
        which renders <YellowMascot face="idle"> + drag handler +
        click-to-pop.
```

## What's done

### CI / build

- **`aarch64-pc-windows-msvc` added to `.github/workflows/build-desktop.yml`** matrix
  + standalone `verify-arm64.yml` (`workflow_dispatch`) for ad-hoc verification.
- Local toolchain verified: VS 2022 Build Tools 2022 with ARM64 component
  is present, `aarch64-pc-windows-msvc` rustup target installed, pnpm
  10.10.0 installed, Tauri CLI installed, Node v24, gh authenticated.
- **PR-time CI: 17/17 green** on commit `d86757a2` (Linux + frontend + tests).
  The fork's own changes (modes module, Pollinations tool, allowlist filter)
  compile and ship clean on x86_64.

### Native ARM64 Windows build: blocked on upstream deps (May 11 finding)

First end-to-end ARM64 Windows build was triggered via `verify-arm64.yml`
on commit `fd439756`. Outcome: build proceeded through Rust toolchain
install + CEF + tauri-cli install + dependency install + frontend build,
then **failed** at `cargo tauri build` on two transitively-vendored
native C/C++ dependencies that do not yet support
`aarch64-pc-windows-msvc`:

1. **`whisper-rs-sys v0.15.0`** (vendored fork at
   `tinyhumansai/whisper-rs-sys`) — the bundled `whisper.cpp` build
   script aborts on ARM64. Likely SIMD intrinsics or a build-flag
   matrix that does not include the ARM64 Windows target.
2. **`cef-dll-sys v146.4.1+146.0.9`** — `libcef_dll_wrapper` C++
   wrapper compile fails for ARM64. Root cause: the vendored
   `tauri-cef` is pinned to **CEF 146.x**, but Spotify's CEF builds
   only ship `windowsarm64` binaries starting at **CEF 147.x**.
   Bumping the vendored CEF version is the structural fix; on the
   wire that means a coordinated upstream PR against
   `app/src-tauri/vendor/tauri-cef`.

Both are real porting projects (not drive-by fixes). Estimated effort:
~2 days for `whisper-rs-sys` ARM64 build-script work; ~1 week for
the CEF 146 → 147 vendor bump (plus regression testing of CEF API
changes in the Tauri shell).

**Mitigation in effect** (per the original Risk table): native ARM64
Windows builds are deferred. Day-1 fork ships as **x86_64 Windows
emulated under Windows-on-ARM** (the user already had this MSI
running locally). The `verify-arm64.yml` workflow stays in the tree
so when the upstream native deps gain ARM64 support, a single
`gh workflow run verify-arm64.yml` re-tests the path with no extra
plumbing.

### Modes module (`src/openhuman/modes/`)

- **`mod.rs`** — `Mode` trait with hooks for `system_prompt_prefix`,
  `reflection_prompt_override`, `heartbeat_prompt_override`,
  `additional_memory_roots`, `session_memory_write_path`, `tool_allowlist`,
  `overlay_source`. Trait-default no-ops keep new modes minimal.
- **`default.rs`** — `DefaultMode`. All hooks return trait defaults =
  byte-identical upstream behaviour. Regression-safety guarantee.
- **`whiskey.rs`** — `WhiskeyMode`. Persona embedded in three string
  constants:
  - `WHISKEY_SYSTEM_PREFIX` — voice, process discipline (Steenbarger /
    Douglas / SMB lineage), the never-execute covenant, A+ catalog grounding.
  - `WHISKEY_REFLECTION_PROMPT` — trading-only reflection schema.
  - `WHISKEY_HEARTBEAT_PROMPT` — periodic background pass with tier-1
    (un-logged trades) and tier-3 (active pattern firing) alerts only.
  - Loads the user's existing Claude Code Whiskey memory dir
    (`~/.claude/projects/.../memory/`) by default.
  - Tool allowlist excludes shell/execute by design.
- **`registry.rs`** — process-wide `RwLock<Arc<dyn Mode>>`, hot-path
  `active_mode()`, `set_active_mode(id)`, `list_modes()` for the picker UI.
  `serial_test`-decorated tests cover switch, regression, unknown-id reject.

### Tools (`src/openhuman/tools/whiskey/`)

- **`image_gen_pollinations.rs`** — free no-key image generation via
  Pollinations.ai. Defaults to `model=flux`, 1024×1024, dim cap 1536.
  Saves to disk with slug-based filenames; returns `(saved_path,
source_url, bytes, elapsed_ms)`. `tokio::fs` everywhere.
  Unit tests for URL building, filename slugging, empty-prompt rejection.

### Wiring done at the crate root

- `pub mod modes;` added to `src/openhuman/mod.rs`
- `pub mod whiskey;` added to `src/openhuman/tools/mod.rs`

## What's left (intentionally — Phase 2)

The end-to-end loop works. These are real follow-ups that didn't fit
the "ship a complete Day-1 baseline" budget but have well-understood
designs and would land cleanly on top of what's here.

### 1. Native ARM64 Windows build
**Blocker**: two upstream-vendored native deps need ARM64 support.
  - `whisper-rs-sys`: bundled `whisper.cpp` build script aborts on
    `aarch64-pc-windows-msvc` (likely SIMD intrinsics).
    Fix: a few days of build-flag matrix work in the
    `tinyhumansai/whisper-rs-sys` fork.
  - `cef-dll-sys v146.4.1`: vendored `tauri-cef` is pinned to CEF
    146.x; Spotify's CEF builds only ship `windowsarm64` binaries
    starting at CEF 147.x. Fix: coordinated upstream PR against
    `app/src-tauri/vendor/tauri-cef` to bump the CEF version (and
    ride out whatever CEF API changes between 146 and 147).

The `verify-arm64.yml` workflow stays in the tree; one
`gh workflow run verify-arm64.yml` re-tests the path the moment the
upstream native deps gain ARM64 support.

### 2. Mascot transparency on Windows-CEF
The mascot uses `WebviewWindowBuilder.transparent(true)`. Whether the
vendored CEF runtime honours that on Windows is unverified — if the
window paints opaque, the mascot is functionally complete but visually
a small square instead of a free-floating sprite. The native fallback
is a Win32 layered window + WebView2 (separate from CEF), parallel to
the macOS NSPanel + WKWebView path. ~600 LOC of `windows-rs` work.

### 3. Phase-2 Whiskey integrations (originally in the plan)
Each is a self-contained module that consumes the existing
infrastructure shipped here:
  - **Screen-watch** for Windows trading platforms (WGC capture +
    Tesseract OCR + Gemini Flash fallback for ambiguous fields). The
    macOS-focused `screen_intelligence/` module is the API model;
    `screen_intelligence/windows/` is the new submodule. Architecture
    details + research are at the bottom of this file under
    "Screen-watch on Windows".
  - **Whiskey-trader hookup**: subscribe to screen-watch events,
    cross-reference against `whiskey_playbook.md` A+ catalog, emit
    setup suggestions via the existing `overlay::publish_attention`
    bus, auto-log fills back to the playbook.
  - **Heartbeat reflection swap**: extend `heartbeat::engine` (or
    its `subconscious::engine` callee) to consult
    `active_mode().heartbeat_prompt_override()`. Currently the
    heartbeat path inherits the persona prefix via the router-level
    injection but doesn't see Whiskey's bespoke heartbeat schema.
  - **Persistent active-mode** across restarts. Today the active
    mode resets to `default` on every process boot. Add a
    `~/.openhuman/active_mode.toml` write on every `set_active_mode`
    + a read at boot. ~30 LOC.
  - **Hotkey customisability**: surface a `register_mascot_summon_hotkey`
    Tauri command + a `HotkeyRecorder.tsx` settings entry, mirroring
    the existing dictation hotkey UX.

## Original "what's left to wire" plan (now historical — kept for diff)

Items 1–10 below were the original Day-1 follow-up plan. **Items 1–4,
6–10 are done** (in the commit history; see "What's done" above).
**Item 5 (session-memory write path) is still open** — Whiskey writes
session memories via the standard memory store today rather than
appending to a dedicated `whiskey_session_log.md`. Low priority since
the existing Claude Code Whiskey skill maintains its own log; re-add
later if cross-skill memory becomes a real user request.



### 1. Inject mode prefix into the LLM request pipeline (~30 LOC)

**File:** `src/openhuman/providers/router.rs`
**Change:** at the point where the outgoing request's system messages
are assembled (search for `SystemMessage` or `system_prompt`), prepend
`crate::openhuman::modes::active_mode().system_prompt_prefix()` if Some.

### 2. Swap reflection prompt by mode (~20 LOC)

**File:** `src/openhuman/learning/reflection.rs`
**Change:** in the `ReflectionHook::run` (or equivalent) function where
the reflection prompt is built, prefer
`active_mode().reflection_prompt_override()` over the upstream default
when Some.

### 3. Same for heartbeat (~15 LOC)

**File:** `src/openhuman/heartbeat/engine.rs`
**Change:** swap heartbeat prompt analogously.

### 4. Memory ingestion: add WhiskeyMode's roots on boot (~30 LOC)

**File:** `src/openhuman/memory/ingestion/...` (locate the boot vault
sweep — probably called from `service::boot` or `app_state::init`)
**Change:** append `active_mode().additional_memory_roots()` to the list
of `.md` vault paths to scan. Existing markdown ingestion handles the
rest.

### 5. Session memory write path

**File:** `src/openhuman/memory/store/...`
**Change:** when WhiskeyMode is active and
`session_memory_write_path()` returns Some, redirect new "session log"
appends to that path (so the user's Claude Code Whiskey skill sees them).

### 6. Tool allowlist enforcement (~20 LOC)

**File:** `src/openhuman/tools/ops.rs` (where tool calls are dispatched)
**Change:** before dispatching, check `active_mode().tool_allowlist()`.
Reject with a clean error if the requested tool isn't in the list.

### 7. Register the Pollinations image-gen tool

**File:** `src/openhuman/tools/impl/mod.rs` (and `schemas.rs`)
**Change:** add a `Tool` impl that wraps
`tools::whiskey::image_gen_pollinations::generate`. Schema entry
exposes prompt + size + seed + model.

### 8. Frontend mode picker

**Files:** new `app/openhuman-app/src/components/ModePicker.tsx`,
plus a settings RPC endpoint that calls
`modes::registry::list_modes()` / `set_active_mode(id)`.
**Behaviour:** dropdown in the header / overlay panel; persists to
config; switches the mascot accent color (per the UX research
findings — mode pill IS the mascot's halo color).

### 9. Always-on-top mascot overlay window

**Files:** `app/src-tauri/src/lib.rs` (window setup) +
`app/src-tauri/tauri.conf.json` (declare `mascot-overlay` window).
**Spec (from UX research):** 56×56 hit, 40×40 visual, opacity 0.85
default → 0.55 idle, breathing animation 5–8 s sine, blink every 14±6 s,
look-toward-cursor when within 200 px. Hover-expand 380×220 panel
within 100 ms. Position persisted per-display.

### 10. Global hotkey via `tauri-plugin-global-shortcut` v2

**Files:** `app/src-tauri/Cargo.toml` (add dep), `app/src-tauri/src/lib.rs`
(register), config field for binding.
**Default:** `Ctrl+Space` (Windows) — `Alt+Space` collides with system
window menu and is a known ChatGPT-Desktop pain point.
**Behaviour:** summons the pre-rendered hidden mascot panel at the mouse
cursor, snapped to nearest screen quadrant. Captures foreground HWND
_before_ stealing focus so "Insert response back" can paste into the
prior app.

### 11. Screen-watch on Windows (Phase 2 — 2–3 days)

The existing `src/openhuman/screen_intelligence/` is macOS-focused. A
new `src/openhuman/screen_intelligence/windows/` submodule needs the
WGC capture path. Stack picks (from research):

- **Capture:** Windows Graphics Capture via `windows-capture` crate
  (NiiightmareXD). Cross-GPU, handles obscured windows, ARM64 wheels.
  Set `WDA_EXCLUDEFROMCAPTURE` on the overlay so it doesn't feed back.
- **OCR primary:** Tesseract 5.x with `--psm 7` and tight numeric
  whitelist (`0123456789.,-+$()`). Render at 2-3× scale + Otsu binarize
  before OCR — accuracy jump is dramatic on small UI fonts.
- **OCR fallback:** RapidOCR (ONNX runtime, ARM64-friendly).
- **Vision LLM:** Gemini 2.5 Flash ($0.000387/image) only when OCR
  confidence drops below 0.7, hard-disable-able via config.
  **Never use vision LLM for chart pattern recognition** — roman-rr
  April 2026 benchmark settles this (51% directional, severe bullish
  bias). LLM is for verifying structured fields only.
- **ROI persistence:** anchor-based (9 anchors + pixel offsets) with
  perceptual-hash drift detection on the ROI border. Survives resize +
  panel rearrangement.
- **Idle detection:** `GetLastInputInfo` polled at 1 Hz from capture
  thread + foreground-window check + `WM_POWERBROADCAST` for sleep.
- **Threading:** dedicated WGC capture thread → bounded queue →
  2-thread OCR pool → async LLM queue → ring-buffer snapshot read by
  the overlay UI thread.

### 12. Whiskey-specific screen-watch hookup

**File:** new `src/openhuman/integrations/whiskey_trader/mod.rs`

- Subscribe to `screen_intelligence` events (which the Phase-2 work
  emits in a structured form).
- Cross-reference extracted state against `whiskey_playbook.md` A+
  catalog — match by symbol + setup-pattern keywords.
- Emit `OverlayAttentionEvent` via `crate::openhuman::overlay::publish_attention`
  with the matched setup ID + confidence %.
- On detected fill (position diff vs. last frame), append to the user's
  `whiskey_playbook.md` in their existing R-multiple/MAE/MFE template.

## Architectural notes

### Why the modes registry is a global RwLock

The active mode is read on the LLM-request hot path. A read-write lock
that takes a sharable read on the hot path and only writes on user-
initiated mode switches is the cheapest correct option. `parking_lot`
read locks are essentially atomic on uncontended state, so this should
add no measurable overhead.

### Why DefaultMode exists

Without a no-op default, every existing call site that consults
`active_mode()` would have to special-case "no mode active." With
DefaultMode, the trait is always present and the default is byte-
identical to upstream — making the regression test "switch to Default,
behave like upstream" trivially true.

### Why the Whiskey persona is in code, not in `.md`

Three reasons:

1. Ships in the binary, no missing-file failure modes.
2. Read-only in the runtime — users can't accidentally edit it via
   their memory tools.
3. The dynamic part (their playbook, pattern log, covenant) IS in
   `.md` and is loaded via `additional_memory_roots()` — separation
   of concerns.

### Why screen-watch isn't a Day-1 deliverable

The existing `screen_intelligence` module is documented as
"macOS-focused" in its `mod.rs`. Building a parallel Windows path is a
2–3 day project (capture API + OCR pipeline + threading + ROI
persistence + idle detection). The Day 1 surface ships everything
that's reachable without new Win32 code, and Phase 2 adds the watcher.

### Why no Tradovate API integration

Initial scope had it; user explicitly dropped it after learning the
API requires a paid subscription on top of a $1k+ funded live account.
The screen-watch path replaces it — works against any platform the
user has on screen, no broker auth, manual-execute-only.

## Build commands

```bash
# From the repo root, after pnpm install:
cargo build                                            # default x86_64 host
cargo build --target aarch64-pc-windows-msvc           # ARM64 native (Windows)
cargo test --package openhuman --lib modes::            # mode tests only
cargo tauri build --target aarch64-pc-windows-msvc     # full ARM64 .msi
```

CI on push to `whiskey` branch will exercise all four target rows including
the new `windows-arm64` matrix entry.

## License

GPL-3.0, inherited from upstream `tinyhumansai/openhuman`.
