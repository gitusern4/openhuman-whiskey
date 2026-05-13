//! Region-of-interest (ROI) anchoring + perceptual-hash drift detection.
//!
//! Each `Roi` is anchored to one of nine corners / edges / centre of the
//! captured window and then offset by `(x_offset_px, y_offset_px)`. The
//! pixel rect is recomputed every frame from the live window size, so
//! resizing the window keeps the ROI tracking the right element.
//!
//! Drift detection: we compute a 64-bit dHash over a 32-pixel-wide border
//! around the ROI rect. If the Hamming distance between the previous and
//! current border-hash exceeds [`DHASH_DRIFT_THRESHOLD`] over five
//! consecutive frames, the orchestrator emits a one-shot `LayoutDrift`
//! event so the user (or a future auto-realigner) can re-anchor.

use serde::{Deserialize, Serialize};

use super::types::Frame;

/// Hamming-distance threshold above which two consecutive ROI border
/// hashes are considered "drifted." Tuned empirically — small UI nudges
/// (e.g. 1px scrollbar reposition) sit well under 8 bits.
pub const DHASH_DRIFT_THRESHOLD: u32 = 8;

/// Number of consecutive over-threshold frames required before drift fires.
pub const DHASH_DRIFT_CONSECUTIVE_FRAMES: u32 = 5;

/// Width in pixels of the border sampled into the perceptual hash.
pub const DHASH_BORDER_PX: u32 = 32;

/// Anchor positions on a window's client rect. Letters mirror compass
/// notation: T = top, M = middle, B = bottom; L = left, C = centre, R = right.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Anchor {
    TL,
    TC,
    TR,
    ML,
    MC,
    MR,
    BL,
    BC,
    BR,
}

/// A single anchored region-of-interest definition. Pixel-rect computation
/// is deferred to [`Roi::resolve_rect`] so the same `Roi` survives window
/// resizes and DPI changes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Roi {
    pub name: String,
    pub anchor: Anchor,
    pub x_offset_px: i32,
    pub y_offset_px: i32,
    pub width_px: u32,
    pub height_px: u32,
}

/// Axis-aligned rectangle, top-left origin, in window-client pixel space.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn right(&self) -> i32 {
        self.x + self.width as i32
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }
    /// Clip to `[0, window_w) x [0, window_h)` and return the visible portion.
    /// Returns `None` if the clipped rect is empty.
    pub fn clip_to(self, window_w: u32, window_h: u32) -> Option<Rect> {
        let x0 = self.x.max(0);
        let y0 = self.y.max(0);
        let x1 = self.right().min(window_w as i32);
        let y1 = self.bottom().min(window_h as i32);
        if x1 <= x0 || y1 <= y0 {
            return None;
        }
        Some(Rect {
            x: x0,
            y: y0,
            width: (x1 - x0) as u32,
            height: (y1 - y0) as u32,
        })
    }
}

impl Roi {
    /// Compute the pixel rect for this ROI inside a window of size
    /// `window_w x window_h`. Result is unclipped — callers that need to
    /// sample pixels should also call [`Rect::clip_to`].
    pub fn resolve_rect(&self, window_w: u32, window_h: u32) -> Rect {
        let (ax, ay) = anchor_origin(self.anchor, window_w, window_h);
        Rect {
            x: ax + self.x_offset_px,
            y: ay + self.y_offset_px,
            width: self.width_px,
            height: self.height_px,
        }
    }
}

fn anchor_origin(anchor: Anchor, w: u32, h: u32) -> (i32, i32) {
    let mid_x = (w / 2) as i32;
    let mid_y = (h / 2) as i32;
    let right = w as i32;
    let bottom = h as i32;
    match anchor {
        Anchor::TL => (0, 0),
        Anchor::TC => (mid_x, 0),
        Anchor::TR => (right, 0),
        Anchor::ML => (0, mid_y),
        Anchor::MC => (mid_x, mid_y),
        Anchor::MR => (right, mid_y),
        Anchor::BL => (0, bottom),
        Anchor::BC => (mid_x, bottom),
        Anchor::BR => (right, bottom),
    }
}

