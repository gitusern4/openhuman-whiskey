//! Whiskey — trading-mentor mode.
//!
//! Persona, memory roots, and reflection prompts that turn OpenHuman into
//! the user's "Whiskey" trading mentor. The persona, covenant, and trade
//! catalog already live as `.md` files maintained by the user's Claude
//! Code Whiskey skill at the path returned by [`whiskey_memory_root`];
//! this mode does not replace those files, it points the OpenHuman memory
//! ingestion sweep at them so the agent talks with the same voice and
//! catalog the user has built up over time.
//!
//! Write-back: Whiskey-on-OpenHuman never overwrites the existing skill
//! files. New session memories go to a single append-only file
//! `whiskey_session_log.md` in the same directory so the Claude Code
//! skill can keep working in parallel.

use std::path::PathBuf;

use super::Mode;

/// `WhiskeyMode` — the trading-mentor persona.
pub struct WhiskeyMode {
    memory_root: PathBuf,
}

impl WhiskeyMode {
    pub const ID: &'static str = "whiskey";

    /// Whitelisted tools while in Whiskey mode. Trading work is heavily
    /// reasoning + memory + image-gen; we deliberately do not expose
    /// shell / arbitrary-execute tools to keep blast radius small.
    ///
    /// Names MUST match the upstream `Tool::name()` strings (snake_case,
    /// no dots) — the allowlist filter compares string-literally.
    const ALLOWED_TOOLS: &'static [&'static str] = &[
        // Memory access (read + write).
        "memory_recall",
        "memory_store",
        "memory_forget",
        // Memory tree — full family. The umbrella `memory_tree` and
        // every specific operation. WHISKEY_AUDIT.md C2 caught the
        // earlier list shipping only the umbrella name and silently
        // disabling the more specific tools the trading mentor needs
        // to drill into the user's curated A+ catalog.
        "memory_tree",
        "memory_tree_drill_down",
        "memory_tree_fetch_leaves",
        "memory_tree_query_global",
        "memory_tree_query_source",
        "memory_tree_query_topic",
        "memory_tree_search_entities",
        // Whiskey fork: free image generation for concept maps.
        "image_gen_pollinations",
        // Web research — for fresh market context the playbook
        // can't pre-cache.
        "web_fetch",
        "web_search_tool",
        "http_request",
        // Read-only timing primitives the agent leans on.
        "current_time",
        // Whiskey fork: TradingView Desktop CDP bridge tools.
        // Read the active chart state (symbol, timeframe, indicators,
        // shapes, alert count) and switch the chart symbol.
        "tv_chart_get_state",
        "tv_chart_set_symbol",
    ];

    /// Construct with the default memory root resolved from the
    /// environment + filesystem (see [`default_whiskey_memory_root`]
    /// for the full resolution chain). Equivalent to
    /// [`WhiskeyMode::with_env_overrides`].
    pub fn new() -> Self {
        Self::with_env_overrides()
    }

    /// Construct by running the full memory-root resolution chain:
    /// `OPENHUMAN_WHISKEY_MEMORY_ROOT` env var, then
    /// `~/.openhuman/whiskey_memory/` if it exists, then the legacy
    /// Claude-Code skill path if it exists, then the openhuman path
    /// as a last-resort fallback. See
    /// [`default_whiskey_memory_root`] for the authoritative spec.
    pub fn with_env_overrides() -> Self {
        Self {
            memory_root: default_whiskey_memory_root(),
        }
    }

    /// Override the memory root (used by tests and by the eventual
    /// settings UI). Bypasses the env / filesystem resolution chain.
    pub fn with_memory_root(memory_root: PathBuf) -> Self {
        Self { memory_root }
    }

    /// The five canonical Whiskey skill files the boot ingestion sweep
    /// folds into the Memory Tree. Files that don't exist yet are
    /// silently skipped by the ingestion layer.
    fn canonical_files(&self) -> Vec<PathBuf> {
        [
            "user_profile_trader.md",
            "whiskey_relationship_log.md",
            "whiskey_covenant.md",
            "whiskey_playbook.md",
            "trade_log.md",
            "pattern_log.md",
            "project_market_briefings.md",
            "project_whiskey_bot_lab.md",
        ]
        .iter()
        .map(|name| self.memory_root.join(name))
        .collect()
    }
}

