//! Public application API facade.

use std::path::PathBuf;

use crate::AppError;
use crate::app::{AppContext, list, status, sync};
use crate::git::CommandGitClient;

fn default_context() -> AppContext<CommandGitClient> {
    AppContext::default()
}

pub fn list(config_path: Option<PathBuf>) -> Result<list::ListReport, AppError> {
    let ctx = default_context();
    list::execute(&ctx, config_path.as_deref())
}

pub fn status(
    config_path: Option<PathBuf>,
    targets: Vec<String>,
    fetch: bool,
) -> Result<status::StatusReport, AppError> {
    let ctx = default_context();
    status::execute(&ctx, config_path.as_deref(), &targets, fetch)
}

pub fn sync(
    config_path: Option<PathBuf>,
    targets: Vec<String>,
    dry_run: bool,
) -> Result<sync::SyncReport, AppError> {
    let ctx = default_context();
    sync::execute(&ctx, config_path.as_deref(), &targets, dry_run)
}
