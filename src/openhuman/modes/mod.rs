//! Modes — switchable agent personalities.
//!
//! A "mode" packages everything that distinguishes one agent persona from
//! another into a single trait object: the system-prompt prefix injected
//! into every LLM call, the reflection prompt used by the post-turn
//! reflection hook, the heartbeat reflection variant, the allowed-tool
//! whitelist, the additional memory roots to ingest, and a stable string
//! ID for persistence.
//!
//! The default mode is [`DefaultMode`], which is a no-op shim: all hooks
//! return `None` so behaviour is byte-identical to the upstream
//! `tinyhumansai/openhuman` agent. Switching to a non-default mode (e.g.
//! [`whiskey::WhiskeyMode`]) lets callsites opt into per-mode behaviour
//! without rewriting domain code.
//!
//! Wired into:
//!   - `providers::router` — `system_prompt_prefix()` is prepended to every
//!     outgoing LLM request when a non-default mode is active.
//!   - `learning::reflection` — `reflection_prompt_override()` swaps the
//!     reflection-hook prompt.
//!   - `heartbeat::engine` — `heartbeat_prompt_override()` swaps the
//!     periodic-reflection prompt.
//!   - `memory::ingestion` — `additional_memory_roots()` add extra
//!     `.md`-vault paths to the boot ingestion sweep.

pub mod default;
pub mod registry;
pub mod whiskey;

use std::path::PathBuf;
use std::sync::Arc;

pub use default::DefaultMode;
pub use registry::{active_mode, set_active_mode, ModeRegistry};
pub use whiskey::WhiskeyMode;

/// Stable string ID used for persistence in config + UI selection.
pub type ModeId = &'static str;

/// One switchable agent persona. Implementations live in this module's
/// submodules; callers should treat them as `Arc<dyn Mode>`.
///
/// All hook methods have default no-op implementations so a new mode
/// only needs to override what it actually wants to change.
pub trait Mode: Send + Sync + 'static {
    /// Stable string identifier — used in `~/.openhuman/config.toml`,
    /// the mode-picker dropdown, and event-bus tracing.
    fn id(&self) -> ModeId;

    /// Human-readable name for the mode-picker UI.
    fn display_name(&self) -> &str;

    /// One-line description for the mode-picker UI tooltip.
    fn description(&self) -> &str {
        ""
    }

    /// String prepended to every outgoing LLM system prompt. Returning
    /// `None` (the default) means "no prefix injected" — used by
    /// [`DefaultMode`] to be byte-identical to upstream.
    fn system_prompt_prefix(&self) -> Option<&str> {
        None
    }

    /// Replacement reflection prompt used by `learning::reflection` after
    /// each qualifying turn. `None` means "use upstream default."
    fn reflection_prompt_override(&self) -> Option<&str> {
        None
    }

    /// Replacement heartbeat reflection prompt used by `heartbeat::engine`
    /// on its periodic background pass.
    fn heartbeat_prompt_override(&self) -> Option<&str> {
        None
    }

    /// Additional `.md`-vault paths the memory ingestion sweep should
    /// fold into the Memory Tree on boot. Empty by default.
    fn additional_memory_roots(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    /// Optional path the mode wants to *write* session-scoped memories
    /// into (so the Claude Code skill that originally owns the source
    /// vault stays read-canonical). `None` = use core's default store.
    fn session_memory_write_path(&self) -> Option<PathBuf> {
        None
    }

    /// Allowed-tool whitelist. If `Some(set)`, only tools whose names
    /// appear in the set may be called while this mode is active. `None`
    /// (default) means "all tools allowed."
    fn tool_allowlist(&self) -> Option<&[&'static str]> {
        None
    }

    /// Optional source label this mode emits when publishing to the
    /// overlay attention bus (`overlay::publish_attention`). Lets
    /// downstream tracing distinguish per-mode messages.
    fn overlay_source(&self) -> &str {
        self.id()
    }
}

/// Convenience boxed alias.
pub type SharedMode = Arc<dyn Mode>;
