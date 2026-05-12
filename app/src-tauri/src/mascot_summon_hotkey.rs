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
//! User-rebindable from the settings UI via the
//! `register_mascot_summon_hotkey` / `unregister_mascot_summon_hotkey`
//! Tauri commands in `lib.rs`. The currently-registered binding
//! variants are tracked in [`MascotSummonHotkeyState`] so the
//! unregister path knows what to take back.

use std::sync::Mutex;

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::dictation_hotkeys::expand_dictation_shortcuts;
use crate::AppRuntime;

/// Default binding string. Uses the project's `CmdOrCtrl` convention so
/// the cross-platform expansion in `expand_dictation_shortcuts` (which
/// is `CmdOrCtrl`-aware) handles macOS vs. non-macOS correctly without
/// extra cfg-gates here.
pub(crate) const DEFAULT_MASCOT_SUMMON_BINDING: &str = "CmdOrCtrl+Shift+Space";

/// Tracks the currently registered mascot-summon hotkey variants so we
/// can unregister them later. Mirrors `DictationHotkeyState`.
pub(crate) struct MascotSummonHotkeyState(pub(crate) Mutex<Vec<String>>);

/// Internal helper: install an `on_shortcut` handler that toggles the
/// mascot when the given variant is pressed. Returns the plugin error
/// verbatim (formatted) so callers can surface it / roll back.
fn install_handler(app: &AppHandle<AppRuntime>, binding: &str) -> Result<(), String> {
    let app_clone = app.clone();
    let binding_for_log = binding.to_string();
    app.global_shortcut()
        .on_shortcut(binding, move |_app, _sc, event| {
            if event.state != ShortcutState::Pressed {
                return;
            }
            log::info!("[mascot-hotkey] {binding_for_log:?} pressed — toggling mascot visibility");
            toggle_mascot(&app_clone);
        })
        .map_err(|e| format!("Failed to register shortcut '{binding}': {e}"))
}

/// Register the default mascot-summon hotkey at app startup.
///
/// Called once from `lib.rs` after the `tauri-plugin-global-shortcut`
/// plugin is built. Idempotent: re-registering the same shortcut is
/// rejected by the plugin and surfaces as a `warn` log here, never an
/// app-boot failure. Successfully registered variants are tracked in
/// [`MascotSummonHotkeyState`] so a later
/// `unregister_mascot_summon_hotkey` call can take them back.
pub(crate) fn register_default(app: &AppHandle<AppRuntime>) {
    let bindings = expand_dictation_shortcuts(DEFAULT_MASCOT_SUMMON_BINDING);
    if bindings.is_empty() {
        log::warn!(
            "[mascot-hotkey] default binding {DEFAULT_MASCOT_SUMMON_BINDING:?} expanded to nothing — \
             skipping registration"
        );
        return;
    }

    let state = app.state::<MascotSummonHotkeyState>();
    let mut guard = state.0.lock().unwrap_or_else(|p| p.into_inner());

    for binding in &bindings {
        match install_handler(app, binding.as_str()) {
            Ok(()) => {
                log::info!("[mascot-hotkey] registered {binding:?}");
                guard.push(binding.clone());
            }
            Err(err) => log::warn!(
                "[mascot-hotkey] failed to register {binding:?}: {err}; \
                 the user's keyboard may have a conflicting binding — skipping"
            ),
        }
    }
}