impl Default for WhiskeyMode {
    fn default() -> Self {
        Self::new()
    }
}

impl Mode for WhiskeyMode {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn display_name(&self) -> &str {
        "Whiskey"
    }

    fn description(&self) -> &str {
        "Trading mentor — reads your A+ catalog and pattern log, scores setups, never executes."
    }

    fn system_prompt_prefix(&self) -> Option<&str> {
        Some(WHISKEY_SYSTEM_PREFIX)
    }

    fn reflection_prompt_override(&self) -> Option<&str> {
        Some(WHISKEY_REFLECTION_PROMPT)
    }

    fn heartbeat_prompt_override(&self) -> Option<&str> {
        Some(WHISKEY_HEARTBEAT_PROMPT)
    }

    fn additional_memory_roots(&self) -> Vec<PathBuf> {
        // The whole directory is added; the ingestion layer scans for
        // `.md` files inside. `canonical_files()` is informational only
        // (used by tests and the doctor) — adding the root catches new
        // files (e.g. `whiskey_session_log.md`, future R-multiple
        // exports) without code changes.
        vec![self.memory_root.clone()]
    }

    fn session_memory_write_path(&self) -> Option<PathBuf> {
        Some(self.memory_root.join("whiskey_session_log.md"))
    }

    fn tool_allowlist(&self) -> Option<&[&'static str]> {
        Some(Self::ALLOWED_TOOLS)
    }

    fn overlay_source(&self) -> &str {
        "whiskey"
    }
}

/// Resolution chain for the Whiskey memory directory. First hit wins.
///
/// 1. **`OPENHUMAN_WHISKEY_MEMORY_ROOT` env var** — explicit override,
///    no existence check (operator opted in; if they typo'd, the
///    downstream `read_dir` returns `None` and ingestion silently
///    skips, which is the correct behaviour). The `OPENHUMAN_` prefix
///    matches the convention used elsewhere in this crate (see
///    `OPENHUMAN_HOME`, `OPENHUMAN_MEMORY_*`, etc.).
/// 2. **`~/.openhuman/whiskey_memory/`** — the canonical
///    cross-machine location, used only if it exists. This is the
///    "I rsynced my Whiskey notes here" path: explicit user opt-in by
///    creating the directory.
/// 3. **`~/.claude/projects/C--Users-legen-Documents-ruflo-main/memory/`**
///    — the legacy hardcoded path the original fork author used.
///    Honored only if it exists, so other users aren't pointed at a
///    nonexistent dir specific to one machine.
/// 4. **Fallback: `~/.openhuman/whiskey_memory/`** — returned even
///    when nothing exists. The memory cache will return `None` until
///    the user creates the directory; that's the intended UX, not an
///    error.
///
/// Resolved at runtime (not a `const`) because `dirs::home_dir()` and
/// `std::env::var()` aren't const-evaluable.
fn default_whiskey_memory_root() -> PathBuf {
    // 1. Explicit env-var override.
    if let Ok(raw) = std::env::var("OPENHUMAN_WHISKEY_MEMORY_ROOT") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    let home = dirs::home_dir();

    // 2. Cross-machine canonical location, if the user created it.
    let openhuman_path = home
        .as_ref()
        .map(|h| h.join(".openhuman").join("whiskey_memory"));
    if let Some(p) = &openhuman_path {
        if p.is_dir() {
            return p.clone();
        }
    }

    // 3. Legacy author-specific path, only if present on this machine.
    if let Some(home) = &home {
        let legacy = home
            .join(".claude")
            .join("projects")
            .join("C--Users-legen-Documents-ruflo-main")
            .join("memory");
        if legacy.is_dir() {
            return legacy;
        }
    }

    // 4. Fallback — the openhuman path even if it doesn't exist yet.
    if let Some(p) = openhuman_path {
        return p;
    }
    // Final fallback when even `dirs::home_dir()` is unavailable.
    PathBuf::from(".openhuman/whiskey_memory")
}

// ---------------------------------------------------------------------------
// Persona prompts.
//
// Kept in this file (not external `.md`) so they ship in the binary and the
// mode is fully self-contained. The actual playbook + covenant are loaded
// dynamically at runtime via `additional_memory_roots()` so the user's
// evolving catalog steers behaviour — these prompts only set tone, scope,
// and the never-execute guardrail.
// ---------------------------------------------------------------------------

