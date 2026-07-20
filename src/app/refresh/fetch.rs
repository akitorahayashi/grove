use std::path::{Path, PathBuf};

use crate::git::{GitClient, GitProgress, GitProgressSink};
use crate::repositories::RepositoryDefinition;

use super::events::{Event, EventSink};
use super::update;
use super::{BlockedReason, Entry, Outcome};

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

    pub(super) fn repository(&self) -> &RepositoryDefinition {
        self.repository
    }

    pub(super) fn resource(&self) -> &Path {
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
    events: &impl EventSink,
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

struct EventProgress<'a, E: EventSink> {
    repository: &'a RepositoryDefinition,
    events: &'a E,
}

impl<'a, E: EventSink> EventProgress<'a, E> {
    fn new(repository: &'a RepositoryDefinition, events: &'a E) -> Self {
        Self { repository, events }
    }
}

impl<E: EventSink> GitProgressSink for EventProgress<'_, E> {
    fn progress(&mut self, progress: GitProgress) -> Result<(), crate::AppError> {
        self.events.emit(Event::GitProgress {
            repository: self.repository.display_path().to_string(),
            progress,
        })
    }
}
