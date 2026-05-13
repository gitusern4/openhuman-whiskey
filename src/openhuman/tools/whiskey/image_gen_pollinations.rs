//! Free image generation via Pollinations.ai.
//!
//! Pollinations exposes a no-auth GET endpoint that returns a PNG given
//! a prompt + size + seed:
//!     https://image.pollinations.ai/prompt/{url-encoded prompt}?width=W&height=H&seed=S&model=M&nologo=true
//!
//! Why Pollinations as the default:
//!  - Truly free, no API key, no rate-limit friction for personal use.
//!  - Works on ARM64 Windows without any local model setup (most local
//!    diffusion stacks lean on CUDA, which the user's machine lacks).
//!  - Good enough for "map out a concept" use cases — concept sketches,
//!    workflow diagrams, scratch visualisations.
//!
//! For higher-quality production work the user can layer on
//! HuggingFace Inference (free tier with API key) or fal.ai (free
//! credits) by setting the optional `provider` parameter on the tool —
//! the chain stays under one tool interface.

use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::Client;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Default model alias on Pollinations. As of May 2026 "flux" routes to
/// a Flux-derived backend that handles concept art and diagrams well.
/// Tested fallbacks: "flux-realism", "any-dark", "turbo".
const DEFAULT_MODEL: &str = "flux";

/// Hard upper bounds — Pollinations rejects bigger requests.
const MAX_DIMENSION: u32 = 1536;

/// Filename-safe prefix for saved generations.
const FILE_PREFIX: &str = "pollinations";

#[derive(Debug, Clone, Deserialize)]
pub struct ImageGenRequest {
    /// User-supplied prompt. No sanitising — Pollinations URL-encodes
    /// internally and rejects clearly disallowed content server-side.
    pub prompt: String,

    /// Width in px. Capped at [`MAX_DIMENSION`]. Default 1024.
    #[serde(default = "default_width")]
    pub width: u32,

    /// Height in px. Capped at [`MAX_DIMENSION`]. Default 1024.
    #[serde(default = "default_height")]
    pub height: u32,

    /// Optional seed for deterministic output. Default: random per call.
    #[serde(default)]
    pub seed: Option<u32>,

    /// Optional model alias override. Default: `"flux"`.
    #[serde(default)]
    pub model: Option<String>,

    /// Disk path override for saved file. INTERNAL ONLY — NOT exposed
    /// in the JSON-Schema for the LLM and rejected with an error if a
    /// caller tries to set it. WHISKEY_AUDIT.md H3 caught the original
    /// shape, where the field deserialized from arbitrary LLM JSON
    /// args and let the model write any byte stream to any path the
    /// process could reach (including `whiskey_playbook.md`,
    /// `active_mode.toml`, shell init files, …). Tests use this field
    /// via the in-crate `with_save_path` helper; agent calls always
    /// flow through the `default_save_dir` instead.
    #[serde(default, skip_deserializing)]
    pub save_path: Option<PathBuf>,
}

fn default_width() -> u32 {
    1024
}

