//! Persistence of the active mode selection across process restarts.
//!
//! The mode registry (see `super::registry`) keeps the active `Mode`
//! pointer behind a `RwLock` for the in-process hot path. That pointer
//! is rebuilt from `DefaultMode` on every boot, so without persistence a
//! user who picks `WhiskeyMode` in the settings UI would see the
//! selection reset to default after the next relaunch.
//!
//! This module mirrors the canonical TOML-persistence pattern used by
//! `app/src-tauri/src/window_state.rs`: lazy path resolution via the
//! configured openhuman root dir (`crate::openhuman::config::
//! default_root_openhuman_dir`), best-effort read/write, and warn-and-
//! skip on every failure mode. The persisted file lives at
//! `<openhuman_dir>/active_mode.toml` and contains a single record:
//!
//! ```toml
//! active_mode = "whiskey"
//! ```
//!
//! Both [`save`] and [`load`] are infallible from the caller's point of
//! view: `save` swallows errors with a warn-log, and `load` returns
//! `None` whenever the file is missing, malformed, or unreadable. That
//! way the registry's startup path always falls back to `DefaultMode`
//! cleanly when persistence is unavailable.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Generic atomic TOML store
// ---------------------------------------------------------------------------

/// Generic atomic TOML store. Writes to `<path>.tmp` then renames over
/// the destination so a crash mid-write cannot truncate the live file.
///
/// # Windows note
/// On Windows, `std::fs::rename` is implemented via `MoveFileExW` with
/// `MOVEFILE_REPLACE_EXISTING` since Rust 1.74 (stabilised in the
/// `windows-sys` backend). This is the correct atomic-replace primitive
/// on NTFS; no third-party crate is required.
///
/// # Best-effort contract
/// - `load`: missing file → `T::default()`; corrupt TOML → `T::default()` +
///   `log::warn`. Never panics, never returns an error.
/// - `save`: creates parent dir if needed; writes `.tmp`; atomically
///   renames. If rename fails, the `.tmp` is removed (best-effort) so
///   stale temps do not accumulate. Returns descriptive `Err(String)` on
///   failure; callers that want the old "swallow errors" behaviour can
///   call `.unwrap_or_else(|e| log::warn!(…))`.
pub struct AtomicTomlStore<T>
where
    T: serde::Serialize + serde::de::DeserializeOwned + Default,
{
    path: std::path::PathBuf,
    _marker: std::marker::PhantomData<T>,
}

