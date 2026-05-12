//! Tools added by the Whiskey fork.
//!
//! These live under `tools/whiskey/` rather than the upstream `tools/`
//! root so the fork's additions are isolated and easy to maintain across
//! upstream merges. Tools registered here are exposed to ALL modes (not
//! just `WhiskeyMode`) — the `Mode::tool_allowlist()` mechanism gates
//! per-mode visibility downstream.

pub mod image_gen_pollinations;
pub mod image_gen_tool;
pub mod tv_chart;

pub use image_gen_tool::{ImageGenPollinationsTool, TOOL_NAME as IMAGE_GEN_TOOL_NAME};
pub use tv_chart::{
    TvChartStateTool, TvSetSymbolTool, GET_STATE_TOOL_NAME as TV_CHART_GET_STATE_TOOL_NAME,
    SET_SYMBOL_TOOL_NAME as TV_CHART_SET_SYMBOL_TOOL_NAME,
};