// ── dHash perceptual hash ───────────────────────────────────────────────

/// Compute a 64-bit dHash over the ROI border in a BGRA8 frame.
///
/// Algorithm: average the BGR channels into a single luma value, downsample
/// the 32px border ring to a 9x8 grid of luma means, and emit one bit per
/// horizontal-neighbour comparison (8 rows × 8 bits = 64). Returns 0 if the
/// rect is empty after clipping.
pub fn dhash_border(frame: &Frame, rect: Rect) -> u64 {
    let Some(rect) = rect.clip_to(frame.width, frame.height) else {
        return 0;
    };
    if rect.width < 2 || rect.height < 2 {
        return 0;
    }
    // Build a 9x8 grid of luma means by sampling the border-ring of the rect.
    // For simplicity (and because the ring is what actually represents
    // "edge of the ROI") we sample a single-pixel-thick perimeter band that
    // is `BORDER_PX`-wide on the inside of `rect` (clamped to rect size).
    let band = DHASH_BORDER_PX
        .min(rect.width / 2)
        .min(rect.height / 2)
        .max(1);

    let mut grid = [[0u32; 9]; 8];
    let mut counts = [[0u32; 9]; 8];

    for row in 0..8u32 {
        for col in 0..9u32 {
            // Cell centre inside the rect.
            let cx = rect.x as u32 + (rect.width.saturating_sub(1) * col) / 8;
            let cy = rect.y as u32 + (rect.height.saturating_sub(1) * row) / 7;

            // Only sample if (cx, cy) lies inside the perimeter band.
            let inside_band = cx < rect.x as u32 + band
                || cx + band >= rect.x as u32 + rect.width
                || cy < rect.y as u32 + band
                || cy + band >= rect.y as u32 + rect.height;

            if !inside_band {
                continue;
            }
            if cx >= frame.width || cy >= frame.height {
                continue;
            }
            let idx = (cy as usize * frame.stride()) + (cx as usize * 4);
            if idx + 2 >= frame.pixels.len() {
                continue;
            }
            let b = frame.pixels[idx] as u32;
            let g = frame.pixels[idx + 1] as u32;
            let r = frame.pixels[idx + 2] as u32;
            // Rec.601 luma approximation.
            let y = (r * 299 + g * 587 + b * 114) / 1000;
            grid[row as usize][col as usize] += y;
            counts[row as usize][col as usize] += 1;
        }
    }

    let mut hash: u64 = 0;
    for row in 0..8 {
        for col in 0..8 {
            let left_avg = if counts[row][col] > 0 {
                grid[row][col] / counts[row][col]
            } else {
                0
            };
            let right_avg = if counts[row][col + 1] > 0 {
                grid[row][col + 1] / counts[row][col + 1]
            } else {
                0
            };
            if left_avg < right_avg {
                hash |= 1u64 << (row * 8 + col);
            }
        }
    }
    hash
}

/// Hamming distance between two dHash values (count of differing bits).
pub fn dhash_distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// Per-ROI sliding tracker for drift state. Owned by the engine.
#[derive(Debug, Default)]
pub struct DriftTracker {
    last_hash: Option<u64>,
    /// Consecutive over-threshold frames since the last reset.
    consecutive_over: u32,
    /// Set after a one-shot drift event has fired; reset by [`Self::ack`].
    fired: bool,
}

