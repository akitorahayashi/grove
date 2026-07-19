use std::path::PathBuf;

use crate::AppError;

/// Label storage location settings.
#[derive(Debug, Clone)]
pub struct LabelStorageSettings {
    /// Base path for storage operations.
    pub storage_path: PathBuf,
}

impl LabelStorageSettings {
    /// Create settings with a custom storage path.
    pub fn with_path(path: PathBuf) -> Self {
        Self { storage_path: path }
    }

    /// Create settings using the HOME-based config directory.
    ///
    /// Uses $HOME/.config/rs-cli-tmpl/labels for consistency across platforms and tests.
    pub fn from_env() -> Result<Self, AppError> {
        let home = std::env::var("HOME")
            .map_err(|_| AppError::config_error("HOME environment variable not set"))?;
        let storage_path = PathBuf::from(home).join(".config").join("rs-cli-tmpl").join("labels");
        Ok(Self { storage_path })
    }
}
