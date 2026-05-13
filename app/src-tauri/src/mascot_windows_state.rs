//! Persistence of the Windows mascot window's position + size.
//!
//! Mirrors [`crate::window_state`]'s pattern (TOML record under the
//! shared `<openhuman_dir>/`) but lives in its own file
//! `mascot_windows_state.toml` so the main window's state and the
//! mascot's state never stomp on each other.
//!
//! Off-screen / malformed records fall through to the bottom-right
//! default placement in [`crate::mascot_windows_window`], so we can't
//! strand the mascot in an unreachable monitor.

#![cfg(target_os = "windows")]

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::cef_profile;

const STATE_FILE: &str = "mascot_windows_state.toml";

/// Persisted geometry for the mascot window. `width`/`height` are
/// recorded for forward-compat (a future "resizable mascot" mode could
/// honour them); today the mascot is fixed-size and they round-trip as
/// the constants from `mascot_windows_window`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct MascotWindowsState {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl MascotWindowsState {
    /// Crude sanity check used by the load path so a corrupt file (zero
    /// dimensions, comically large dimensions) doesn't make the mascot
    /// invisible or off-screen.
    pub(crate) fn has_sane_dimensions(&self) -> bool {
        self.width >= 32 && self.height >= 32 && self.width <= 4096 && self.height <= 4096
    }
}

fn state_path() -> Option<PathBuf> {
    cef_profile::default_root_openhuman_dir()
        .ok()
        .map(|root| root.join(STATE_FILE))
}

/// Best-effort load of the persisted record. Returns `None` on every
/// kind of failure (missing file, IO error, parse error) — the caller
/// is expected to fall back to the default bottom-right placement.
pub(crate) fn load() -> Option<MascotWindowsState> {
    let path = state_path()?;
    let raw = std::fs::read_to_string(&path).ok()?;
    match toml::from_str::<MascotWindowsState>(&raw) {
        Ok(state) => {
            log::debug!(
                "[mascot-windows-state] loaded x={} y={} w={} h={}",
                state.x,
                state.y,
                state.width,
                state.height
            );
            Some(state)
        }
        Err(err) => {
            log::warn!(
                "[mascot-windows-state] parse {} failed: {err}; using default placement",
                path.display()
            );
            None
        }
    }
}

/// Persist the four numbers. Best-effort — IO errors are logged at
/// `warn` and dropped (the mascot UI continues to function, the user
/// just won't see their last position remembered after a restart).
pub(crate) fn save_state(x: i32, y: i32, width: u32, height: u32) {
    let state = MascotWindowsState {
        x,
        y,
        width,
        height,
    };
    let Some(path) = state_path() else {
        log::warn!("[mascot-windows-state] no path available; skip save");
        return;
    };
    if let Some(parent) = path.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            log::warn!(
                "[mascot-windows-state] mkdir {} failed: {err}; skip save",
                parent.display()
            );
            return;
        }
    }
    let raw = match toml::to_string_pretty(&state) {
        Ok(r) => r,
        Err(err) => {
            log::warn!("[mascot-windows-state] serialize failed: {err}; skip save");
            return;
        }
    };
    if let Err(err) = std::fs::write(&path, raw) {
        log::warn!(
            "[mascot-windows-state] write {} failed: {err}",
            path.display()
        );
    } else {
        log::info!("[mascot-windows-state] saved x={x} y={y} w={width} h={height}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_sane_dimensions_accepts_normal() {
        let state = MascotWindowsState {
            x: 0,
            y: 0,
            width: 96,
            height: 96,
        };
        assert!(state.has_sane_dimensions());
    }

    #[test]
    fn has_sane_dimensions_rejects_zero() {
        let state = MascotWindowsState {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
        assert!(!state.has_sane_dimensions());
    }

    #[test]
    fn has_sane_dimensions_rejects_giant() {
        let state = MascotWindowsState {
            x: 0,
            y: 0,
            width: 99999,
            height: 99999,
        };
        assert!(!state.has_sane_dimensions());
    }

    #[test]
    fn round_trip_through_toml() {
        let state = MascotWindowsState {
            x: 1234,
            y: -56,
            width: 96,
            height: 96,
        };
        let raw = toml::to_string_pretty(&state).expect("serialize ok");
        let parsed: MascotWindowsState = toml::from_str(&raw).expect("parse ok");
        assert_eq!(parsed, state);
    }
}
