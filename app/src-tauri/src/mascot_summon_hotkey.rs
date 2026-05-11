//! Global hotkey to summon the floating mascot from anywhere.
//!
//! Default binding: `CmdOrCtrl+Shift+Space`.
//!   - Windows: `Ctrl+Shift+Space` (avoids the system Win+Space IME
//!     switcher and `Ctrl+Space` which collides with IDE autocomplete
//!     when the user has VS Code / IntelliJ in the foreground).
//!   - macOS: `Cmd+Shift+Space` (avoids Spotlight's `Cmd+Space`).
//!   - Linux: no-op for now — no mascot path on Linux.
//!
//! Mirrors the pattern in `dictation_hotkeys.rs` + the
//! `register_dictation_hotkey` command in `lib.rs`: register at
//! startup, unregister on shutdown, swallow registration failures with
//! a `warn` log so a busy hotkey on the user's machine doesn't keep
//! the app from booting.
//!
//! Configurability is deliberately deferred. v1 hardcodes the default
//! binding; a follow-up will expose `register_mascot_summon_hotkey`
//! / `unregister_mascot_summon_hotkey` Tauri commands so the
//! settings UI can rebind it like the dictation hotkey already does.

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::dictation_hotkeys::expand_dictation_shortcuts;
use crate::AppRuntime;

/// Default binding string. Uses the project's `CmdOrCtrl` convention so
/// the cross-platform expansion in `expand_dictation_shortcuts` (which
/// is `CmdOrCtrl`-aware) handles macOS vs. non-macOS correctly without
/// extra cfg-gates here.
pub(crate) const DEFAULT_MASCOT_SUMMON_BINDING: &str = "CmdOrCtrl+Shift+Space";

/// Register the default mascot-summon hotkey at app startup.
///
/// Called once from `lib.rs` after the `tauri-plugin-global-shortcut`
/// plugin is built. Idempotent: re-registering the same shortcut is
/// rejected by the plugin and surfaces as a `warn` log here, never an
/// app-boot failure.
pub(crate) fn register_default(app: &AppHandle<AppRuntime>) {
    let bindings = expand_dictation_shortcuts(DEFAULT_MASCOT_SUMMON_BINDING);
    if bindings.is_empty() {
        log::warn!(
            "[mascot-hotkey] default binding {DEFAULT_MASCOT_SUMMON_BINDING:?} expanded to nothing — \
             skipping registration"
        );
        return;
    }

    for binding in &bindings {
        let app_clone = app.clone();
        let binding_for_log = binding.clone();
        let result =
            app.global_shortcut()
                .on_shortcut(binding.as_str(), move |_app, _sc, event| {
                    if event.state != ShortcutState::Pressed {
                        return;
                    }
                    log::info!(
                        "[mascot-hotkey] {binding_for_log:?} pressed — toggling mascot visibility"
                    );
                    toggle_mascot(&app_clone);
                });
        match result {
            Ok(()) => log::info!("[mascot-hotkey] registered {binding:?}"),
            Err(err) => log::warn!(
                "[mascot-hotkey] failed to register {binding:?}: {err}; \
                 the user's keyboard may have a conflicting binding — skipping"
            ),
        }
    }
}

/// Toggle: show the mascot if it's hidden, hide if it's already up.
/// Per-platform dispatch lives in `mascot_window_show` /
/// `mascot_window_hide` (in `lib.rs`); this just calls them and logs
/// any failure without propagating, since the hotkey is best-effort
/// from the user's POV.
fn toggle_mascot(app: &AppHandle<AppRuntime>) {
    let already_open = mascot_is_open(app);
    if already_open {
        log::debug!("[mascot-hotkey] mascot already open — hiding");
        if let Err(err) = crate::mascot_window_hide(app.clone()) {
            log::warn!("[mascot-hotkey] mascot_window_hide failed: {err}");
        }
    } else {
        log::debug!("[mascot-hotkey] mascot not open — showing");
        if let Err(err) = crate::mascot_window_show(app.clone()) {
            log::warn!("[mascot-hotkey] mascot_window_show failed: {err}");
        }
    }
}

/// Per-platform openness check. macOS uses the thread-local in
/// `mascot_native_window`; Windows uses Tauri's window registry via
/// `mascot_windows_window::is_open`. Both branches go through the
/// `mascot_native_window_is_open(app)` helper in `lib.rs` which is
/// already cfg-gated.
fn mascot_is_open(app: &AppHandle<AppRuntime>) -> bool {
    #[cfg(target_os = "macos")]
    {
        let _ = app;
        crate::mascot_native_window::is_open()
    }
    #[cfg(target_os = "windows")]
    {
        crate::mascot_windows_window::is_open(app)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = app;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::DEFAULT_MASCOT_SUMMON_BINDING;
    use crate::dictation_hotkeys::expand_dictation_shortcuts;

    #[test]
    fn default_binding_is_cmdorctrl_shift_space() {
        // Hard-pin the default so a typo in a future refactor is caught.
        assert_eq!(DEFAULT_MASCOT_SUMMON_BINDING, "CmdOrCtrl+Shift+Space");
    }

    #[test]
    fn default_binding_expands_to_at_least_one_shortcut_per_os() {
        // The CmdOrCtrl convention should always produce at least one
        // OS-specific binding the plugin can register against.
        let expanded = expand_dictation_shortcuts(DEFAULT_MASCOT_SUMMON_BINDING);
        assert!(!expanded.is_empty(), "expansion yielded zero bindings");
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn default_binding_includes_cmd_variant_on_macos() {
        let expanded = expand_dictation_shortcuts(DEFAULT_MASCOT_SUMMON_BINDING);
        assert!(expanded.iter().any(|b| b == "Cmd+Shift+Space"));
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn default_binding_is_ctrl_variant_off_macos() {
        let expanded = expand_dictation_shortcuts(DEFAULT_MASCOT_SUMMON_BINDING);
        assert_eq!(expanded, vec!["Ctrl+Shift+Space".to_string()]);
    }
}
