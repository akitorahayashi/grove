//! Subcommand implementations.

pub(in crate::cli) mod cache;
pub(in crate::cli) mod clone;
pub(in crate::cli) mod init;
pub(in crate::cli) mod refresh;
pub(in crate::cli) mod status;
pub(in crate::cli) mod sync;
pub(in crate::cli) mod validate;

use std::path::PathBuf;

use owo_colors::OwoColorize;

use crate::AppError;
use crate::config;

use crate::cli::output::{Output, terminal_text};
use crate::cli::tty::report::write_line;

/// Resolves the grove.toml a subcommand operates on before the use case loads
/// it. Discovery that ascended above the current directory names the file it
/// settled on, so a root picked up from an unexpected depth acts announced
/// rather than silently.
pub(in crate::cli) fn resolve_config(
    explicit: Option<PathBuf>,
    output: &mut Output<'_>,
) -> Result<PathBuf, AppError> {
    let discovered = explicit.is_none();
    let path = config::locate(explicit.as_deref())?;
    if discovered && path.parent() != Some(std::env::current_dir()?.canonicalize()?.as_path()) {
        let displayed = terminal_text(&path.display().to_string());
        write_line(output, format_args!("{}", format!("Config: {displayed}").dimmed()))?;
    }
    Ok(path)
}
