//! The operating system trash boundary.

use std::io;
use std::path::Path;

use crate::AppError;

/// Moving a directory to the trash, where it stays recoverable. There is no
/// permanent-deletion fallback: a failure is the caller's to report.
pub trait Trash: Sync {
    fn trash(&self, path: &Path) -> Result<(), AppError>;
}

/// The platform trash: freedesktop's Trash specification on Linux, honoring
/// `XDG_DATA_HOME`, and `NSFileManager` on macOS. `NSFileManager` is chosen
/// over the crate's Finder default because Finder is driven through
/// AppleScript, which needs an Automation grant that SSH sessions and fresh
/// terminals lack.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemTrash;

impl Trash for SystemTrash {
    fn trash(&self, path: &Path) -> Result<(), AppError> {
        context().delete(path).map_err(|err| {
            io::Error::other(format!("failed to move '{}' to the trash: {err}", path.display()))
                .into()
        })
    }
}

#[cfg(target_os = "macos")]
fn context() -> trash::TrashContext {
    use trash::macos::{DeleteMethod, TrashContextExtMacos};

    let mut context = trash::TrashContext::default();
    context.set_delete_method(DeleteMethod::NsFileManager);
    context
}

#[cfg(not(target_os = "macos"))]
fn context() -> trash::TrashContext {
    trash::TrashContext::default()
}
