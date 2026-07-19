//! Shared testing utilities for rs-cli-tmpl integration tests.

use assert_cmd::Command;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Testing harness providing an isolated HOME/workspace pair for CLI and SDK exercises.
#[allow(dead_code)]
pub struct TestContext {
    root: TempDir,
    work_dir: PathBuf,
    original_home: Option<OsString>,
}

#[allow(dead_code)]
impl TestContext {
    /// Create a new isolated environment and point `HOME` to it so the CLI uses local storage.
    pub fn new() -> Self {
        let root = TempDir::new().expect("Failed to create temp directory for tests");
        let work_dir = root.path().join("work");
        fs::create_dir_all(&work_dir).expect("Failed to create test work directory");

        let original_home = env::var_os("HOME");
        // SAFETY: Tests are serialized via #[serial]. No other threads or child processes
        // concurrently read or modify the process environment during the test. Restoration
        // is performed deterministically in Drop. env::set_var is unsafe in Rust 2024.
        unsafe {
            env::set_var("HOME", root.path());
        }

        Self { root, work_dir, original_home }
    }

    /// Absolute path to the emulated `$HOME` directory.
    pub fn home(&self) -> &Path {
        self.root.path()
    }

    /// Path to the workspace directory used for CLI invocations.
    pub fn work_dir(&self) -> &Path {
        &self.work_dir
    }

    /// Convenience helper to create additional sibling workspaces (e.g., for linking scenarios).
    pub fn create_workspace(&self, name: &str) -> PathBuf {
        let path = self.home().join(name);
        fs::create_dir_all(&path).expect("Failed to create additional workspace");
        path
    }

    /// Populate the default workspace with an item file containing the provided contents.
    pub fn write_item_file(&self, contents: &str) {
        let item_path = self.work_dir().join("item.txt");
        fs::write(&item_path, contents).expect("Failed to write item file for test");
    }

    /// Create an item file in the given directory with the provided contents.
    pub fn write_item_file_in<P: AsRef<Path>>(&self, dir: P, contents: &str) {
        let path = dir.as_ref().join("item.txt");
        fs::write(path, contents).expect("Failed to write item file");
    }

    /// Build a command for invoking the compiled `rs-cli-tmpl` binary within the default workspace.
    pub fn cli(&self) -> Command {
        self.cli_in(self.work_dir())
    }

    /// Build a command for invoking the compiled `rs-cli-tmpl` binary within a custom directory.
    pub fn cli_in<P: AsRef<Path>>(&self, dir: P) -> Command {
        let mut cmd =
            Command::cargo_bin("rs-cli-tmpl").expect("Failed to locate rs-cli-tmpl binary");
        cmd.current_dir(dir.as_ref()).env("HOME", self.home());
        cmd
    }

    /// Return the path where the CLI stores a saved item file for the provided identifier.
    pub fn saved_item_path(&self, id: &str) -> PathBuf {
        self.home().join(".config").join("rs-cli-tmpl").join("items").join(id).join("item.txt")
    }

    /// Return the path where the CLI stores a saved label file for the provided label name.
    pub fn saved_label_path(&self, name: &str) -> PathBuf {
        self.home()
            .join(".config")
            .join("rs-cli-tmpl")
            .join("labels")
            .join("definitions")
            .join(name)
            .join("label.txt")
    }

    /// Return the path used to represent one item-label link.
    pub fn label_link_path(&self, item_id: &str, label_name: &str) -> PathBuf {
        self.home()
            .join(".config")
            .join("rs-cli-tmpl")
            .join("labels")
            .join("links")
            .join(item_id)
            .join(label_name)
    }

    /// Assert that a saved item contains the provided value snippet.
    pub fn assert_saved_item_contains(&self, id: &str, expected_snippet: &str) {
        let item_path = self.saved_item_path(id);
        assert!(item_path.exists(), "Expected saved item at {}", item_path.display());
        let content = fs::read_to_string(&item_path).expect("Failed to read saved item");
        assert!(
            content.contains(expected_snippet),
            "Saved item for id `{id}` did not contain `{expected}`; content: {content}",
            expected = expected_snippet
        );
    }

    /// Assert that a saved label exists.
    pub fn assert_saved_label_exists(&self, name: &str) {
        let label_path = self.saved_label_path(name);
        assert!(label_path.exists(), "Expected saved label at {}", label_path.display());
    }

    /// Assert that an item-label link exists.
    pub fn assert_label_link_exists(&self, item_id: &str, label_name: &str) {
        let link_path = self.label_link_path(item_id, label_name);
        assert!(link_path.exists(), "Expected label link at {}", link_path.display());
    }

    /// Assert that an item-label link is removed.
    pub fn assert_label_link_missing(&self, item_id: &str, label_name: &str) {
        let link_path = self.label_link_path(item_id, label_name);
        assert!(!link_path.exists(), "Expected no label link at {}", link_path.display());
    }

    /// Execute a closure after temporarily switching into the provided directory.
    ///
    /// The original directory is always restored, even if the closure panics.
    pub fn with_dir<F, R, P>(&self, dir: P, action: F) -> R
    where
        F: FnOnce() -> R,
        P: AsRef<Path>,
    {
        struct DirRestore {
            original: PathBuf,
        }
        impl Drop for DirRestore {
            fn drop(&mut self) {
                let _ = env::set_current_dir(&self.original);
            }
        }

        let original = env::current_dir().expect("Failed to capture current dir");
        env::set_current_dir(dir.as_ref()).expect("Failed to switch current dir");
        let _guard = DirRestore { original };
        action()
    }
}

impl Drop for TestContext {
    fn drop(&mut self) {
        // SAFETY: Tests are serialized via #[serial]. No other threads or child processes
        // concurrently read or modify the process environment during the test.
        // env::set_var/remove_var are unsafe in Rust 2024.
        match &self.original_home {
            Some(value) => unsafe {
                env::set_var("HOME", value);
            },
            None => unsafe {
                env::remove_var("HOME");
            },
        }
    }
}
