use std::path::{Path, PathBuf};

use crate::AppError;

const CONFIG_FILE_NAME: &str = "grove.toml";

pub(super) fn resolve_config_path(explicit_config: Option<&Path>) -> Result<PathBuf, AppError> {
    if let Some(path) = explicit_config {
        return path
            .canonicalize()
            .map_err(|err| AppError::config_error(format!("{}: {err}", path.display())));
    }

    let mut current = std::env::current_dir()?;
    loop {
        let candidate = current.join(CONFIG_FILE_NAME);
        if candidate.is_file() {
            return candidate.canonicalize().map_err(AppError::from);
        }

        if !current.pop() {
            return Err(AppError::config_error(
                "grove.toml was not found in the current directory or its parents",
            ));
        }
    }
}
