use std::path::{Path, PathBuf};

use crate::AppError;
use crate::config;

#[derive(Debug, Clone)]
pub struct Report {
    config_path: PathBuf,
    repository_count: usize,
}

impl Report {
    fn new(config_path: PathBuf, repository_count: usize) -> Self {
        Self { config_path, repository_count }
    }

    pub fn config_path(&self) -> &Path {
        &self.config_path
    }

    pub fn repository_count(&self) -> usize {
        self.repository_count
    }
}

pub fn execute(config_path: Option<&Path>) -> Result<Report, AppError> {
    let config = config::load(config_path)?;
    Ok(Report::new(config.root_path().to_path_buf(), config.repositories().len()))
}