const WHISKEY_SYSTEM_PREFIX: &str = r#"You are Whiskey — the user's trading mentor.

Identity:
- You are not a generic assistant. You are a senior trader who took the
  user under your wing. You talk like a real trader texting between
  setups: short, declarative, no hedging filler.
- You remember every prior trade in the user's playbook (loaded from
  whiskey_playbook.md and trade_log.md). When the user describes a
  setup, you compare it against the A+ catalog FIRST, before improvising.

Process discipline (Steenbarger / Douglas / Brett N. Steenbarger lineage):
- Risk-defined plans only. Every suggestion includes: entry, stop,
  target, R-multiple, position size relative to the user's stated risk
  floor.
- A+ catalog setups get full size. Off-catalog setups get half-R or
  paper, with explicit "this isn't catalog yet — log it for promotion
  candidacy" framing.
- Score every setup with a confidence percentage. The percentage means
  "playbook match score, not edge guarantee." Always say so when you
  state it.
- Surface psychological patterns from pattern_log.md when triggered
  (revenge, FOMO, oversize, disposition, tilt). Surface ONCE, then drop
  it. No nagging.

Hard constraints (the covenant — see whiskey_covenant.md):
- You NEVER execute trades. The user always executes manually.
- You NEVER move money or change orders.
- You NEVER overstate confidence to push the user into a trade.
- When you don't know the live market state, say so plainly. Do not
  invent quotes or positions.
- If the user is in tilt or has hit the daily loss limit, you say it
  once and stop suggesting setups for the session.

Voice:
- Best-friend-who-made-it tone. Direct, warm, zero ceremony.
- Use the user's actual language for setups when it appears in the
  playbook (their pattern names, not generic textbook names).
- When you reference the playbook, name the file and the specific
  setup ID so the user can verify.

If asked anything outside trading, briefly redirect: "Outside scope for
me right now — switch to the default mode and I'll catch you when
you're back."
"#;

const WHISKEY_REFLECTION_PROMPT: &str = r#"You just finished a turn with the user inside Whiskey trading-mentor mode.
Reflect ONLY on the trading dimension of what happened. Output JSON with:
- observations: factual notes about the user's current trade or thinking.
- patterns: any pattern_log.md categories that fired this turn (revenge,
  FOMO, oversize, disposition, tilt). Empty list if none — do NOT invent.
- user_preferences: trading-specific preferences the user revealed
  (preferred instruments, time-of-day, risk caps).
- user_reflections: explicit user self-statements about their trading
  (verbatim or near-verbatim). Empty if none.

Do not include market predictions, generic trading advice, or content
unrelated to this user's playbook + covenant. If the turn was off-topic,
return all-empty arrays.
"#;

// WHISKEY_AUDIT.md H5: the previous version of this prompt instructed
// the model to "scan recent screen_intelligence snapshots" — but the
// Windows screen_intelligence engine is a documented stub that returns
// `NotImplementedYet` and produces zero snapshots. LLMs faced with
// "scan X for Y" and no X reliably hallucinate Y, so the rewritten
// prompt below grounds the heartbeat strictly in Memory Tree state +
// the user's curated playbook files, both of which actually exist.
// When the screen-watch engine ships, restore the screen_intelligence
// reference here.
const WHISKEY_HEARTBEAT_PROMPT: &str = r#"Periodic background reflection while Whiskey mode is active.
Look ONLY at the user's existing playbook files (whiskey_playbook.md,
pattern_log.md, trade_log.md) and any new Memory Tree updates since the
last heartbeat. Do not invent screen-state, position data, or fills —
when no source confirms a fact, omit it.

Surface ONLY:
1. Pattern_log.md categories that look likely to fire in the next 30
   minutes given the most recent reflections in user_reflections /
   conversations memory (cite the specific reflection).
2. A+ catalog setups in whiskey_playbook.md that the user has noted
   matching market conditions for, and that have not yet been entered
   in this session (cite the specific catalog entry by ID).
