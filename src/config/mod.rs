//! Loading and validation for grove.toml.

mod addition;
mod discovery;
mod file;
mod include;
mod lock;
mod resolved;
mod retirement;
mod validation;

pub(crate) use addition::{Addition, EditSession};
pub(crate) use discovery::locate;
pub(crate) use lock::DirectoryLock;
pub use resolved::ResolvedConfig;
pub(crate) use retirement::Retirement;

use std::path::Path;

use crate::AppError;
use crate::repositories::redact_urls_for_display;

/// Shares the root configuration directory's lock with other readers while
/// keeping out `gv add` and `gv retire`, which take it exclusively.
pub(crate) fn share_root_lock(explicit_config: Option<&Path>) -> Result<DirectoryLock, AppError> {
    let root_path = locate(explicit_config)?;
    let directory = root_path
        .parent()
        .ok_or_else(|| AppError::config_error(format!("{} has no parent", root_path.display())))?;
    DirectoryLock::acquire(directory, true)
}

pub fn load(explicit_config: Option<&Path>) -> Result<ResolvedConfig, AppError> {
    let root_path = locate(explicit_config)?;
    let loaded = include::load_tree(&root_path)?;
    validation::resolve(loaded)
}

pub(crate) fn load_without_overrides(
    explicit_config: Option<&Path>,
) -> Result<ResolvedConfig, AppError> {
    let root_path = locate(explicit_config)?;
    let loaded = include::load_tree_without_overrides(&root_path)?;
    validation::resolve(loaded)
}

fn load_with_replacement(
    root_path: &Path,
    replacement_path: &Path,
    contents: &str,
) -> Result<ResolvedConfig, AppError> {
    let loaded = include::load_tree_with_replacement(root_path, replacement_path, contents)?;
    validation::resolve(loaded)
}

/// A configuration error caused by a parser, decoder, or I/O failure. Parser
/// messages quote the offending document, credentials in a URL included, and
/// library callers log errors without the CLI's rendering, so the message is
/// redacted here; `error.rs` cannot reach the redactor without a dependency
/// cycle.
fn source_error(
    message: String,
    source: impl std::error::Error + Send + Sync + 'static,
) -> AppError {
    AppError::config_source(redact_urls_for_display(&message), source)
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File, TryLockError};

    use tempfile::TempDir;

    use super::share_root_lock;

    #[test]
    fn a_shared_root_lock_keeps_out_exclusive_holders_only() {
        let root = TempDir::new().unwrap();
        let config = root.path().join("grove.toml");
        fs::write(&config, "version = 2\n").unwrap();

        let shared = share_root_lock(Some(&config)).unwrap();
        let competing = File::open(root.path()).unwrap();

        assert!(matches!(competing.try_lock(), Err(TryLockError::WouldBlock)));
        competing.try_lock_shared().unwrap();
        competing.unlock().unwrap();
        drop(shared);
        competing.try_lock().unwrap();
    }
}
