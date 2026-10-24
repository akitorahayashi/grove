use std::path::{Path, PathBuf};

use crate::repositories::{RemovedRepository, RepositoryDefinition};

/// Fully validated configuration resolved from a root grove.toml.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    root_path: PathBuf,
    groups: Vec<GroupDirectory>,
    repositories: Vec<RepositoryDefinition>,
    removed: Vec<RemovedRepository>,
}

impl ResolvedConfig {
    pub(super) fn new(
        root_path: PathBuf,
        groups: Vec<GroupDirectory>,
        repositories: Vec<RepositoryDefinition>,
        removed: Vec<RemovedRepository>,
    ) -> Self {
        Self { root_path, groups, repositories, removed }
    }

    pub fn root_path(&self) -> &Path {
        &self.root_path
    }

    pub fn repositories(&self) -> &[RepositoryDefinition] {
        &self.repositories
    }

    pub(crate) fn removed(&self) -> &[RemovedRepository] {
        &self.removed
    }

    pub(super) fn groups(&self) -> &[GroupDirectory] {
        &self.groups
    }
}

#[derive(Debug, Clone)]
pub(super) struct GroupDirectory {
    path: PathBuf,
    display_path: String,
}

impl GroupDirectory {
    pub(super) fn new(path: PathBuf, display_path: String) -> Self {
        Self { path, display_path }
    }

    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn display_path(&self) -> &str {
        &self.display_path
    }
}
