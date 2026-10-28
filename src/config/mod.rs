//! Loading and validation for grove.toml.

mod addition;
mod discovery;
mod file;
mod include;
mod resolved;
mod validation;

pub(crate) use addition::{Addition, EditSession};
pub(crate) use discovery::locate;
pub use resolved::ResolvedConfig;

use std::path::Path;

use crate::AppError;
use crate::repositories::redact_urls_for_display;

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
