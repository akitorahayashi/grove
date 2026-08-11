use std::path::{Path, PathBuf};

use crate::AppError;

const CONFIG_FILE_NAME: &str = "grove.toml";

/// Locates the root grove.toml by ascending from the current directory, so a
/// command issued inside a managed repository addresses the same root as one
/// issued at the root itself. Repository paths already resolve relative to the
/// file that declares them and cannot escape its root, so the ascent changes
/// which file is found without changing what any command means. The nearest
/// file wins, which leaves a nested root authoritative for the tree it covers.
pub(crate) fn locate(explicit_config: Option<&Path>) -> Result<PathBuf, AppError> {
    if let Some(path) = explicit_config {
        return path
            .canonicalize()
            .map_err(|err| AppError::config_source(format!("{}: {err}", path.display()), err));
    }

    let start = std::env::current_dir()?;
    for directory in start.ancestors() {
        let candidate = directory.join(CONFIG_FILE_NAME);
        if candidate.is_file() {
            return candidate.canonicalize().map_err(AppError::from);
        }
    }

    Err(AppError::config_error(format!(
        "grove.toml was not found in {} or any parent directory",
        start.display()
    )))
}
