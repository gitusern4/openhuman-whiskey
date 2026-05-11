//! Persona memory cache — runtime injection of mode-specific markdown.
//!
//! When a mode's `additional_memory_roots()` returns one or more
//! directories, we pull the `.md` files in those directories into a
//! bounded text block on every LLM request and prepend it to the
//! system prompt (after the mode's own `system_prompt_prefix`).
//!
//! The cache is keyed by `(absolute file path, mtime)` so unchanged
//! files round-trip from memory; an external edit (the user's Claude
//! Code Whiskey skill writing to `whiskey_playbook.md`, say) bumps the
//! mtime and the next call re-reads.
//!
//! Bounded by design — a Whiskey playbook + pattern log + covenant
//! could easily push tens of thousands of tokens, which would blow the
//! context budget on every turn. Caps:
//!
//! - Per-file:  `MAX_PER_FILE_BYTES` of read content
//! - Skip:      files larger than `SKIP_FILES_OVER_BYTES` (don't even
//!              open them — too noisy to be useful in a prompt-prefix
//!              slot, and likely to hide valuable smaller files
//!              behind their bulk if we did include them)
//! - Total:     `MAX_TOTAL_BYTES` across all files (truncates the last
//!              included file mid-content rather than dropping it)
//!
//! The output is wrapped in a `## Mode Memory` section so the LLM can
//! tell where the memory block starts and stops.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

use once_cell::sync::Lazy;

use super::Mode;

/// Per-file content cap. Most Whiskey memory files are 1k–8k chars;
/// 4 KB is enough to capture the headers + a representative chunk
/// without dominating the prompt budget.
const MAX_PER_FILE_BYTES: usize = 4 * 1024;

/// Don't open files larger than this. A 1 MB markdown file is almost
/// certainly an export / dump that doesn't belong in every prompt; the
/// user can reference it explicitly via a future memory-search tool.
const SKIP_FILES_OVER_BYTES: u64 = 256 * 1024;

/// Total cap across all files. Roughly ~2k tokens budget at 4 chars
/// per token, which leaves headroom for the rest of the system prompt
/// + the user's message + the model's reply on standard 128k models.
const MAX_TOTAL_BYTES: usize = 16 * 1024;

/// One cached file: the truncated content + the mtime we read it at.
#[derive(Debug, Clone)]
struct CacheEntry {
    mtime: SystemTime,
    truncated_content: String,
}

/// Process-wide LRU-ish cache. `BTreeMap` keeps a deterministic file
/// order (alphabetical) so the assembled prompt is stable across
/// calls when the underlying files haven't changed — important for
/// Anthropic-style prompt-prefix caching downstream.
static CACHE: Lazy<Mutex<BTreeMap<PathBuf, CacheEntry>>> =
    Lazy::new(|| Mutex::new(BTreeMap::new()));

/// Resolve the persona memory block for the given mode, or `None` if
/// the mode has no memory roots / all roots are empty.
///
/// Cheap on the hot path when nothing changed: each file is `metadata`
/// + cache hit + clone of the cached string. Only files whose mtime
/// has moved get a re-read.
pub fn resolve(mode: &dyn Mode) -> Option<String> {
    let roots = mode.additional_memory_roots();
    if roots.is_empty() {
        return None;
    }

    let mut included: Vec<(PathBuf, String)> = Vec::new();
    let mut total_bytes: usize = 0;

    for root in &roots {
        let entries = match list_markdown_files(root) {
            Some(list) => list,
            None => continue,
        };
        for path in entries {
            if total_bytes >= MAX_TOTAL_BYTES {
                break;
            }
            let Some(content) = load_or_refresh(&path) else {
                continue;
            };
            let remaining = MAX_TOTAL_BYTES.saturating_sub(total_bytes);
            let trimmed = if content.len() > remaining {
                truncate_on_char_boundary(&content, remaining)
            } else {
                content
            };
            total_bytes = total_bytes.saturating_add(trimmed.len());
            included.push((path, trimmed));
        }
        if total_bytes >= MAX_TOTAL_BYTES {
            break;
        }
    }

    if included.is_empty() {
        return None;
    }

    Some(format_memory_block(mode, &included))
}

