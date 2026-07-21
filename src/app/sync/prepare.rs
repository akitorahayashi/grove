use std::path::{Path, PathBuf};

use crate::app::cache::CacheStore;
use crate::app::events::{EventProgress, EventSink};
use crate::app::phases::PhaseTask;
use crate::git::GitClient;
use crate::repositories::RepositoryDefinition;

use super::update;
use super::{BlockedReason, Entry, Outcome, Phase};

pub(super) enum Task<'a> {
    Clone {
        index: usize,
        repository: &'a RepositoryDefinition,
    },
    Fetch {
        index: usize,
        repository: &'a RepositoryDefinition,
        common_directory: PathBuf,
        default_branch: String,
    },
}

impl PhaseTask for Task<'_> {
    fn repository(&self) -> &RepositoryDefinition {
        match self {
            Self::Clone { repository, .. } | Self::Fetch { repository, .. } => repository,
        }
    }

    fn resource(&self) -> &Path {
        match self {
            Self::Clone { repository, .. } => repository.path(),
            Self::Fetch { common_directory, .. } => common_directory,
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
    cache: &CacheStore,
    task: &Task<'a>,
    events: &impl EventSink<Phase>,
) -> Result<Completion<'a>, crate::AppError> {
    let repository = task.repository();
    let mut progress = EventProgress::new(repository, events);

    match task {
        Task::Clone { index, repository } => Ok(
            match cache.place(
                git,
                repository.url(),
                repository.path(),
                Some(repository.root()),
                repository.default_branch(),
                &mut progress,
            ) {
                Ok(cache) => Completion::Entry {
                    index: *index,
                    entry: Entry::new(
                        repository,
                        Outcome::Cloned { url: repository.url().to_string(), cache },
                    ),
                    prepared: true,
                },
                Err(err) if matches!(err, crate::AppError::Internal(_)) => return Err(err),
                Err(err) => Completion::Entry {
                    index: *index,
                    entry: Entry::new(
                        repository,
                        Outcome::Blocked { reason: BlockedReason::CloneFailed(err.to_string()) },
                    ),
                    prepared: false,
                },
            },
        ),
        Task::Fetch { index, repository, common_directory, default_branch } => {
            Ok(match git.fetch(repository.path(), &mut progress) {
                Ok(()) => Completion::Update(update::Task::new(
                    *index,
                    repository,
                    common_directory.clone(),
                    default_branch.clone(),
                )),
                Err(err) if matches!(err, crate::AppError::Internal(_)) => return Err(err),
                Err(err) => Completion::Entry {
                    index: *index,
                    entry: Entry::new(
                        repository,
                        Outcome::Blocked { reason: BlockedReason::FetchFailed(err.to_string()) },
                    ),
                    prepared: false,
                },
            })
        }
    }
}
