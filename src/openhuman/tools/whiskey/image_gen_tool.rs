//! `Tool` impl wrapping the pure-functional Pollinations image generator.
//!
//! Registers as `image_gen_pollinations`. The agent calls it with
//! `{ prompt, width?, height?, seed?, model? }`. The tool generates the
//! image, saves to `<workspace>/.openhuman/generated_images/`, and
//! returns a `ToolResult` that includes the saved-file path + source
//! URL + bytes + elapsed time as JSON, so the agent can quote the
//! result back at the user.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::openhuman::skills::types::ToolContent;
use crate::openhuman::tools::traits::{PermissionLevel, Tool, ToolCategory, ToolResult};
use crate::openhuman::tools::whiskey::image_gen_pollinations::{
    generate, ImageGenError, ImageGenRequest,
};

/// Tool name as seen by the LLM and by [`crate::openhuman::modes`]
/// allowlists. Match this string exactly when adding to a `tool_allowlist`.
pub const TOOL_NAME: &str = "image_gen_pollinations";

/// Subdirectory under the workspace where generated images are saved.
const SUBDIR: &str = ".openhuman/generated_images";

/// Tool wrapper. Holds the workspace dir so the generator knows where
/// to write images. The LLM does not get to pick the save path —
/// `save_path` is `skip_deserializing` on the request struct AND
/// not exposed in the JSON-Schema below AND blocked by
/// `additionalProperties: false`. Defense-in-depth, per
/// WHISKEY_AUDIT.md H3 (LLM-controlled write paths are an attack
/// surface; combined with prompt-injection in playbook .md files
/// they could overwrite the playbook itself, `active_mode.toml`,
/// or shell init files).
pub struct ImageGenPollinationsTool {
    save_dir: Arc<PathBuf>,
}

impl ImageGenPollinationsTool {
    pub fn new(workspace_dir: PathBuf) -> Self {
        Self {
            save_dir: Arc::new(workspace_dir.join(SUBDIR)),
        }
    }
}

#[async_trait]
impl Tool for ImageGenPollinationsTool {
    fn name(&self) -> &str {
        TOOL_NAME
    }

    fn description(&self) -> &str {
        "Generate an image from a text prompt via the free Pollinations.ai \
         API. No API key required. Returns the saved file path. Useful for \
         concept maps, workflow diagrams, scratch sketches. Default model is \
         flux; default size 1024x1024 (max 1536). Provide a seed for \
         deterministic output."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "What to draw. Pass plain English; the tool URL-encodes."
                },
                "width": {
                    "type": "integer",
                    "minimum": 64,
                    "maximum": 1536,
                    "default": 1024,
                    "description": "Image width in pixels."
                },
                "height": {
                    "type": "integer",
                    "minimum": 64,
                    "maximum": 1536,
                    "default": 1024,
                    "description": "Image height in pixels."
                },
                "seed": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Deterministic seed. Omit for random."
                },
                "model": {
                    "type": "string",
                    "description": "Pollinations model alias (default 'flux'). Other tested values: 'flux-realism', 'turbo', 'any-dark'."
                }
            },
            "required": ["prompt"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let request: ImageGenRequest =
            serde_json::from_value(args).map_err(|e| anyhow::anyhow!("invalid arguments: {e}"))?;
        match generate(request, &self.save_dir).await {
            Ok(resp) => {
                let payload = json!({
                    "saved_path": resp.saved_path.display().to_string(),
                    "source_url": resp.source_url,
                    "bytes": resp.bytes,
                    "elapsed_ms": resp.elapsed_ms,
                });
                Ok(ToolResult {
                    content: vec![ToolContent::Text {
                        text: payload.to_string(),
                    }],
                    is_error: false,
                    markdown_formatted: None,
                })
            }
            Err(err) => {
                let msg = match &err {
                    ImageGenError::EmptyPrompt => "prompt was empty".to_string(),
                    ImageGenError::Http(e) => format!("network error: {e}"),
                    ImageGenError::BadStatus { status } => {
                        format!("Pollinations returned HTTP {status}")
                    }
                    ImageGenError::Io(e) => format!("filesystem error: {e}"),
                };
                Ok(ToolResult {
                    content: vec![ToolContent::Text { text: msg }],
                    is_error: true,
                    markdown_formatted: None,
                })
            }
        }
    }

    fn permission_level(&self) -> PermissionLevel {
        // The tool reaches the network (Pollinations.ai) AND writes
        // to disk under the workspace's generated_images subdir. The
        // permission system has no `Network` variant; `Write` is the
        // closest fit and correctly gates against tools that must
        // not write at all.
        //
        // WHISKEY_AUDIT.md L14: previous comment claimed "no host
        // writes other than under generated_images" — true only
        // because save_path is now skip_deserializing (H3 fix). With
        // an LLM-controllable save_path the claim was false, hence
        // the lockdown in commit 58716e0f.
        PermissionLevel::Write
    }

    fn category(&self) -> ToolCategory {
        // Talks to an external service (image.pollinations.ai), so this
        // is a Skill tool by the project's classification.
        ToolCategory::Skill
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_tool() -> ImageGenPollinationsTool {
        ImageGenPollinationsTool::new(std::env::temp_dir())
    }

    #[test]
    fn name_matches_constant() {
        assert_eq!(make_tool().name(), TOOL_NAME);
        assert_eq!(TOOL_NAME, "image_gen_pollinations");
    }

    #[test]
    fn parameters_schema_requires_prompt() {
        let schema = make_tool().parameters_schema();
        let required = schema
            .get("required")
            .and_then(Value::as_array)
            .expect("schema has required array");
        assert!(required.iter().any(|v| v.as_str() == Some("prompt")));
    }

    #[tokio::test]
    async fn empty_prompt_returns_error_result() {
        let result = make_tool()
            .execute(json!({ "prompt": "   " }))
            .await
            .expect("tool returned a ToolResult");
        assert!(result.is_error);
    }

    #[test]
    fn permission_level_is_write_not_dangerous() {
        // A Pollinations call is network + workspace-write; not host
        // shell exec. Make sure we don't accidentally promote it.
        assert_eq!(make_tool().permission_level(), PermissionLevel::Write);
    }

    #[test]
    fn category_is_skill_not_system() {
        assert_eq!(make_tool().category(), ToolCategory::Skill);
    }
}
