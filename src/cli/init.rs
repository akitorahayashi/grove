use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use clap::Args;

use crate::AppError;

const CONFIG_TEMPLATE: &str = include_str!("../assets/grove.toml.tpl");

#[derive(Args)]
pub(super) struct InitCommand;

pub(super) fn run(config: Option<PathBuf>, _command: InitCommand) -> Result<(), AppError> {
    if config.is_some() {
        return Err(AppError::config_error("--config cannot be used with init"));
    }

    let path = std::env::current_dir()?.join("grove.toml");
    let mut file = OpenOptions::new().write(true).create_new(true).open(&path)?;
    file.write_all(CONFIG_TEMPLATE.as_bytes())?;

    println!("created {}", path.display());

    Ok(())
}
