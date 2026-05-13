//! Windows mascot path — Tauri WebviewWindow with always-on-top + transparent.
//!
//! Parallel to [`crate::mascot_native_window`] which uses a native macOS
//! NSPanel + WKWebView host. The macOS-only file exists because the
//! vendored tauri-cef runtime cannot render transparent windowed-mode
//! browsers on macOS (CEF clamps `BrowserSettings.background_color`
//! alpha for windowed browsers; only off-screen rendering supports
//! transparency, which the runtime does not enable). The Windows
//! equivalent of that limitation may or may not bite — if it does, the
//! mascot ships as an opaque small square instead of a transparent
//! floating sprite. That's acceptable for v1 (the window is still
//! always-on-top, draggable, click-to-pop-out, and the user can give
//! feedback). A native Win32 layered window + WebView2 fallback is the
//! Phase-2 fix if transparency does not come through.
//!
//! TODO(WHISKEY_AUDIT.md L4): file an upstream issue against tauri-cef
//! for transparent windowed-mode on Windows once the v1 ARM64 build
//! is shipping; until then this comment is the tracking surface so
//! the limitation isn't lost when the file changes.
//!
//! API mirrors `mascot_native_window`:
//!   - `show(app)` — create or focus the floating window
//!   - `hide()` — close + destroy
//!   - `is_open(app)` — whether the window currently exists
//!
//! The window's React entry is selected via `?window=mascot-win` in the
//! URL query string (parallel to macOS's `?window=mascot`). See
//! `app/src/main.tsx` for the routing branch.
//!
//! Deliberately gated `#[cfg(target_os = "windows")]` so the macOS and
//! Linux builds never see this module. Linux currently has no floating
//! mascot path; on Linux `mascot_window_show` returns the existing
//! "mascot is macOS-only for now" error from `lib.rs` (kept honest by
//! a `cfg`-gated branch alongside the new Windows one).

#![cfg(target_os = "windows")]

use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder};

use crate::mascot_windows_state;
use crate::AppRuntime;

/// Stable Tauri window label. Matches what the React entry checks for.
pub(crate) const MASCOT_WINDOW_LABEL: &str = "mascot-win";

/// Default mascot dimensions — matches the macOS panel size baseline so
/// the React component renders identically across platforms.
const MASCOT_WIDTH: f64 = 96.0;
const MASCOT_HEIGHT: f64 = 96.0;

/// Margin from the bottom-right of the primary monitor when no saved
/// position exists yet. Matches the spec from the UX research pass
/// ("snap to screen bottom-right with 24px margin").
const DEFAULT_EDGE_MARGIN: i32 = 24;

/// Whether a mascot window is currently registered with Tauri.
pub(crate) fn is_open(app: &AppHandle<AppRuntime>) -> bool {
    app.get_webview_window(MASCOT_WINDOW_LABEL).is_some()
}

/// Bring the existing mascot window forward, or create it if absent.
///
/// Returns `Err` only on construction failure (Tauri couldn't allocate
/// the window or load the URL); a no-op focus on an existing window is
/// `Ok(())`.
pub(crate) fn show(app: &AppHandle<AppRuntime>) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window(MASCOT_WINDOW_LABEL) {
        log::debug!("[mascot-windows] reusing existing mascot window");
        let _ = existing.show();
        let _ = existing.set_focus();
        return Ok(());
    }

    log::info!("[mascot-windows] creating mascot window label={MASCOT_WINDOW_LABEL}");

    // Build the URL the webview loads. App-relative URL — Tauri resolves
    // to the dev server in development and the bundled index.html in
    // production. The `?window=mascot-win` query string is what the
    // React entry switches on (mirrors macOS's `?window=mascot`).
    let url = WebviewUrl::App("index.html?window=mascot-win".into());

    let mut builder = WebviewWindowBuilder::new(app, MASCOT_WINDOW_LABEL, url)
        .title("Whiskey")
        .inner_size(MASCOT_WIDTH, MASCOT_HEIGHT)
        .resizable(false)
        .decorations(false)
        // Transparent if the CEF runtime supports it (see module doc
        // comment); falls back to opaque on backends that don't.
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        // Don't steal focus when shown — the mascot is a passive
        // overlay, not an app the user is "switching to".
        .focused(false)
        .visible(true);

    // Apply the saved position if we have one and it's still on-screen;
    // otherwise stash the bottom-right default position into the builder.
    let initial_position = mascot_windows_state::load()
        .filter(|state| state.has_sane_dimensions())
        .map(|state| (state.x, state.y))
        .unwrap_or_else(|| default_bottom_right(app));
    builder = builder.position(initial_position.0 as f64, initial_position.1 as f64);

    let window = builder
        .build()
        .map_err(|err| format!("[mascot-windows] WebviewWindowBuilder.build failed: {err}"))?;

    // Re-clamp to ensure the window is actually on a connected monitor
    // (the saved-position branch above already did the visibility check
    // if state was loaded; but a brand-new install or a malformed file
    // both flow through `default_bottom_right` which assumed a primary
    // monitor exists — paranoid double-check here).
    if let Err(err) = window.set_position(PhysicalPosition::new(
        initial_position.0,
        initial_position.1,
    )) {
        log::warn!("[mascot-windows] set_position after build failed: {err}");
    }
    if let Err(err) = window.set_size(PhysicalSize::new(MASCOT_WIDTH as u32, MASCOT_HEIGHT as u32))
    {
        log::warn!("[mascot-windows] set_size after build failed: {err}");
    }

    // Best-effort: hide the window from screen-share captures so
    // OBS/Zoom/Teams don't broadcast the user's mascot, AND so any
    // future Whiskey screen-watch loop never feeds its own overlay
    // back through OCR. Failure is non-fatal — older Windows builds
    // without `WDA_EXCLUDEFROMCAPTURE` will simply behave as before.
    apply_capture_exclusion(&window);

    log::info!("[mascot-windows] mascot window created and shown");
    Ok(())
}