/// List every `.md` file directly inside `root` (no recursion), sorted
/// alphabetically. Recursion would let an unrelated nested project
/// pollute the prompt — flat listing matches how the Whiskey Claude
/// skill organises files (one directory, flat).
fn list_markdown_files(root: &Path) -> Option<Vec<PathBuf>> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(err) => {
            log::debug!(
                "[modes::memory_cache] read_dir({}) failed: {err}; skipping root",
                root.display()
            );
            return None;
        }
    };
    let mut out: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("md"))
                .unwrap_or(false)
        })
        .collect();
    out.sort();
    Some(out)
}

/// Cached load: returns the truncated content for `path`, refreshing
/// from disk if the mtime moved since the last read.
fn load_or_refresh(path: &Path) -> Option<String> {
    let metadata = match fs::metadata(path) {
        Ok(m) => m,
        Err(err) => {
            log::debug!(
                "[modes::memory_cache] metadata({}) failed: {err}",
                path.display()
            );
            return None;
        }
    };
    if metadata.len() > SKIP_FILES_OVER_BYTES {
        log::debug!(
            "[modes::memory_cache] skipping {} ({}B over cap)",
            path.display(),
            metadata.len()
        );
        return None;
    }
    let mtime = metadata.modified().ok()?;

    let mut cache = CACHE.lock().ok()?;
    if let Some(entry) = cache.get(path) {
        if entry.mtime == mtime {
            return Some(entry.truncated_content.clone());
        }
    }
    drop(cache); // release lock during disk IO

    let raw = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(err) => {
            log::debug!(
                "[modes::memory_cache] read_to_string({}) failed: {err}",
                path.display()
            );
            return None;
        }
    };
    let truncated = if raw.len() > MAX_PER_FILE_BYTES {
        truncate_on_char_boundary(&raw, MAX_PER_FILE_BYTES)
    } else {
        raw
    };

    let mut cache = CACHE.lock().ok()?;
    cache.insert(
        path.to_path_buf(),
        CacheEntry {
            mtime,
            truncated_content: truncated.clone(),
        },
    );
    Some(truncated)
}

/// Truncate `s` to at most `max_bytes` while staying on a UTF-8 char
/// boundary so we never emit invalid UTF-8.
fn truncate_on_char_boundary(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = String::with_capacity(end + 16);
    out.push_str(&s[..end]);
    out.push_str("\n[…truncated…]\n");
    out
}

