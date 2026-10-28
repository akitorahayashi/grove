use std::fs::File;
use std::path::Path;

use crate::AppError;

/// An advisory lock on a configuration directory. `gv add` and `gv retire`
/// take it on the root configuration's directory before reading, so neither
/// acts on a configuration the other is changing; editors outside grove do not
/// participate.
pub(super) struct DirectoryLock {
    file: File,
}

impl DirectoryLock {
    pub(super) fn acquire(directory: &Path, shared: bool) -> Result<Self, AppError> {
        let file = File::open(directory)?;
        if shared {
            file.lock_shared()?;
        } else {
            file.lock()?;
        }
        Ok(Self { file })
    }
}

impl Drop for DirectoryLock {
    fn drop(&mut self) {
        let _ = File::unlock(&self.file);
    }
}
