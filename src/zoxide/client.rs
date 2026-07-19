use std::path::{Path, PathBuf};

use crate::AppError;

/// Contract for zoxide operations owned by grove.
pub trait ZoxideClient: Sync {
    fn verify_available(&self) -> Result<(), AppError>;

    fn resolve_symlinks(&self) -> bool;

    fn entries(&self) -> Result<Vec<PathBuf>, AppError>;

    fn add(&self, path: &Path) -> Result<(), AppError>;
}
