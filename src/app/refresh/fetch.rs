use std::path::{Path, PathBuf};

use crate::git::GitClient;
use crate::phases::{EventProgress, EventSink, Task as PhaseTask};
use crate::repositories::RepositoryDefinition;

use super::update;
use super::{BlockedReason, Entry, Outcome, Phase};

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
}

impl PhaseTask for Task<'_> {
    fn repository(&self) -> &RepositoryDefinition {
        self.repository
    }

    fn resource(&self) -> &Path {
        &self.common_directory
    }
}

pub(super) enum Completion<'a> {
    Entry { index: usize, entry: Entry },
    Refresh(update::Task<'a>),
}

impl Completion<'_> {
    pub(super) fn fetched(&self) -> bool {
        matches!(self, Self::Refresh(_))
    }
}

pub(super) fn repository<'a>(
    git: &impl GitClient,
    task: &Task<'a>,
    events: &impl EventSink<Phase>,
) -> Result<Completion<'a>, crate::AppError> {
    let mut progress = EventProgress::new(task.repository, events);

    Ok(match git.fetch(task.repository.path(), &mut progress) {
        Ok(()) => Completion::Refresh(update::Task::new(
            task.index,
            task.repository,
            task.common_directory.clone(),
            task.default_branch.clone(),
        )),
        Err(error) if matches!(error, crate::AppError::Internal(_)) => return Err(error),
        Err(error) => Completion::Entry {
            index: task.index,
            entry: Entry::new(
                task.repository,
                Outcome::Blocked { reason: BlockedReason::FetchFailed(error.to_string()) },
            ),
        },
    })
}
