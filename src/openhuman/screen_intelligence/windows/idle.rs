//! User-idle detection via `GetLastInputInfo`.
//!
//! Polled at 1 Hz from the capture thread. When the system has been idle
//! longer than [`IDLE_THRESHOLD_MS`] AND the captured trading window is no
//! longer the foreground window, the engine throttles capture from 2 Hz
//! down to 0.2 Hz. Any input or focus return resumes 2 Hz on the next tick.
//!
//! `GetTickCount` wraps every ~49.7 days; the comparison here uses
//! `wrapping_sub`, which is the documented Microsoft pattern.

/// Idle threshold above which we consider the user "away" (5 minutes).
pub const IDLE_THRESHOLD_MS: u32 = 5 * 60 * 1000;

/// Compute idle milliseconds given the system's `GetTickCount` value (`now_tick`)
/// and the `dwTime` field from `LASTINPUTINFO` (`last_input_tick`). Both are
/// 32-bit and wrap; use `wrapping_sub` per Microsoft docs.
///
/// Pure function, target-agnostic, so tests can validate edge cases on
/// any host.
pub fn idle_ms_from_ticks(now_tick: u32, last_input_tick: u32) -> u32 {
    now_tick.wrapping_sub(last_input_tick)
}

/// `true` when the system has been idle ≥ [`IDLE_THRESHOLD_MS`].
pub fn is_idle(now_tick: u32, last_input_tick: u32) -> bool {
    idle_ms_from_ticks(now_tick, last_input_tick) >= IDLE_THRESHOLD_MS
}

/// Live idle probe — calls `GetLastInputInfo` + `GetTickCount`. Returns
/// `None` if the Win32 call fails (extremely rare in practice).
#[cfg(target_os = "windows")]
pub fn current_idle_ms() -> Option<u32> {
    use ::windows::Win32::System::SystemInformation::GetTickCount;
    use ::windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};

    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    // SAFETY: `info` is fully initialised; cbSize equals the actual struct size.
    let ok = unsafe { GetLastInputInfo(&mut info) };
    if !ok.as_bool() {
        return None;
    }
    let now = unsafe { GetTickCount() };
    Some(idle_ms_from_ticks(now, info.dwTime))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_ms_simple() {
        assert_eq!(idle_ms_from_ticks(10_000, 9_000), 1_000);
    }

    #[test]
    fn idle_ms_wraps_correctly_at_u32_boundary() {
        // System ticks just rolled over: now=100, last_input_was = u32::MAX - 50
        // Expected idle = 50 + 100 + 1 = 151.
        let now = 100u32;
        let last = u32::MAX - 50;
        assert_eq!(idle_ms_from_ticks(now, last), 151);
        // And it's still well under the threshold.
        assert!(!is_idle(now, last));
    }

    #[test]
    fn is_idle_threshold_inclusive() {
        let now = IDLE_THRESHOLD_MS;
        assert!(is_idle(now, 0));
        assert!(!is_idle(now - 1, 0));
    }
}
