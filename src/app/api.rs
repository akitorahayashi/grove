//! Public application API facade.

use std::path::PathBuf;

use crate::AppError;
use crate::app::events::EventSink;
use crate::app::{AppContext, init, refresh, status, sync, validate};
use crate::git::CommandGitClient;

fn default_context() -> AppContext<CommandGitClient> {
    AppContext::default()
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
) -> Result<sync::Report, AppError> {
    let ctx = default_context();
    sync::execute(&ctx, config_path.as_deref(), &targets, dry_run)
}

pub fn refresh(
    config_path: Option<PathBuf>,
    targets: Vec<String>,
    dry_run: bool,
) -> Result<refresh::Report, AppError> {
    let ctx = default_context();
    refresh::execute(&ctx, config_path.as_deref(), &targets, dry_run)
}

pub fn validate(config_path: Option<PathBuf>) -> Result<validate::Report, AppError> {
    validate::execute(config_path.as_deref())
}

pub(crate) fn init(directory: PathBuf) -> Result<init::Report, AppError> {
    init::execute(&directory)
}

pub(crate) fn sync_with_options(
    config_path: Option<PathBuf>,
    targets: Vec<String>,
    options: sync::SyncOptions,
) -> Result<sync::Report, AppError> {
    let ctx = default_context();
    sync::execute_with_options(&ctx, config_path.as_deref(), &targets, options)
}

pub(crate) fn refresh_with_options(
    config_path: Option<PathBuf>,
    targets: Vec<String>,
    options: refresh::RefreshOptions,
) -> Result<refresh::Report, AppError> {
    let ctx = default_context();
    refresh::execute_with_options(&ctx, config_path.as_deref(), &targets, options)
}

pub(crate) fn refresh_with_events(
    config_path: Option<PathBuf>,
    targets: Vec<String>,
    options: refresh::RefreshOptions,
    events: &impl EventSink<refresh::Phase>,
) -> Result<refresh::Report, AppError> {
    let ctx = default_context();
    refresh::execute_with_events(&ctx, config_path.as_deref(), &targets, options, events)
}

pub(crate) fn sync_with_events(
    config_path: Option<PathBuf>,
    targets: Vec<String>,
    options: sync::SyncOptions,
    events: &impl EventSink<sync::Phase>,
) -> Result<sync::Report, AppError> {
    let ctx = default_context();
    sync::execute_with_events(&ctx, config_path.as_deref(), &targets, options, events)
}