fn default_height() -> u32 {
    1024
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageGenResponse {
    /// Absolute path to the saved PNG on disk.
    pub saved_path: PathBuf,
    /// Pollinations URL the image was fetched from — preserved for
    /// reproducibility and debugging.
    pub source_url: String,
    /// Reported byte size of the saved file.
    pub bytes: u64,
    /// Wall-clock generation latency.
    pub elapsed_ms: u128,
}

#[derive(Debug, Error)]
pub enum ImageGenError {
    #[error("prompt was empty")]
    EmptyPrompt,
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Pollinations returned non-success status: {status}")]
    BadStatus { status: u16 },
    #[error("filesystem error writing image: {0}")]
    Io(#[from] std::io::Error),
}

/// Generate an image. Saves to disk and returns metadata.
///
/// Defaults assume the caller is OK with disk writes under the app's
/// generated-images directory. Override via `request.save_path` for
/// tests or one-off destinations.
pub async fn generate(
    request: ImageGenRequest,
    default_save_dir: &Path,
) -> Result<ImageGenResponse, ImageGenError> {
    if request.prompt.trim().is_empty() {
        return Err(ImageGenError::EmptyPrompt);
    }

    let width = request.width.min(MAX_DIMENSION).max(64);
    let height = request.height.min(MAX_DIMENSION).max(64);
    let model = request.model.as_deref().unwrap_or(DEFAULT_MODEL);
    let url = build_url(&request.prompt, width, height, request.seed, model);

    let started = std::time::Instant::now();
    let client = Client::builder()
        // Pollinations can be slow on first call; 60s is generous.
        .timeout(Duration::from_secs(60))
        .build()?;
    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        return Err(ImageGenError::BadStatus {
            status: resp.status().as_u16(),
        });
    }
    let bytes = resp.bytes().await?;

    let saved_path = match request.save_path {
        Some(path) => path,
        None => {
            tokio::fs::create_dir_all(default_save_dir).await?;
            default_save_dir.join(generate_filename(&request.prompt, request.seed))
        }
    };
    tokio::fs::write(&saved_path, &bytes).await?;

    let elapsed_ms = started.elapsed().as_millis();
    let size_on_disk = bytes.len() as u64;

    log::info!(
        "[image_gen.pollinations] generated {} bytes in {}ms -> {}",
        size_on_disk,
        elapsed_ms,
        saved_path.display()
    );

    Ok(ImageGenResponse {
        saved_path,
        source_url: url,
        bytes: size_on_disk,
        elapsed_ms,
    })
}

fn build_url(prompt: &str, width: u32, height: u32, seed: Option<u32>, model: &str) -> String {
    let encoded = urlencoding::encode(prompt);
    let seed_param = seed.map(|s| format!("&seed={s}")).unwrap_or_default();
    format!(
        "https://image.pollinations.ai/prompt/{encoded}?width={width}&height={height}&model={model}&nologo=true{seed_param}"
    )
}

fn generate_filename(prompt: &str, seed: Option<u32>) -> String {
    // Slug the first 32 chars of the prompt for legibility, then add a
    // timestamp suffix so concurrent calls don't collide.
    let mut slug: String = prompt
        .chars()
        .take(32)
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        slug.push_str("image");
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let seed_str = seed.map(|s| format!("-{s}")).unwrap_or_default();
    format!("{FILE_PREFIX}-{ts}-{slug}{seed_str}.png")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_contains_all_required_params() {
        let url = build_url("a serene mountain", 1024, 768, Some(42), "flux");
        assert!(url.starts_with("https://image.pollinations.ai/prompt/"));
        assert!(url.contains("a%20serene%20mountain"));
        assert!(url.contains("width=1024"));
        assert!(url.contains("height=768"));
        assert!(url.contains("seed=42"));
        assert!(url.contains("model=flux"));
        assert!(url.contains("nologo=true"));
    }

    #[test]
    fn url_omits_seed_when_none() {
        let url = build_url("test", 512, 512, None, "flux");
        assert!(!url.contains("seed="));
    }

    #[test]
    fn filename_is_slug_safe() {
        let name = generate_filename("Trade Setup: MNQ short @ 21000!", Some(7));
        assert!(name.starts_with("pollinations-"));
        assert!(name.ends_with("-7.png"));
        // Slug should not contain disallowed chars.
        assert!(!name.contains(' '));
        assert!(!name.contains('@'));
        assert!(!name.contains('!'));
        assert!(!name.contains(':'));
    }

    #[test]
    fn empty_prompt_filename_uses_fallback() {
        let name = generate_filename("!!!!", None);
        assert!(name.contains("image"));
    }

    #[tokio::test]
    async fn empty_prompt_rejected() {
        let req = ImageGenRequest {
            prompt: "   ".into(),
            width: 1024,
            height: 1024,
            seed: None,
            model: None,
            save_path: None,
        };
        let dir = std::env::temp_dir().join("openhuman-image-gen-test");
        let err = generate(req, &dir).await.unwrap_err();
        assert!(matches!(err, ImageGenError::EmptyPrompt));
    }
}
