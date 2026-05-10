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
    const ALLOWED_TOOLS: &'static [&'static str] = &[
        "memory.search",
        "memory.fetch",
        "memory.append",
        "image_gen.pollinations",
        "web.search",
        "web.fetch",
        "screen_intelligence.snapshot",
        "screen_intelligence.subscribe",
    ];

    /// Construct with the default memory root resolved from the user's
    /// home directory, falling back to a sensible relative path if
    /// `dirs::home_dir()` returns `None`.
    pub fn new() -> Self {
        Self {
            memory_root: default_whiskey_memory_root(),
        }
    }

    /// Override the memory root (used by tests and by the eventual
    /// settings UI).
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

/// Default location of the Whiskey memory files. Resolved at runtime
/// (not a `const`) because `dirs::home_dir()` is not const.
fn default_whiskey_memory_root() -> PathBuf {
    // Match the path the user's Claude Code Whiskey skill writes to.
    // Per MEMORY.md in the user's profile, the canonical location is:
    //   ~/.claude/projects/C--Users-<user>-Documents-ruflo-main/memory/
    // This will not exist on every install — that's fine, ingestion
    // tolerates missing roots.
    if let Some(home) = dirs::home_dir() {
        // The colon-encoded project dir is specific to this user; we
        // resolve the parent and let ingestion enumerate. Any new
        // project-scoped Claude memory dir will work the same.
        home.join(".claude")
            .join("projects")
            .join("C--Users-legen-Documents-ruflo-main")
            .join("memory")
    } else {
        PathBuf::from(".claude/projects/whiskey/memory")
    }
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

const WHISKEY_HEARTBEAT_PROMPT: &str = r#"Periodic background reflection while Whiskey mode is active.
Scan recent screen_intelligence snapshots and Memory Tree updates for:
1. New trades the screen-watch detected that have not yet been logged
   to whiskey_playbook.md.
2. Open positions that have been held past their planned exit time.
3. Pattern_log.md categories that look likely to fire in the next 30
   minutes given current behaviour.
Emit a short overlay-attention message ONLY for tier-1 (un-logged trades
that need logging now) or tier-3 (active pattern with high probability).
Stay silent otherwise. Never alert just to be helpful.
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
        // Session log lives inside the configured root.
        assert!(session_log.starts_with(&roots[0]));
        assert_eq!(
            session_log.file_name().unwrap().to_string_lossy(),
            "whiskey_session_log.md"
        );
    }

    #[test]
    fn whiskey_mode_tool_allowlist_excludes_shell() {
        let m = WhiskeyMode::new();
        let allowed = m.tool_allowlist().expect("whiskey allowlists tools");
        assert!(allowed.iter().any(|t| t.starts_with("memory.")));
        assert!(allowed.iter().any(|t| *t == "image_gen.pollinations"));
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
}
