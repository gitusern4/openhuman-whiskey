use std::path::{Path, PathBuf};

use crate::file_logging;

fn get_workspace_root() -> PathBuf {
    file_logging::resolve_data_dir()
}

fn resolve_and_validate(path: &str) -> Result<PathBuf, String> {
    let root = get_workspace_root();

    // Support both relative paths and absolute paths that are inside the root
    let target = if Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        root.join(path)
    };

    // Prevent traversing outside workspace via ..
    if path.contains("..") {
        return Err("Path traversal denied".into());
    }

    // Attempt to canonicalize
    let canonical_root = root.canonicalize().unwrap_or(root.clone());
    let canonical_target = target.canonicalize().unwrap_or(target.clone());

    if !canonical_target.starts_with(&canonical_root) {
        return Err("Path is outside the workspace".into());
    }

    Ok(canonical_target)
}

#[tauri::command]
pub fn open_workspace_path(path: String) -> Result<(), String> {
    let resolved = resolve_and_validate(&path)?;
    if !resolved.exists() {
        return Err("Path does not exist".into());
    }

    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(&resolved).spawn();

    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("explorer")
        .arg(&resolved)
        .spawn();

    #[cfg(target_os = "linux")]
    let result = std::process::Command::new("xdg-open")
        .arg(&resolved)
        .spawn();

    result
        .map(|_| ())
        .map_err(|e| format!("failed to open path {}: {}", resolved.display(), e))
}

#[tauri::command]
pub fn reveal_workspace_path(path: String) -> Result<(), String> {
    let resolved = resolve_and_validate(&path)?;
    if !resolved.exists() {
        return Err("Path does not exist".into());
    }

    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open")
        .arg("-R")
        .arg(&resolved)
        .spawn();

    #[cfg(target_os = "windows")]
    let result = std::process::Command::new("explorer")
        .arg(format!("/select,{}", resolved.display()))
        .spawn();

    #[cfg(target_os = "linux")]
    let result = {
        let parent = resolved.parent().unwrap_or(&resolved);
        std::process::Command::new("xdg-open").arg(parent).spawn()
    };

    result
        .map(|_| ())
        .map_err(|e| format!("failed to reveal path {}: {}", resolved.display(), e))
}

#[tauri::command]
pub fn read_workspace_file_string(path: String) -> Result<String, String> {
    let resolved = resolve_and_validate(&path)?;
    if !resolved.exists() {
        return Err("Path does not exist".into());
    }
    if !resolved.is_file() {
        return Err("Path is not a file".into());
    }

    // Optionally check if file is text/markdown
    let ext = resolved.extension().and_then(|s| s.to_str()).unwrap_or("");
    match ext.to_lowercase().as_str() {
        "md" | "txt" | "json" | "csv" | "log" => {
            std::fs::read_to_string(&resolved).map_err(|e| e.to_string())
        }
        _ => Err("File extension not supported for safe preview".into()),
    }
}
