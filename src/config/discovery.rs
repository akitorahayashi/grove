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
            .map_err(|err| super::source_error(format!("{}: {err}", path.display()), err));
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
                    super::source_error(format!("{}: {err}", candidate.display()), err)
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
                return Err(super::source_error(format!("{}: {err}", candidate.display()), err));
            }
        }
    }

    Err(AppError::config_error(format!(
        "grove.toml was not found in {} or any parent directory",
        start.display()
    )))
}

/// The root configuration a replacement writes, located as `locate` locates
/// it, except that an explicitly named file may be absent, since replacing
/// creates it; its directory must exist. An entry that exists but cannot be
/// resolved, such as a broken symbolic link, still fails rather than being
/// taken for an absent file.
pub(super) fn locate_destination(explicit_config: Option<&Path>) -> Result<PathBuf, AppError> {
    let Some(path) = explicit_config else {
        return locate(None);
    };
    match std::fs::symlink_metadata(path) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        _ => return locate(explicit_config),
    }
    let name = path.file_name().ok_or_else(|| {
        AppError::config_error(format!("{}: not a configuration file path", path.display()))
    })?;
    let directory = match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    };
    let directory = directory
        .canonicalize()
        .map_err(|err| super::source_error(format!("{}: {err}", directory.display()), err))?;
    Ok(directory.join(name))
}
