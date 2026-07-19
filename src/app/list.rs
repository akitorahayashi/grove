use std::path::Path;

use serde::Serialize;

use crate::AppError;
use crate::app::AppContext;
use crate::config;
use crate::git::GitClient;

#[derive(Debug, Clone, Serialize)]
pub struct ListEntry {
    pub name: String,
    pub path: String,
    pub url: String,
    pub source_config: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListReport {
    pub repositories: Vec<ListEntry>,
}

pub fn execute(
    ctx: &AppContext<impl GitClient>,
    config_path: Option<&Path>,
) -> Result<ListReport, AppError> {
    ctx.git().verify_available()?;
    let config = config::load(config_path)?;
    let repositories = config
        .repositories()
        .iter()
        .map(|repository| ListEntry {
            name: repository.name().as_str().to_string(),
            path: repository.path().display().to_string(),
            url: repository.url().to_string(),
            source_config: repository.source_config().display().to_string(),
        })
        .collect();

    Ok(ListReport { repositories })
}
