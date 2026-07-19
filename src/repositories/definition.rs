use std::path::{Path, PathBuf};

use super::RepositoryName;

/// A repository definition after configuration validation and path resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryDefinition {
    name: RepositoryName,
    path: PathBuf,
    display_path: String,
    url: String,
    default_branch: Option<String>,
    source_config: PathBuf,
}

impl RepositoryDefinition {
    pub fn new(
        name: RepositoryName,
        path: PathBuf,
        display_path: String,
        url: String,
        default_branch: Option<String>,
        source_config: PathBuf,
    ) -> Self {
        Self { name, path, display_path, url, default_branch, source_config }
    }

    pub fn name(&self) -> &RepositoryName {
        &self.name
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn display_path(&self) -> &str {
        &self.display_path
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn default_branch(&self) -> Option<&str> {
        self.default_branch.as_deref()
    }

    pub fn source_config(&self) -> &Path {
        &self.source_config
    }
}
