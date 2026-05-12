use std::collections::HashSet;

/// Maps UI-level tool toggle IDs (stored in app state) to the Rust tool
/// `name()` values they control. Tools not covered by any mapping entry
/// are always retained — only tools that appear here are filterable.
const TOOL_ID_TO_RUST_NAMES: &[(&str, &[&str])] = &[
    ("shell", &["shell"]),
    ("git_operations", &["git_operations"]),
    ("file_read", &["file_read", "read_diff", "csv_export"]),
    ("file_write", &["file_write", "update_memory_md"]),
    ("screenshot", &["screenshot"]),
    ("image_info", &["image_info"]),
    ("browser_open", &["browser_open"]),
    ("browser", &["browser"]),
    ("http_request", &["http_request"]),
    ("web_search", &["web_search_tool"]),
    ("memory_store", &["memory_store"]),
    ("memory_recall", &["memory_recall"]),
    ("memory_forget", &["memory_forget"]),
    (
        "cron",
        &[
            "cron_add",
            "cron_list",
            "cron_remove",
            "cron_update",
            "cron_run",
            "cron_runs",
        ],
    ),
    ("schedule", &["schedule"]),
];

/// All Rust tool names that are filterable (union of all mapping values).
/// Any tool whose name is NOT in this set is infrastructure and always retained.
fn all_filterable_tool_names() -> HashSet<&'static str> {
    TOOL_ID_TO_RUST_NAMES
        .iter()
        .flat_map(|(_, names)| names.iter().copied())
        .collect()
}

/// Given the list of enabled Rust tool names (already expanded from UI IDs by
/// the frontend), retain only tools that are either infrastructure (not
/// filterable) or explicitly enabled.
///
/// An empty `enabled_tool_names` list means "all enabled" (default / not yet
/// configured) — the filter is a no-op in that case.
pub(crate) fn filter_tools_by_user_preference(
    tools: &mut Vec<Box<dyn crate::openhuman::tools::Tool>>,
    enabled_tool_names: &[String],
) {
    if enabled_tool_names.is_empty() {
        // Empty list means all tools are enabled (user has not configured preferences yet).
        return;
    }

    let filterable = all_filterable_tool_names();

    let allowed: HashSet<&str> = enabled_tool_names.iter().map(String::as_str).collect();

    let before = tools.len();
    tools.retain(|tool| {
        let name = tool.name();
        // Infrastructure tools not covered by any mapping entry are always retained.
        if !filterable.contains(name) {
            return true;
        }
        allowed.contains(name)
    });
    let after = tools.len();

    if before != after {
        log::debug!(
            "[tool-filter] filtered tools by user preference: {} → {} tools ({} removed)",
            before,
            after,
            before - after
        );
    }
}

/// Whiskey fork: filter the tool registry down to the active mode's
/// allowlist. `DefaultMode` returns `None` from `tool_allowlist()` so
/// this is a no-op when the user is in default mode — upstream
/// behaviour preserved.
///
/// Modes that DO supply an allowlist (e.g. `WhiskeyMode`) restrict the
/// agent to a deliberate subset for safety. Whiskey explicitly excludes
/// shell/execute tools so the trading-mentor mode cannot run arbitrary
/// host commands even if the LLM tries.
pub(crate) fn filter_tools_by_active_mode(tools: &mut Vec<Box<dyn crate::openhuman::tools::Tool>>) {
    let mode = crate::openhuman::modes::active_mode();
    let Some(allowlist) = mode.tool_allowlist() else {
        return;
    };
    let allowed: HashSet<&str> = allowlist.iter().copied().collect();

    let before = tools.len();
    tools.retain(|tool| allowed.contains(tool.name()));
    let after = tools.len();

    if before != after {
        log::info!(
            "[tool-filter] active mode '{}' allowlist: {} → {} tools ({} removed)",
            mode.id(),
            before,
            after,
            before - after
        );
    }
}