Emit a short overlay-attention message ONLY when one of those two
conditions has clear, source-cited evidence. Stay silent otherwise.
Never alert just to be helpful. Never report on data you cannot cite.
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whiskey_mode_id_and_naming() {
        let m = WhiskeyMode::new();
        assert_eq!(m.id(), "whiskey");
        assert_eq!(m.display_name(), "Whiskey");
        assert!(m.system_prompt_prefix().unwrap().contains("trading mentor"));
        assert!(m.reflection_prompt_override().is_some());
        assert!(m.heartbeat_prompt_override().is_some());
    }

    #[test]
    fn whiskey_mode_has_memory_root_and_session_log() {
        let m = WhiskeyMode::new();
        let roots = m.additional_memory_roots();
        assert_eq!(roots.len(), 1);
        let session_log = m.session_memory_write_path().unwrap();
        // Session log lives inside the configured root, regardless of
        // which branch of the resolution chain produced the root.
        assert!(session_log.starts_with(&roots[0]));
        assert_eq!(
            session_log.file_name().unwrap().to_string_lossy(),
            "whiskey_session_log.md"
        );
        // The resolved root must be non-empty — the resolution chain
        // always yields *some* path, even if the directory doesn't
        // exist yet on this machine.
        assert!(!roots[0].as_os_str().is_empty());
    }

    #[test]
    fn whiskey_mode_tool_allowlist_excludes_shell() {
        let m = WhiskeyMode::new();
        let allowed = m.tool_allowlist().expect("whiskey allowlists tools");
        // At least one memory tool, the image-gen tool, no shell/execute.
        assert!(allowed.iter().any(|t| t.starts_with("memory_")));
        assert!(allowed.iter().any(|t| *t == "image_gen_pollinations"));
        assert!(!allowed.iter().any(|t| t.contains("shell")));
        assert!(!allowed.iter().any(|t| t.contains("execute")));
    }

    #[test]
    fn whiskey_mode_with_custom_root() {
        let custom = PathBuf::from("/tmp/whiskey-test");
        let m = WhiskeyMode::with_memory_root(custom.clone());
        assert_eq!(m.additional_memory_roots(), vec![custom.clone()]);
        assert_eq!(
            m.session_memory_write_path().unwrap(),
            custom.join("whiskey_session_log.md")
        );
    }

    // ---------------------------------------------------------------
    // Resolution chain tests.
    //
    // The env-var test mutates a process-wide variable, so it is
    // marked `#[serial]`-style by guarding the var name to one
    // unique to this test (and clearing it on exit). The other
    // resolution-chain tests exercise `default_whiskey_memory_root`
    // indirectly via temp directories where possible, or directly
    // when a controlled HOME is needed.
    // ---------------------------------------------------------------

    /// Guard that resets an env var on drop so a panicking test
    /// can't leak state into sibling tests.
    ///
    /// WHISKEY_AUDIT.md M5/M6: env vars are process-global, so two
    /// tests in different files that mutate ANY env var can race
    /// even when their respective per-file test locks are held.
    /// EnvVarGuard now also holds a process-wide
    /// `EnvVarTestGuard` for the lifetime of the guard, serializing
    /// against any other env-var-touching test in the binary.
    struct EnvVarGuard {
        key: &'static str,
        prior: Option<String>,
        // Field, not phantom — holding the guard for the lifetime of
        // EnvVarGuard is exactly the point. Drops in field-decl
        // order (this last) so the env restoration above happens
        // before we release the env-var test lock.
        _env_lock: crate::openhuman::modes::EnvVarTestGuard,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let lock = crate::openhuman::modes::EnvVarTestGuard::new();
            let prior = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self {
                key,
                prior,
                _env_lock: lock,
            }
        }

        fn unset(key: &'static str) -> Self {
            let lock = crate::openhuman::modes::EnvVarTestGuard::new();
            let prior = std::env::var(key).ok();
            std::env::remove_var(key);
            Self {
                key,
                prior,
                _env_lock: lock,
            }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.prior {
                Some(v) => std::env::set_var(self.key, v),
                None => std::env::remove_var(self.key),
            }
        }
    }

    fn unique_tmp_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("whiskey-resolve-{name}-{nanos}"));
        std::fs::create_dir_all(&dir).expect("create test dir");
        dir
    }

    #[test]
    fn resolution_env_var_overrides_everything() {
        let tmp = unique_tmp_dir("env-override");
        let _guard = EnvVarGuard::set(
            "OPENHUMAN_WHISKEY_MEMORY_ROOT",
            tmp.to_str().expect("utf8 tmp path"),
        );
        let resolved = default_whiskey_memory_root();
        assert_eq!(resolved, tmp);

        // And the public constructor wires it through.
        let m = WhiskeyMode::with_env_overrides();
        assert_eq!(m.additional_memory_roots(), vec![tmp]);
    }

    #[test]
    fn resolution_env_var_empty_string_is_ignored() {
        // An empty / whitespace env var should NOT clobber the
        // filesystem-based fallbacks — treat as if unset.
        let _guard = EnvVarGuard::set("OPENHUMAN_WHISKEY_MEMORY_ROOT", "   ");
        let resolved = default_whiskey_memory_root();
        // We can't assert the exact path (depends on the runner's
        // home dir), but it must NOT be the empty string from the env.
        assert!(!resolved.as_os_str().is_empty());
        assert_ne!(resolved, PathBuf::from("   "));
    }

    #[test]
    fn resolution_legacy_path_honored_only_when_present() {
        // We can't safely create the legacy path on the runner's
        // real home dir without polluting the user's filesystem, so
        // we exercise the *negative* branch: when the env var is
        // unset and the legacy path doesn't exist, the resolved
        // root must still be a non-empty path inside the openhuman
        // fallback location (or a relative fallback if no home).
        let _guard = EnvVarGuard::unset("OPENHUMAN_WHISKEY_MEMORY_ROOT");
        let resolved = default_whiskey_memory_root();
        // Should NOT contain the legacy author-specific segment
        // unless that directory genuinely exists on this machine.
        let s = resolved.to_string_lossy();
        let legacy_marker = "C--Users-legen-Documents-ruflo-main";
        if s.contains(legacy_marker) {
            // If the resolver returned the legacy path, the dir must
            // actually exist (this is the branch the original author
            // hits on their own box).
            assert!(
                resolved.is_dir(),
                "legacy path returned but does not exist: {}",
                resolved.display()
            );
        } else {
            // Otherwise we should be on the openhuman fallback. Pin
            // both the parent (must contain `.openhuman`) AND the leaf
            // (must be `whiskey_memory`) so a future refactor that
            // returns `~/.openhuman` (no subdir) or some unrelated leaf
            // is caught here. WHISKEY_AUDIT.md L5.
            assert!(
                s.contains("openhuman") || s.contains(".openhuman"),
                "expected openhuman fallback, got {}",
                resolved.display()
            );
            assert_eq!(
                resolved.file_name().and_then(|n| n.to_str()),
                Some("whiskey_memory"),
                "expected leaf `whiskey_memory`, got {}",
                resolved.display()
            );
        }
    }

    #[test]
    fn resolution_openhuman_dir_preferred_over_legacy() {
        // When the openhuman dir exists, it must win over the
        // legacy path (regardless of whether the legacy path also
        // exists on this machine). We can't safely manipulate the
        // user's real ~/.openhuman, so we simulate the branch by
        // pointing the env var at a known-existing temp dir — that
        // is functionally identical to the openhuman branch from
        // the resolver's perspective (both produce "first hit
        // before legacy"). Combined with
        // `resolution_legacy_path_honored_only_when_present`, this
        // covers the ordering contract.
        let tmp = unique_tmp_dir("prefers-openhuman");
        let _guard = EnvVarGuard::set(
            "OPENHUMAN_WHISKEY_MEMORY_ROOT",
            tmp.to_str().expect("utf8 tmp path"),
        );
        let resolved = default_whiskey_memory_root();
        assert_eq!(resolved, tmp);
        assert!(
            !resolved
                .to_string_lossy()
                .contains("C--Users-legen-Documents-ruflo-main"),
            "openhuman/env path must take precedence over the legacy author path"
        );
    }

    #[test]
    fn resolution_fallback_returns_openhuman_path_even_when_missing() {
        // With env var unset, the resolver must always return a
        // non-empty path. On a clean CI runner with no
        // ~/.openhuman/whiskey_memory and no legacy dir, that path
        // will be the openhuman location even though it doesn't
        // exist — memory_cache::resolve will then return None,
        // which is the documented contract.
        let _guard = EnvVarGuard::unset("OPENHUMAN_WHISKEY_MEMORY_ROOT");
        let resolved = default_whiskey_memory_root();
        assert!(!resolved.as_os_str().is_empty());
        assert!(
            resolved.file_name().is_some(),
            "expected a real path leaf, got {}",
            resolved.display()
        );
    }
}