/// Re-register the mascot-summon hotkey to a new binding string.
///
/// Mirrors the dictation-hotkey re-register flow: expand the binding
/// per-platform, unregister any currently registered variants, install
/// handlers for each new variant, and roll back on a per-variant
/// failure (re-installing the previous set so the user is never left
/// with no working hotkey).
pub(crate) async fn register(app: AppHandle<AppRuntime>, shortcut: String) -> Result<(), String> {
    log::info!("[mascot-hotkey] register: shortcut={shortcut}");

    let old_shortcuts = {
        let state = app.state::<MascotSummonHotkeyState>();
        let guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
        guard.clone()
    };

    let expanded_shortcuts = expand_dictation_shortcuts(&shortcut);
    if expanded_shortcuts.is_empty() {
        return Err("Shortcut cannot be empty".to_string());
    }
    log::info!(
        "[mascot-hotkey] expanded shortcuts: {}",
        expanded_shortcuts.join(", ")
    );

    let mut unregistered_old: Vec<String> = Vec::new();
    for old in &old_shortcuts {
        log::debug!("[mascot-hotkey] unregistering previous shortcut: {old}");
        if let Err(e) = app.global_shortcut().unregister(old.as_str()) {
            for restored in &unregistered_old {
                if let Err(restore_err) = install_handler(&app, restored.as_str()) {
                    log::warn!(
                        "[mascot-hotkey] rollback failed while restoring old shortcut '{restored}': {restore_err}"
                    );
                }
            }
            return Err(format!(
                "Failed to unregister previous shortcut '{old}': {e}"
            ));
        }
        unregistered_old.push(old.clone());
    }

    let mut newly_registered: Vec<String> = Vec::new();
    for shortcut_variant in &expanded_shortcuts {
        if let Err(err) = install_handler(&app, shortcut_variant.as_str()) {
            log::error!("[mascot-hotkey] failed to register shortcut '{shortcut_variant}': {err}");
            for registered in &newly_registered {
                if let Err(unregister_err) = app.global_shortcut().unregister(registered.as_str()) {
                    log::warn!(
                        "[mascot-hotkey] rollback failed while unregistering '{registered}': {unregister_err}"
                    );
                }
            }
            for old in &old_shortcuts {
                if let Err(restore_err) = install_handler(&app, old.as_str()) {
                    log::warn!(
                        "[mascot-hotkey] rollback failed while restoring old shortcut '{old}': {restore_err}"
                    );
                }
            }
            return Err(err);
        }
        newly_registered.push(shortcut_variant.clone());
    }

    {
        let state = app.state::<MascotSummonHotkeyState>();
        let mut guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
        *guard = expanded_shortcuts.clone();
    }

    log::info!(
        "[mascot-hotkey] shortcuts registered: {}",
        expanded_shortcuts.join(", ")
    );
    Ok(())
}

/// Drain the state Vec and unregister each currently-registered
/// mascot-summon hotkey variant. No-op when nothing is registered.
pub(crate) async fn unregister_all(app: AppHandle<AppRuntime>) -> Result<(), String> {
    log::info!("[mascot-hotkey] unregister_all: called");
    let state = app.state::<MascotSummonHotkeyState>();
    let mut guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
    if guard.is_empty() {
        log::debug!("[mascot-hotkey] no shortcut registered — nothing to unregister");
        return Ok(());
    }
    let old_shortcuts = guard.clone();
    guard.clear();
    drop(guard);
    for old in old_shortcuts {
        log::debug!("[mascot-hotkey] unregistering shortcut: {old}");
        app.global_shortcut()
            .unregister(old.as_str())
            .map_err(|e| {
                log::warn!("[mascot-hotkey] failed to unregister '{old}': {e}");
                format!("Failed to unregister shortcut '{old}': {e}")
            })?;
        log::info!("[mascot-hotkey] shortcut unregistered: {old}");
    }
    Ok(())
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
    use super::{MascotSummonHotkeyState, DEFAULT_MASCOT_SUMMON_BINDING};
    use crate::dictation_hotkeys::expand_dictation_shortcuts;
    use std::sync::Mutex;

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

    #[test]
    fn state_starts_empty_and_accepts_inserts() {
        let state = MascotSummonHotkeyState(Mutex::new(Vec::new()));
        {
            let guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
            assert!(guard.is_empty(), "fresh state should be empty");
        }
        {
            let mut guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
            guard.push("Ctrl+Shift+Space".to_string());
            guard.push("Cmd+Shift+Space".to_string());
        }
        let guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
        assert_eq!(guard.len(), 2);
        assert!(guard.contains(&"Ctrl+Shift+Space".to_string()));
    }

    #[test]
    fn state_clear_drops_all_entries() {
        let state = MascotSummonHotkeyState(Mutex::new(vec![
            "Ctrl+Shift+Space".to_string(),
            "Cmd+Shift+Space".to_string(),
        ]));
        {
            let mut guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
            assert_eq!(guard.len(), 2);
            guard.clear();
        }
        let guard = state.0.lock().unwrap_or_else(|p| p.into_inner());
        assert!(guard.is_empty(), "state should be empty after clear");
    }

    #[test]
    fn empty_shortcut_expands_to_nothing() {
        // The `register` Tauri command relies on this to short-circuit
        // with a "Shortcut cannot be empty" error before touching the
        // global-shortcut plugin.
        assert!(expand_dictation_shortcuts("").is_empty());
        assert!(expand_dictation_shortcuts("   ").is_empty());
    }
}
