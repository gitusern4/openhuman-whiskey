//! Tools added by the Whiskey fork.
//!
//! These live under `tools/whiskey/` rather than the upstream `tools/`
//! root so the fork's additions are isolated and easy to maintain across
//! upstream merges. Tools registered here are exposed to ALL modes (not
//! just `WhiskeyMode`) — the `Mode::tool_allowlist()` mechanism gates
//! per-mode visibility downstream.

pub mod image_gen_pollinations;
