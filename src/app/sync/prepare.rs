use crate::git::{GitClient, GitProgress, GitProgressSink};
use crate::repositories::RepositoryDefinition;

use super::events::{Event, EventSink};
use super::update;
use super::{BlockedReason, Entry, Outcome};

pub(super) enum Task<'a> {
    Clone {
        index: usize,
        repository: &'a RepositoryDefinition,
    },
    Fetch {
        index: usize,
        repository: &'a RepositoryDefinition,
        default_branch: String,
        current_branch: String,
    },
}

impl Task<'_> {
    pub(super) fn repository(&self) -> &RepositoryDefinition {
        match self {
            Self::Clone { repository, .. } | Self::Fetch { repository, .. } => repository,
        }
    }
}

pub(super) enum Completion<'a> {
    Entry { index: usize, entry: Entry, prepared: bool },
    Update(update::Task<'a>),
}

impl Completion<'_> {
    pub(super) fn prepared(&self) -> bool {
        match self {
            Self::Entry { prepared, .. } => *prepared,
            Self::Update(_) => true,
        }
    }
}

pub(super) fn repository<'a>(
    git: &impl GitClient,
    task: &Task<'a>,
    events: &impl EventSink,
) -> Completion<'a> {
    let repository = task.repository();
    let mut progress = EventProgress::new(repository, events);

    match task {
        Task::Clone { index, repository } => {
            match git.clone_repository(repository.url(), repository.path(), &mut progress) {
                Ok(()) => Completion::Entry {
                    index: *index,
                    entry: Entry::new(
                        repository,
                        Outcome::Cloned { url: repository.url().to_string() },
                    ),
                    prepared: true,
                },
                Err(err) => Completion::Entry {
                    index: *index,
                    entry: Entry::new(
                        repository,
                        Outcome::Blocked { reason: BlockedReason::CloneFailed(err.to_string()) },
                    ),
                    prepared: false,
                },
            }
        }
        Task::Fetch { index, repository, default_branch, current_branch } => {
            match git.fetch(repository.path(), &mut progress) {
                Ok(()) => Completion::Update(update::Task::new(
                    *index,
                    repository,
                    default_branch.clone(),
                    current_branch.clone(),
                )),
                Err(err) => Completion::Entry {
                    index: *index,
                    entry: Entry::new(
                        repository,
                        Outcome::Blocked { reason: BlockedReason::FetchFailed(err.to_string()) },
                    ),
                    prepared: false,
                },
            }
        }
    }
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
    fn progress(&mut self, progress: GitProgress) {
        self.events.emit(Event::GitProgress {
            repository: self.repository.display_path().to_string(),
            progress,
        });
    }
}
