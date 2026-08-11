use std::io;
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
        // The presence of the entry itself ends the search: a broken symlink
        // or unreadable file here must surface as a failure, not silently
        // defer to a parent configuration covering a different repository set.
        match std::fs::symlink_metadata(&candidate) {
            Ok(_) => {
                let resolved = candidate.canonicalize().map_err(|err| {
                    AppError::config_source(format!("{}: {err}", candidate.display()), err)
                })?;
                if !resolved.is_file() {
                    return Err(AppError::config_error(format!(
                        "{}: not a regular file",
                        candidate.display()
                    )));
                }
                return Ok(resolved);
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(AppError::config_source(
                    format!("{}: {err}", candidate.display()),
                    err,
                ));
            }
        }
    }

    Err(AppError::config_error(format!(
        "grove.toml was not found in {} or any parent directory",
        start.display()
    )))
}