/// Per-dispatch enforcement check — returns `true` if the named tool is
/// allowed by the *currently-active* mode's `tool_allowlist()`.
///
/// This is the second layer of mode-based tool gating. The first layer
/// is [`filter_tools_by_active_mode`], which prunes the registry at
/// construction time so the LLM never even sees disallowed tools. That
/// layer is great for the steady state but goes stale the moment the
/// user switches modes mid-session — the agent loop is still holding
/// the registry it built when mode A was active, and the LLM may try to
/// call a tool that A allowed but the now-current mode B does not. This
/// helper closes that gap by re-consulting the active mode at each
/// dispatch.
///
/// `DefaultMode` returns `None` from `tool_allowlist()`, so this
/// function returns `true` for every tool name — preserving upstream
/// behaviour for users who never switch modes. Modes that DO supply an
/// allowlist (e.g. `WhiskeyMode`) reject anything outside it.
///
/// Note: callers are responsible for converting a `false` return into
/// the user-facing rejection (a `ToolResult` with `is_error: true` and
/// a clear message). This function intentionally only answers the
/// boolean question so it can be reused from any dispatch site.
pub(crate) fn is_tool_allowed_in_active_mode(tool_name: &str) -> bool {
    let mode = crate::openhuman::modes::active_mode();
    match mode.tool_allowlist() {
        None => true,
        Some(allowlist) => allowlist.iter().any(|n| *n == tool_name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::modes::{set_active_mode, DefaultMode, WhiskeyMode};
    use std::sync::Mutex;

    /// Process-wide lock so tests that mutate the global active-mode
    /// pointer don't race. Single in-file Mutex keeps the project's
    /// dev-dependency surface flat (no `serial_test` needed).
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn reset_to_default() {
        let _ = set_active_mode(DefaultMode::ID);
    }

    #[test]
    fn default_mode_does_not_filter() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        reset_to_default();
        // No real tools needed — the allowlist short-circuit triggers
        // before any retain pass.
        let mut tools: Vec<Box<dyn crate::openhuman::tools::Tool>> = Vec::new();
        filter_tools_by_active_mode(&mut tools);
        assert!(tools.is_empty()); // Empty in == empty out, no panic.
    }

    #[test]
    fn whiskey_mode_allowlist_is_consulted() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        // Switch to Whiskey, verify the allowlist contains expected
        // tools. The actual retain on a non-empty Vec is exercised by
        // integration tests that build a real tool registry.
        let _ = set_active_mode(WhiskeyMode::ID);
        let mode = crate::openhuman::modes::active_mode();
        let allowlist = mode.tool_allowlist().expect("whiskey allowlists tools");
        // Tool names must match `Tool::name()` strings exactly — the
        // image-gen tool registers as `image_gen_pollinations`.
        assert!(allowlist.iter().any(|t| *t == "image_gen_pollinations"));
        assert!(!allowlist.iter().any(|t| t.contains("shell")));
        reset_to_default();
    }

    // ── Per-dispatch enforcement (`is_tool_allowed_in_active_mode`) ─

    #[test]
    fn default_mode_allows_every_tool_name_per_dispatch() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        reset_to_default();
        // DefaultMode advertises no allowlist → helper returns true for
        // arbitrary names (including ones that don't exist as real tools).
        assert!(is_tool_allowed_in_active_mode("shell"));
        assert!(is_tool_allowed_in_active_mode("image_gen_pollinations"));
        assert!(is_tool_allowed_in_active_mode("nonexistent_tool_xyz"));
    }

    #[test]
    fn whiskey_mode_allows_listed_tool_per_dispatch() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let _ = set_active_mode(WhiskeyMode::ID);
        assert!(is_tool_allowed_in_active_mode("image_gen_pollinations"));
        reset_to_default();
    }

    #[test]
    fn whiskey_mode_rejects_shell_per_dispatch() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let _ = set_active_mode(WhiskeyMode::ID);
        assert!(!is_tool_allowed_in_active_mode("shell"));
        reset_to_default();
    }

    /// Integration-style: simulate the dispatch site's two-step flow
    /// (check helper → build rejection ToolResult on false) and verify
    /// that mid-session mode switches flip the verdict for the same
    /// tool name.
    #[test]
    fn mode_switch_flips_dispatch_verdict_for_same_tool() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tool_name = "shell";

        // Whiskey rejects shell — dispatch would build an error ToolResult.
        let _ = set_active_mode(WhiskeyMode::ID);
        assert!(!is_tool_allowed_in_active_mode(tool_name));
        let mode = crate::openhuman::modes::active_mode();
        let rejection = crate::openhuman::tools::traits::ToolResult::error(format!(
            "Tool '{}' is not allowed in active mode '{}'",
            tool_name,
            mode.id()
        ));
        assert!(rejection.is_error);
        assert!(rejection
            .output()
            .contains("not allowed in active mode 'whiskey'"));

        // Switch back — same name now passes the check.
        reset_to_default();
        assert!(is_tool_allowed_in_active_mode(tool_name));
    }
}
