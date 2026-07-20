use std::path::{Path, PathBuf};

use super::{BranchName, RemoteUrl, RepositoryName};

/// A repository definition after configuration validation and path resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryDefinition {
    name: RepositoryName,
    path: PathBuf,
    display_path: String,
    url: RemoteUrl,
    default_branch: Option<BranchName>,
    source_config: PathBuf,
    root: PathBuf,
}

impl RepositoryDefinition {
    pub fn new(
        name: RepositoryName,
        path: PathBuf,
        display_path: String,
        url: RemoteUrl,
        default_branch: Option<BranchName>,
        source_config: PathBuf,
        root: PathBuf,
    ) -> Self {
        Self { name, path, display_path, url, default_branch, source_config, root }
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

    pub fn url(&self) -> &RemoteUrl {
        &self.url
    }

    pub fn default_branch(&self) -> Option<&BranchName> {
        self.default_branch.as_ref()
    }

    pub fn source_config(&self) -> &Path {
        &self.source_config
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }
}
