use std::path::{Path, PathBuf};

use crate::repositories::RepositoryDefinition;

/// Fully validated configuration resolved from a root grove.toml.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    root_path: PathBuf,
    repositories: Vec<RepositoryDefinition>,
}

impl ResolvedConfig {
    pub(super) fn new(root_path: PathBuf, repositories: Vec<RepositoryDefinition>) -> Self {
        Self { root_path, repositories }
    }

    pub fn root_path(&self) -> &Path {
        &self.root_path
    }

    pub fn repositories(&self) -> &[RepositoryDefinition] {
        &self.repositories
    }
}
