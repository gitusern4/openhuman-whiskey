use crate::workspace::resolve_and_validate;
use std::path::{Path, PathBuf};

#[test]
fn resolve_and_validate_rejects_parent_traversal() {
    let result = resolve_and_validate("../escaped");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Path traversal denied");
}

#[test]
fn resolve_and_validate_accepts_relative_path() {
    let result = resolve_and_validate("valid/file.txt");
    assert!(result.is_ok());
}
