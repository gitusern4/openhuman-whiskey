//! Default mode — no-op shim that preserves upstream OpenHuman behaviour.
//!
//! By construction this mode overrides nothing: every hook returns the
//! trait default. When `DefaultMode` is the active mode, the agent
//! behaves byte-identically to upstream `tinyhumansai/openhuman`. This
//! is the regression-safety guarantee for the modes-abstraction fork.

use super::Mode;

pub struct DefaultMode;

impl DefaultMode {
    pub const ID: &'static str = "default";

    pub fn new() -> Self {
        Self
    }
}

impl Default for DefaultMode {
    fn default() -> Self {
        Self::new()
    }
}

impl Mode for DefaultMode {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn display_name(&self) -> &str {
        "Default"
    }

    fn description(&self) -> &str {
        "Stock OpenHuman assistant. No persona overrides, no extra memory roots, all tools allowed."
    }
    // All other hooks fall back to the trait defaults — that is the
    // entire point of this mode.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mode_is_noop() {
        let m = DefaultMode::new();
        assert_eq!(m.id(), "default");
        assert!(m.system_prompt_prefix().is_none());
        assert!(m.reflection_prompt_override().is_none());
        assert!(m.heartbeat_prompt_override().is_none());
        assert!(m.additional_memory_roots().is_empty());
        assert!(m.session_memory_write_path().is_none());
        assert!(m.tool_allowlist().is_none());
    }
}