impl<T> AtomicTomlStore<T>
where
    T: serde::Serialize + serde::de::DeserializeOwned + Default,
{
    /// Create a store that persists to `path`.
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        Self {
            path: path.into(),
            _marker: std::marker::PhantomData,
        }
    }

    /// Return the resolved path this store writes to.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }

    /// Load the value from disk.
    ///
    /// - File missing → `T::default()` (silent).
    /// - IO error → `T::default()` + `log::warn`.
    /// - Corrupt TOML → `T::default()` + `log::warn`.
    pub fn load(&self) -> T {
        let raw = match std::fs::read_to_string(&self.path) {
            Ok(r) => r,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return T::default();
            }
            Err(err) => {
                log::warn!(
                    "[AtomicTomlStore] read {} failed: {err}; returning default",
                    self.path.display()
                );
                return T::default();
            }
        };
        match toml::from_str::<T>(&raw) {
            Ok(v) => v,
            Err(err) => {
                log::warn!(
                    "[AtomicTomlStore] parse {} failed: {err}; returning default",
                    self.path.display()
                );
                T::default()
            }
        }
    }

    /// Atomically persist `value` to disk.
    ///
    /// Steps:
    /// 1. Create parent directory (and all ancestors) if missing.
    /// 2. Serialize `value` to TOML.
    /// 3. Write the TOML to `<path>.tmp`.
    /// 4. Rename `<path>.tmp` → `<path>` (atomic replace on NTFS/ext4).
    /// 5. On any error after step 3, remove the `.tmp` file (best-effort,
    ///    ignoring its own error) so stale temps don't accumulate.
    pub fn save(&self, value: &T) -> Result<(), String> {
        // 1. Create parent dir.
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                format!("[AtomicTomlStore] mkdir {} failed: {err}", parent.display())
            })?;
        }

        // 2. Serialize.
        let raw = toml::to_string_pretty(value)
            .map_err(|err| format!("[AtomicTomlStore] serialize failed: {err}"))?;

        // 3. Write to .tmp.
        let tmp_path = self.path.with_extension("tmp");
        std::fs::write(&tmp_path, &raw).map_err(|err| {
            format!(
                "[AtomicTomlStore] write tmp {} failed: {err}",
                tmp_path.display()
            )
        })?;

        // 4. Atomic rename: tmp → target.
        // On Windows, std::fs::rename maps to MoveFileExW(MOVEFILE_REPLACE_EXISTING)
        // since Rust 1.74, providing atomic overwrite on NTFS without any
        // additional crate dependency.
        if let Err(err) = std::fs::rename(&tmp_path, &self.path) {
            // 5. Clean up .tmp so we don't leave stale files behind.
            let _ = std::fs::remove_file(&tmp_path); // best-effort, ignore error
            return Err(format!(
                "[AtomicTomlStore] rename {} -> {} failed: {err}",
                tmp_path.display(),
                self.path.display()
            ));
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Active-mode persistence (site 1)
// ---------------------------------------------------------------------------

const STATE_FILE: &str = "active_mode.toml";

/// Env-var override used by the unit tests to redirect the persistence
/// file to a temp dir without touching `$HOME/.openhuman`. Production
/// callers never set this — when unset, [`state_path`] falls through to
/// `default_root_openhuman_dir()`.
const TEST_OVERRIDE_ENV: &str = "OPENHUMAN_ACTIVE_MODE_FILE";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ActiveModeState {
    #[serde(default)]
    active_mode: String,
}

fn state_path() -> Option<PathBuf> {
    if let Ok(override_path) = std::env::var(TEST_OVERRIDE_ENV) {
        if !override_path.is_empty() {
            return Some(PathBuf::from(override_path));
        }
    }
    match crate::openhuman::config::default_root_openhuman_dir() {
        Ok(root) => Some(root.join(STATE_FILE)),
        Err(err) => {
            log::warn!("[modes/persistence] no openhuman dir available: {err}");
            None
        }
    }
}

/// Persist the given mode id. Best-effort: any failure (path
/// resolution, mkdir, serialize, write) is logged at warn level and
/// swallowed. Never panics, never returns an error.
pub(crate) fn save(id: &str) {
    let Some(path) = state_path() else {
        return;
    };
    let store = AtomicTomlStore::<ActiveModeState>::new(&path);
    let state = ActiveModeState {
        active_mode: id.to_string(),
    };
    if let Err(err) = store.save(&state) {
        log::warn!("[modes/persistence] {err}");
    } else {
        log::info!("[modes/persistence] saved active_mode={id}");
    }
}

/// Read the persisted active mode id, if any.
///
/// Returns `None` for every failure mode (no path, missing file, IO
/// error, malformed TOML). Never panics. The caller (registry init) is
/// expected to validate the returned id against the registered mode set
/// and fall back to `DefaultMode` when the id is unknown.
pub(crate) fn load() -> Option<String> {
    let path = state_path()?;
    let store = AtomicTomlStore::<ActiveModeState>::new(&path);
    let state = store.load();
    // An empty id means either the file was missing/corrupt (AtomicTomlStore
    // returned Default) or the file contains an empty string. Either way the
    // registry should fall back to DefaultMode, so return None.
    if state.active_mode.is_empty() {
        None
    } else {
        Some(state.active_mode)
    }
}

/// Process-wide lock for tests — the env-var override is global state,
/// so any test that touches it (in this module OR in `super::registry`'s
/// tests, where `set_active_mode` triggers `save`) must serialize on
/// this single mutex. Exposed `pub(super)` so the registry tests can
/// share it.
#[cfg(test)]
pub(super) static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Env-var name re-exported for test-only use by sibling modules that
/// also need to redirect persistence to a temp file.
#[cfg(test)]
pub(super) const TEST_OVERRIDE_ENV_FOR_TESTS: &str = TEST_OVERRIDE_ENV;

#[cfg(test)]
mod tests {
    use super::*;

    /// RAII guard: sets the override env var on construct, clears it
    /// on drop. Ensures even a panicking test does not leak the
    /// override into a sibling test.
    struct EnvGuard;
    impl EnvGuard {
        fn set(path: &std::path::Path) -> Self {
            std::env::set_var(TEST_OVERRIDE_ENV, path);
            Self
        }
    }
    impl Drop for EnvGuard {
        fn drop(&mut self) {
            std::env::remove_var(TEST_OVERRIDE_ENV);
        }
    }

    // -----------------------------------------------------------------------
    // Legacy active-mode tests (preserved verbatim)
    // -----------------------------------------------------------------------

    #[test]
    fn round_trip_via_temp_dir_override() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("active_mode.toml");
        let _env = EnvGuard::set(&path);

        save("whiskey");
        assert!(path.exists(), "save should have written the file");
        let loaded = load();
        assert_eq!(loaded.as_deref(), Some("whiskey"));
    }

    #[test]
    fn missing_file_returns_none() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("does_not_exist.toml");
        let _env = EnvGuard::set(&path);

        assert!(load().is_none());
    }

    #[test]
    fn malformed_toml_returns_none() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("active_mode.toml");
        std::fs::write(&path, "this is not = valid toml ===\n[[[").unwrap();
        let _env = EnvGuard::set(&path);

        assert!(load().is_none());
    }

    #[test]
    fn save_creates_missing_parent_dir() {
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        // Two levels of missing directories — exercise create_dir_all.
        let nested = tmp.path().join("nested").join("deeper");
        let path = nested.join("active_mode.toml");
        assert!(!nested.exists());
        let _env = EnvGuard::set(&path);

        save("default");
        assert!(nested.exists(), "save should have created the parent dir");
        assert!(path.exists());
        assert_eq!(load().as_deref(), Some("default"));
    }

    // -----------------------------------------------------------------------
    // AtomicTomlStore<T> unit tests
    // -----------------------------------------------------------------------

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
    struct TestRecord {
        #[serde(default)]
        value: String,
        #[serde(default)]
        count: u32,
    }

    #[test]
    fn atomic_write_creates_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.toml");
        let store = AtomicTomlStore::<TestRecord>::new(&path);

        let record = TestRecord {
            value: "hello".to_string(),
            count: 42,
        };
        store.save(&record).expect("save should succeed");
        assert!(path.exists(), "file should exist after save");

        let loaded = store.load();
        assert_eq!(loaded, record);
    }

    #[test]
    fn atomic_write_replaces_existing() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.toml");
        let store = AtomicTomlStore::<TestRecord>::new(&path);

        let first = TestRecord {
            value: "first".to_string(),
            count: 1,
        };
        store.save(&first).unwrap();

        let second = TestRecord {
            value: "second".to_string(),
            count: 2,
        };
        store.save(&second).unwrap();

        let loaded = store.load();
        assert_eq!(loaded, second, "second save should overwrite first");
    }

    #[test]
    fn atomic_write_no_partial_file() {
        // Simulate a crash mid-write: write only to .tmp without renaming.
        // load() must still return the OLD value (or default if no prior write).
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.toml");
        let store = AtomicTomlStore::<TestRecord>::new(&path);

        // Write initial known-good state.
        let good = TestRecord {
            value: "good".to_string(),
            count: 99,
        };
        store.save(&good).unwrap();

        // Simulate a crash: write garbage to .tmp but don't rename.
        let tmp_path = path.with_extension("tmp");
        std::fs::write(&tmp_path, "garbage mid-write content {{{{").unwrap();

        // The live file is untouched; load should return the old good value.
        let loaded = store.load();
        assert_eq!(loaded, good, "live file must be unaffected by .tmp crash");
    }

    #[test]
    fn corrupt_file_returns_default() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.toml");
        std::fs::write(&path, "not valid toml ===\n[[[").unwrap();

        let store = AtomicTomlStore::<TestRecord>::new(&path);
        let loaded = store.load();
        assert_eq!(
            loaded,
            TestRecord::default(),
            "corrupt file must return default"
        );
    }

    #[test]
    fn missing_parent_dir_creates_it() {
        let tmp = tempfile::tempdir().unwrap();
        let nested = tmp.path().join("a").join("b").join("c");
        let path = nested.join("file.toml");
        assert!(!nested.exists());

        let store = AtomicTomlStore::<TestRecord>::new(&path);
        store
            .save(&TestRecord::default())
            .expect("save to new dir should succeed");
        assert!(
            path.exists(),
            "file should exist after creating parent dirs"
        );
    }

    #[test]
    fn tmp_file_not_left_behind_on_rename_failure() {
        // We can't easily force rename() to fail in a cross-platform way,
        // but we can verify the .tmp is gone after a successful save.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("test.toml");
        let store = AtomicTomlStore::<TestRecord>::new(&path);

        store
            .save(&TestRecord {
                value: "x".to_string(),
                count: 1,
            })
            .unwrap();

        let tmp_path = path.with_extension("tmp");
        assert!(
            !tmp_path.exists(),
            ".tmp file must be cleaned up after successful rename"
        );
    }

    #[test]
    fn env_var_override_path_is_used() {
        // Verify the active-mode module still honours TEST_OVERRIDE_ENV.
        let _g = TEST_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("override.toml");
        let _env = EnvGuard::set(&path);

        save("zeth");
        assert!(path.exists());
        assert_eq!(load().as_deref(), Some("zeth"));
    }
}
