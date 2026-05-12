# QA Findings - Whiskey Fork - 2026-05-12

Auditor: QA-Expert agent
Branch: whiskey HEAD 61429b8d
Commits audited: 61429b8d 2b69ab6f 6cc56c91 fa0e652e 4ea8a0a0 aed51f20 7abb0573 d0471bd5
Audit date: 2026-05-12
Total findings: 12 (2 critical, 4 high, 4 medium, 2 low)

---

## CRITICAL

### F-C1 -- CI BROKEN: Type Check and Test both FAIL on HEAD 61429b8d

Severity: CRITICAL
CI checks: Type Check run 25755230084, Test run 25755230107
Status: Both conclusion=failure. PR Quality and Installer Smoke pass.

Type Check failure: app/src/test/whiskeyOverlay.smoke.test.ts:115
Error: TS6133 -- fn is declared but its value is never read.
Commit 61429b8d added const fn = new Function(wrapper)() at line 115.
Variable is immediately shadowed by inline formatLockoutUntil at line 117 and fn is never used. tsc exits 2.

Test failure: app/src/components/onboarding/__tests__/OnboardingWizard.test.tsx
Test: OnboardingWizard > calls tv_cdp_probe and tv_cdp_attach in order
Error: AssertionError: expected vi.fn() to be called with arguments: [ tv_cdp_probe ]
The probe mock expectation is violated. Likely PR8/PR5 interaction where probe moved into async flow mock does not await.

Fix:
1. Delete const fn at whiskeyOverlay.smoke.test.ts:115 -- dead code.
2. Fix OnboardingWizard.test.tsx: add mockResolvedValueOnce for tv_cdp_probe before wizard step fires.

Action required: Block merge of 61429b8d until both checks pass.

---

### F-C2 -- invalidate_tks_cache never called from any settings-save path

Severity: CRITICAL
Files:
- src/openhuman/overlay/bus.rs:25,60 -- function defined, comment says save-site must call it
- app/src-tauri/src/order_flow_commands.rs:79 -- tks_mods_config::save with no cache invalidation after
- Codebase-wide grep: zero external callers of invalidate_tks_cache

What is broken: User toggles hide_risk_pct and saves. TOML written correctly.
publish_attention reads from atomic cache (HIDE_RISK_PCT_CACHE at bus.rs:17) which holds pre-save value.
Risk percentages continue shown/hidden per old setting until process restart.

Note: FIX-C agent assigned. Expected-pending but active behavioral defect on HEAD.

Fix: After every tks_mods_config::save(), add crate::openhuman::overlay::bus::invalidate_tks_cache().
Confirmed missing call site: order_flow_commands.rs:79.

---

## HIGH

### F-H1 -- tv_cdp_eval IPC gate: zero tests exercise the rejection path

Severity: HIGH
File: app/src-tauri/src/tradingview_cdp.rs -- tests module lines 1198-1258

5 unit tests exist: all cover extract_tv_targets, pick_first_tv_target, DEFAULT_TV_CDP_PORT.
Zero tests cover tv_cdp_eval or its webview-label gate at line 522.

Missing tests:
1. Non-main label -- returns the specific rejection error string
2. main label -- delegates to tv_cdp_eval_internal

The label gate is the security boundary flagged in architect review 2026-05-12.
Without a regression test, a future refactor removing the gate has no automated catch.

Fix: Add two unit tests. Mock TvCdpState to avoid needing a live CDP connection.

---

### F-H2 -- filter_by_nonce has zero tests; all four required cases absent

Severity: HIGH
File: app/src-tauri/src/tv_overlay.rs -- tests module lines 464-589

10 tests present -- all parse/serialization smoke tests. Zero tests for filter_by_nonce (line 395).

Missing cases:
1. nonce-match: command with correct nonce passes through
2. nonce-mismatch: command with wrong nonce is dropped
3. missing-nonce: command with nonce:null is dropped
4. no-session-nonce (expected=None): ALL commands dropped (init guard)

filter_by_nonce is the sole defense against TV-page script forgery of outbox commands.
Function is pure -- no mocking needed, approx 20 lines of test code.

---

### F-H3 -- tv_cdp_eval_internal extraction: sibling routing verified OK; one doc gap

Severity: HIGH (doc gap only; code is correct)
File: app/src-tauri/src/tradingview_cdp.rs

All five sibling commands (tv_cdp_get_chart_state L574, tv_cdp_set_symbol L629,
tv_cdp_draw_sltp L911, tv_cdp_clear_sltp L943, tv_cdp_get_order_flow_state L1098)
correctly route through tv_cdp_eval_internal, not the webview-gated public tv_cdp_eval. Public API preserved.

Doc gap: tv_overlay.rs::dispatch_command (line 422) calls cdp_eval_raw directly.
Intentional (overlay loop holds raw session reference; TV-page scripts cannot call Tauri IPC)
but function doc does not say so. A future reader may question whether this is an oversight.

Fix: Add doc comment to dispatch_command explaining why cdp_eval_raw is used directly and why it is safe.

---

### F-H4 -- WHISKEY.md Phase-2 section lists items that have since shipped