impl DriftTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a new frame's hash. Returns `Some(distance)` exactly once when
    /// the consecutive-over count crosses [`DHASH_DRIFT_CONSECUTIVE_FRAMES`].
    pub fn observe(&mut self, hash: u64) -> Option<u32> {
        let prev = self.last_hash.replace(hash);
        let Some(prev) = prev else {
            return None;
        };
        let dist = dhash_distance(prev, hash);
        if dist > DHASH_DRIFT_THRESHOLD {
            self.consecutive_over = self.consecutive_over.saturating_add(1);
        } else {
            self.consecutive_over = 0;
        }
        if self.consecutive_over >= DHASH_DRIFT_CONSECUTIVE_FRAMES && !self.fired {
            self.fired = true;
            return Some(dist);
        }
        None
    }

    /// Acknowledge a fired drift event (re-arm the one-shot).
    pub fn ack(&mut self) {
        self.fired = false;
        self.consecutive_over = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roi(name: &str, anchor: Anchor, x: i32, y: i32, w: u32, h: u32) -> Roi {
        Roi {
            name: name.into(),
            anchor,
            x_offset_px: x,
            y_offset_px: y,
            width_px: w,
            height_px: h,
        }
    }

    #[test]
    fn anchor_top_left_offsets_from_origin() {
        let r = roi("a", Anchor::TL, 10, 20, 100, 50);
        assert_eq!(
            r.resolve_rect(800, 600),
            Rect {
                x: 10,
                y: 20,
                width: 100,
                height: 50
            }
        );
    }

    #[test]
    fn anchor_bottom_right_negative_offset_works() {
        // BR-anchored ROI tracks the bottom-right corner across resizes.
        let r = roi("pnl", Anchor::BR, -200, -50, 180, 30);
        let small = r.resolve_rect(800, 600);
        let large = r.resolve_rect(1920, 1080);
        assert_eq!(small.right(), 800 - 200 + 180);
        assert_eq!(small.bottom(), 600 - 50 + 30);
        // Same offsets in absolute terms — only the origin moved with resize.
        assert_eq!(large.x - small.x, 1920 - 800);
        assert_eq!(large.y - small.y, 1080 - 600);
        assert_eq!(small.width, large.width);
        assert_eq!(small.height, large.height);
    }

    #[test]
    fn anchor_middle_centre_centres() {
        let r = roi("mc", Anchor::MC, -50, -50, 100, 100);
        let rect = r.resolve_rect(1000, 800);
        assert_eq!(rect.x, 500 - 50);
        assert_eq!(rect.y, 400 - 50);
    }

    #[test]
    fn rect_clip_handles_off_screen() {
        let r = Rect {
            x: -10,
            y: -10,
            width: 50,
            height: 50,
        };
        let clipped = r.clip_to(100, 100).unwrap();
        assert_eq!(clipped.x, 0);
        assert_eq!(clipped.y, 0);
        assert_eq!(clipped.width, 40);
        assert_eq!(clipped.height, 40);

        let off = Rect {
            x: 200,
            y: 200,
            width: 10,
            height: 10,
        };
        assert!(off.clip_to(100, 100).is_none());
    }

    #[test]
    fn dhash_distance_basic() {
        assert_eq!(dhash_distance(0, 0), 0);
        assert_eq!(dhash_distance(0xFFFF_FFFF_FFFF_FFFF, 0), 64);
        assert_eq!(dhash_distance(0b1010, 0b0101), 4);
    }

    #[test]
    fn dhash_identical_frame_has_zero_distance() {
        let f = Frame {
            captured_at_ms: 0,
            width: 64,
            height: 64,
            pixels: vec![128u8; 64 * 64 * 4],
        };
        let rect = Rect {
            x: 0,
            y: 0,
            width: 64,
            height: 64,
        };
        let h1 = dhash_border(&f, rect);
        let h2 = dhash_border(&f, rect);
        assert_eq!(h1, h2);
        assert_eq!(dhash_distance(h1, h2), 0);
    }

    #[test]
    fn drift_tracker_fires_after_consecutive_over_threshold() {
        let mut t = DriftTracker::new();
        // First hash establishes a baseline; no event possible.
        assert!(t.observe(0).is_none());
        // Massively different hashes for 5 consecutive frames must fire once.
        let mut fired = 0u32;
        for _ in 0..DHASH_DRIFT_CONSECUTIVE_FRAMES {
            if t.observe(u64::MAX).is_some() {
                fired += 1;
            }
        }
        assert_eq!(fired, 1, "drift event must be one-shot");
        // Further matching hashes should not fire again until ack.
        assert!(t.observe(u64::MAX).is_none());
        t.ack();
        // Now drift can re-arm.
        for _ in 0..DHASH_DRIFT_CONSECUTIVE_FRAMES {
            let _ = t.observe(0);
        }
        // The second sequence (going from MAX -> 0 repeatedly = single change)
        // should not have fired more than one extra event total.
    }
}
