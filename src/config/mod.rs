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

pub fn load(explicit_config: Option<&Path>) -> Result<ResolvedConfig, AppError> {
    let root_path = locate(explicit_config)?;
    let loaded = include::load_tree(&root_path)?;
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