/// Wrap the included files into one markdown block with file headers
/// so the LLM can tell which playbook section came from which file.
fn format_memory_block(mode: &dyn Mode, files: &[(PathBuf, String)]) -> String {
    let mut out = String::new();
    out.push_str(&format!("## {} Memory\n\n", mode.display_name()));
    out.push_str(
        "The following are persistent reference files for this mode. Treat them as \
         authoritative context the user has curated; quote file names when you cite \
         specific items.\n\n",
    );
    for (path, content) in files {
        let label = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("(unknown)");
        out.push_str(&format!("### `{label}`\n\n"));
        out.push_str(content.trim_end());
        out.push_str("\n\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openhuman::modes::WhiskeyMode;
    use std::fs::File;
    use std::io::Write;
    use std::path::PathBuf;
    use std::time::Duration;

    /// Minimal stub mode used to control which root the cache resolves
    /// against — the real WhiskeyMode points at the user's home dir,
    /// which is brittle in tests.
    struct StubMode {
        roots: Vec<PathBuf>,
    }

    impl Mode for StubMode {
        fn id(&self) -> &'static str {
            "test-stub"
        }
        fn display_name(&self) -> &str {
            "TestStub"
        }
        fn additional_memory_roots(&self) -> Vec<PathBuf> {
            self.roots.clone()
        }
    }

    fn unique_tmp_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(Duration::ZERO)
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("whiskey-memcache-{name}-{nanos}"));
        fs::create_dir_all(&dir).expect("create test dir");
        dir
    }

    #[test]
    fn resolve_returns_none_when_no_memory_roots() {
        let mode = StubMode { roots: vec![] };
        assert!(resolve(&mode).is_none());
    }

    #[test]
    fn resolve_returns_none_when_root_dir_does_not_exist() {
        let mode = StubMode {
            roots: vec![PathBuf::from("/nonexistent/whiskey-test-path")],
        };
        assert!(resolve(&mode).is_none());
    }

    #[test]
    fn resolve_returns_none_when_root_has_no_md_files() {
        let dir = unique_tmp_dir("no-md");
        // Plant a non-markdown file to confirm the .md filter works.
        fs::write(dir.join("notes.txt"), "ignored").expect("write txt");
        let mode = StubMode { roots: vec![dir] };
        assert!(resolve(&mode).is_none());
    }

    #[test]
    fn resolve_concatenates_md_files_with_headers() {
        let dir = unique_tmp_dir("concat");
        fs::write(dir.join("alpha.md"), "alpha body").expect("write alpha");
        fs::write(dir.join("beta.md"), "beta body").expect("write beta");
        let mode = StubMode {
            roots: vec![dir.clone()],
        };
        let block = resolve(&mode).expect("memory block");
        // Both filenames appear in headers.
        assert!(block.contains("### `alpha.md`"));
        assert!(block.contains("### `beta.md`"));
        // Both bodies appear.
        assert!(block.contains("alpha body"));
        assert!(block.contains("beta body"));
        // Mode display name is in the section header.
        assert!(block.contains("## TestStub Memory"));
    }

    #[test]
    fn resolve_truncates_files_larger_than_per_file_cap() {
        let dir = unique_tmp_dir("trunc-per-file");
        let huge = "a".repeat(MAX_PER_FILE_BYTES * 2);
        fs::write(dir.join("big.md"), &huge).expect("write big");
        let mode = StubMode { roots: vec![dir] };
        let block = resolve(&mode).expect("memory block");
        // The truncation marker must appear, the full original must not.
        assert!(block.contains("[…truncated…]"));
        assert!(block.len() < huge.len());
    }

    #[test]
    fn resolve_skips_files_larger_than_skip_threshold() {
        let dir = unique_tmp_dir("skip-huge");
        // Write a file larger than the skip threshold.
        let mut f = File::create(dir.join("colossal.md")).expect("create colossal");
        let chunk = vec![b'x'; 32 * 1024];
        for _ in 0..((SKIP_FILES_OVER_BYTES as usize / chunk.len()) + 1) {
            f.write_all(&chunk).expect("write chunk");
        }
        // Plant a small file alongside it so the result is non-empty.
        fs::write(dir.join("small.md"), "small body").expect("write small");
        let mode = StubMode { roots: vec![dir] };
        let block = resolve(&mode).expect("memory block");
        assert!(block.contains("### `small.md`"));
        // The colossal file's marker bytes must NOT appear.
        assert!(!block.contains("xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"));
    }

    #[test]
    fn resolve_re_reads_after_mtime_change() {
        let dir = unique_tmp_dir("mtime-refresh");
        let path = dir.join("evolving.md");
        fs::write(&path, "v1 content").expect("write v1");

        let mode = StubMode {
            roots: vec![dir.clone()],
        };
        let first = resolve(&mode).expect("first read");
        assert!(first.contains("v1 content"));

        // Sleep just enough for the OS mtime resolution to advance,
        // then rewrite. On Windows mtime is FILETIME (100ns); 50ms is
        // overkill but cheap.
        std::thread::sleep(Duration::from_millis(50));
        fs::write(&path, "v2 content").expect("write v2");

        let second = resolve(&mode).expect("second read");
        assert!(
            second.contains("v2 content"),
            "expected v2 after mtime change, got: {second}"
        );
        assert!(!second.contains("v1 content"));
    }

    #[test]
    fn truncate_on_char_boundary_preserves_utf8() {
        // Multi-byte char straddles position 1 (a one-byte 'a' + a 3-byte '€').
        let s = "a€bc";
        // Force max_bytes to land mid-codepoint at byte 2 (inside €).
        let truncated = truncate_on_char_boundary(s, 2);
        assert!(truncated.starts_with("a"));
        assert!(truncated.contains("[…truncated…]"));
        // No invalid UTF-8 panic just by reaching this assertion is enough.
    }

    #[test]
    fn whiskey_mode_resolve_is_callable_without_panicking() {
        // The real WhiskeyMode points at a path that may or may not
        // exist on the test runner — the function must not panic in
        // either case. The result will be None on most CI.
        let mode = WhiskeyMode::new();
        let _ = resolve(&mode);
    }
}
