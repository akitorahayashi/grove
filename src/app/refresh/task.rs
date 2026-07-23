use std::path::{Path, PathBuf};

use crate::phases::Task as PhaseTask;
use crate::repositories::RepositoryDefinition;

/// A repository that passed the check phase and flows through the fetch and
/// refresh phases as a single value. The fetch phase hands it onward on success
/// rather than reconstructing it for the refresh phase.
#[derive(Clone)]
pub(super) struct Task<'a> {
    index: usize,
    repository: &'a RepositoryDefinition,
    common_directory: PathBuf,
    default_branch: String,
}

impl<'a> Task<'a> {
    pub(super) fn new(
        index: usize,
        repository: &'a RepositoryDefinition,
        common_directory: PathBuf,
        default_branch: String,
    ) -> Self {
        Self { index, repository, common_directory, default_branch }
    }

    pub(super) fn index(&self) -> usize {
        self.index
    }

    pub(super) fn default_branch(&self) -> &str {
        &self.default_branch
    }
}

impl PhaseTask for Task<'_> {
    fn repository(&self) -> &RepositoryDefinition {
        self.repository
    }

    fn resource(&self) -> &Path {
        &self.common_directory
    }
}
