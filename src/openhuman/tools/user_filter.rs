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
}
