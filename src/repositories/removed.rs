use std::path::{Path, PathBuf};

use super::{RemoteUrl, RepositoryName};

/// A repository declared under `removed` after configuration validation and
/// path resolution: the old location sync converges away from, either by
/// moving it to a live repository or by moving it to the trash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovedRepository {
    name: RepositoryName,
    path: PathBuf,
    display_path: String,
    url: RemoteUrl,
    moved_to: Option<RepositoryName>,
}

impl RemovedRepository {
    pub fn new(
        name: RepositoryName,
        path: PathBuf,
        display_path: String,
        url: RemoteUrl,
        moved_to: Option<RepositoryName>,
    ) -> Self {
        Self { name, path, display_path, url, moved_to }
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

    pub fn moved_to(&self) -> Option<&RepositoryName> {
        self.moved_to.as_ref()
    }
}
