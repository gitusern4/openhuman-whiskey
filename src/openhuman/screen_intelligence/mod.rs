//! Screen capture, accessibility automation, and vision summaries (macOS-focused).

pub(crate) mod cli;
pub mod ops;
mod schemas;
pub mod server;

mod capture;
mod capture_worker;
mod engine;

mod helpers;
mod image_processing;
mod input;
mod limits;
mod permissions;
mod processing_worker;
mod state;
mod types;
mod vision;
/// Windows screen-watch submodule — WGC capture, anchored ROIs,
/// Tesseract OCR, idle gating. macOS / Linux builds skip this entirely
/// via the cfg gate.
///
/// WHISKEY_AUDIT.md M3: was `pub mod windows`, exposing every internal
/// type (Frame, Roi, Anchor, EngineError, EngineConfig, OcrError,
/// TradingEvent, etc.) as crate-public despite the module being
/// explicit STUBS. `pub(crate)` matches every sibling module here and
/// keeps consumer-visible API churn minimal as the implementation
/// lands. All current callers (the module's own tests + future
/// in-crate Tauri commands) compile fine under crate visibility.
#[cfg(target_os = "windows")]
pub(crate) mod windows;

pub use ops as rpc;
pub use ops::*;
pub use schemas::{
    all_controller_schemas as all_screen_intelligence_controller_schemas,
    all_registered_controllers as all_screen_intelligence_registered_controllers,
};
pub use state::{global_engine, AccessibilityEngine};
pub use types::*;

#[cfg(test)]
mod tests;