Severity: HIGH (documentation accuracy)
File: WHISKEY.md:147-201

Stale claims:
- WHISKEY.md:195-198: Persistent active-mode listed as Phase 2. modes/persistence.rs exists and is tested. Shipped.
- WHISKEY.md:199-201: Hotkey customisability listed as Phase 2. mascot_summon_hotkey.rs + ModesPanel.tsx hotkey card exist. Shipped.
- WHISKEY.md:11-12: 17/17 PR checks green. As of HEAD 61429b8d, Type Check and Test are failing.
- WHISKEY_AUDIT.md header: Branch at 33ebce69. Real HEAD is 61429b8d.

Fix: Refresh Phase-2 list, update or remove CI green claim.

---

## MEDIUM

### F-M1 -- Lockout serde back-compat: verified OK; one missing explicit test

Severity: MEDIUM (verified clean with gap)
File: src/openhuman/modes/lockout.rs:104, 107-117

armed_for_reset_until: Option<u64> carries serde(default) at line 104.
Default impl at line 107 sets it to None. Back-compat confirmed.
Gap: round-trip test at line 445 does not cover TOML missing this field -> None.
A hand-crafted TOML string test would pin this explicitly.

---

### F-M2 -- lockout_reset Result return and arm-gate wiring verified; no end-to-end test

Severity: MEDIUM (verified clean with gap)
File: app/src-tauri/src/lib.rs:952-956

lockout_reset returns Result<LockoutStatus, String>. Gate routes through request_force_reset. Correct.
Gap: no test exercises lockout_reset -> request_force_reset via the Tauri command layer.
Missing: not-armed rejection, armed-but-cooldown-active rejection, success path.

---

### F-M3 -- DoneStep.tsx route target /settings/tks-mods verified present

Severity: MEDIUM (verified clean)
Files: app/src/pages/Settings.tsx:350, app/src/components/onboarding/steps/DoneStep.tsx:26

navigate to /settings/tks-mods in DoneStep.handleTksMods.
Settings.tsx registers path=tks-mods with TksModsPanel at line 350. Import at line 30. Route wired correctly.

---

### F-M4 -- Test theatre: whiskeyOverlay.smoke.test.ts is 100% happy-path

Severity: MEDIUM
File: app/src/test/whiskeyOverlay.smoke.test.ts

14 tests all pass. All happy-path: JS snippet shape checks, state serialization, date formatting.
Per WHISKEY_AUDIT.md L1 theatre convention.

No test exercises: nonce injection, malformed JSON drain, null-session push, any failure path in the polling loop.

---

## LOW

### F-L1 -- Module compile graph verified: all mod declarations match present files

Severity: LOW (verified clean)
File: app/src-tauri/src/lib.rs:1-48

All mod declarations correspond to a .rs file or a directory with mod.rs.
No orphaned declarations, no duplicates. Whiskey-specific additions all have corresponding files.

---

### F-L2 -- docs/ARCHITECTURE_REVIEW_2026-05-12.md does not exist

Severity: LOW
Expected path: docs/ARCHITECTURE_REVIEW_2026-05-12.md

Three inline code comments reference senior architect review 2026-05-12:
- src/openhuman/modes/lockout.rs:99
- app/src-tauri/src/tradingview_cdp.rs:508
- app/src-tauri/src/tv_overlay.rs:109

No document at that path exists. docs/ has 14 files; none is an architecture review.

Fix: Commit the architect review document, or update inline comments to reference WHISKEY_AUDIT.md.

---

## Verified OK

Item | Result
--- | ---
CI Build / PR Quality / Installer Smoke on HEAD 61429b8d | PASS (2 of 7 checks fail -- see F-C1)
armed_for_reset_until serde default back-compat | PASS -- lockout.rs:104
lockout_reset returns Result | PASS -- lib.rs:952
arm_force_reset used (not deprecated force_reset) | PASS -- lib.rs:942
Sibling CDP commands route via tv_cdp_eval_internal | PASS -- all 5 verified
tv_cdp_eval webview gate present | PASS -- tradingview_cdp.rs:522
DoneStep navigates to /settings/tks-mods | PASS -- DoneStep.tsx:26
/settings/tks-mods route registered in Settings.tsx | PASS -- Settings.tsx:350
Module compile graph: mod decls match files | PASS
filter_by_nonce called in polling loop and drain | PASS -- tv_overlay.rs:304, 389

---

## End Summary

Severity | Count
--- | ---
CRITICAL | 2
HIGH | 4
MEDIUM | 4
LOW | 2
TOTAL | 12

Most urgent: F-C1 -- CI broken on HEAD 61429b8d. Type Check fails on dead fn variable (one-line delete). Test fails on onboarding wizard mock setup regression. Branch cannot merge or serve as stable base until green.

Second priority: F-C2 -- invalidate_tks_cache never called from settings-save path. hide_risk_pct toggle silently ignored at runtime. FIX-C agent assigned; expected-pending.

Third priority: F-H2 -- filter_by_nonce has zero tests. Four cases, pure function, approx 20 lines. Security claim against TV-page script forgery is unverified.