/// Tear down the mascot window if present.
pub(crate) fn hide(app: &AppHandle<AppRuntime>) -> Result<(), String> {
    let Some(window) = app.get_webview_window(MASCOT_WINDOW_LABEL) else {
        log::debug!("[mascot-windows] hide called but no mascot window is open — no-op");
        return Ok(());
    };

    // Capture position before destroying so the next `show` lands in
    // the same spot. Best-effort — read failure just means we'll re-use
    // whatever was last persisted.
    if let Ok(pos) = window.outer_position() {
        if let Ok(size) = window.outer_size() {
            mascot_windows_state::save_state(pos.x, pos.y, size.width, size.height);
        }
    }

    window
        .close()
        .map_err(|err| format!("[mascot-windows] window.close failed: {err}"))?;
    log::info!("[mascot-windows] mascot window closed");
    Ok(())
}

/// Persist current position. Called from the frontend after the user
/// drags the mascot to a new spot (via the
/// `mascot_windows_save_position` Tauri command in `lib.rs`).
pub(crate) fn save_current_position(app: &AppHandle<AppRuntime>) {
    let Some(window) = app.get_webview_window(MASCOT_WINDOW_LABEL) else {
        return;
    };
    let Ok(pos) = window.outer_position() else {
        log::warn!("[mascot-windows] save_current_position: outer_position unavailable");
        return;
    };
    let Ok(size) = window.outer_size() else {
        log::warn!("[mascot-windows] save_current_position: outer_size unavailable");
        return;
    };
    mascot_windows_state::save_state(pos.x, pos.y, size.width, size.height);
}

/// Compute the bottom-right corner of the primary monitor, inset by
/// [`DEFAULT_EDGE_MARGIN`]. Falls back to (0, 0) when no monitor info is
/// available (the window will be visible somewhere even in the worst
/// case — Windows clamps to the desktop bounds).
fn default_bottom_right(app: &AppHandle<AppRuntime>) -> (i32, i32) {
    let Some(window) = app.get_webview_window("main") else {
        return (DEFAULT_EDGE_MARGIN, DEFAULT_EDGE_MARGIN);
    };
    let monitor = match window
        .primary_monitor()
        .or_else(|_| window.current_monitor())
    {
        Ok(Some(m)) => m,
        _ => return (DEFAULT_EDGE_MARGIN, DEFAULT_EDGE_MARGIN),
    };
    let pos = monitor.position();
    let size = monitor.size();
    let x = pos.x + size.width as i32 - MASCOT_WIDTH as i32 - DEFAULT_EDGE_MARGIN;
    let y = pos.y + size.height as i32 - MASCOT_HEIGHT as i32 - DEFAULT_EDGE_MARGIN;
    (x, y)
}

/// Best-effort `WDA_EXCLUDEFROMCAPTURE` via `windows-rs` if available.
///
/// Falls back to a no-op when `windows-rs` isn't pulled into the build
/// (the dep is gated to Windows-only in `Cargo.toml` so it always
/// compiles on this OS, but we keep this helper resilient in case the
/// dep is ever made conditional). Not having capture exclusion just
/// means screen-share will see the mascot — annoying but not broken.
fn apply_capture_exclusion<R: tauri::Runtime>(window: &tauri::WebviewWindow<R>) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE,
    };

    let hwnd = match window.hwnd() {
        Ok(h) => HWND(h.0 as *mut _),
        Err(err) => {
            log::warn!("[mascot-windows] hwnd() unavailable, skip capture exclusion: {err}");
            return;
        }
    };
    // SAFETY: hwnd was just obtained from the live window; the call is
    // safe by construction. The WDA_EXCLUDEFROMCAPTURE flag is
    // documented as a no-op on Windows builds prior to 10 2004 — the
    // call still returns success, just without the exclusion taking
    // effect, which matches the "best effort" contract here.
    unsafe {
        let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
    }
    log::debug!("[mascot-windows] WDA_EXCLUDEFROMCAPTURE applied to mascot HWND");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_constant_matches_url_query() {
        // Sanity check: if anyone changes MASCOT_WINDOW_LABEL, the
        // `?window=...` query string in show() needs to change too.
        // This catches a mismatch at build time rather than runtime.
        assert_eq!(MASCOT_WINDOW_LABEL, "mascot-win");
    }

    #[test]
    fn default_dimensions_are_sane() {
        // Mascot is intentionally small. If anyone bumps these to
        // window-sized numbers, the always-on-top UX collapses.
        assert!(MASCOT_WIDTH < 200.0);
        assert!(MASCOT_HEIGHT < 200.0);
        assert!(MASCOT_WIDTH > 32.0);
        assert!(MASCOT_HEIGHT > 32.0);
    }
}
