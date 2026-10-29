use crate::AppError;
use crate::app::entry::BlockedReasonDetails;
use crate::config::Repoint;
use crate::git::{Removal, RepositoryProbe};
use crate::inspection::{self, Checkout};

pub(crate) type Entry = crate::app::entry::Entry<Outcome>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Outcome {
    Planned { url: String },
    Repointed { url: String },
    Blocked { reason: BlockedReason },
}

/// Why a checkout's origin is left as it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BlockedReason {
    SymbolicLink,
    NotAWorkTree,
    InsideAnotherRepository,
    MissingOrigin,
    RemoteUrlMismatch,
    SetUrlFailed(String),
}

impl BlockedReason {
    pub(crate) fn message(&self) -> String {
        match self {
            Self::SymbolicLink => inspection::symbolic_link().to_string(),
            Self::NotAWorkTree => inspection::destination_not_git_repository().to_string(),
            Self::InsideAnotherRepository => inspection::inside_another_repository().to_string(),
            Self::MissingOrigin => inspection::missing_origin().to_string(),
            Self::RemoteUrlMismatch => "remote URL matches neither the previous nor the declared \
                                        URL; align the origin"
                .to_string(),
            Self::SetUrlFailed(message) => format!("origin not repointed: {message}"),
        }
    }
}

enum Assessment {
    Settled,
    Planned,
    Blocked(BlockedReason),
    Mismatch { actual: String },
}

/// Only a checkout whose origin still names the previous URL exactly is
/// repointed, as removal requires before it moves anything; one already on
/// the declared URL is settled, and one on any other URL is not grove's to
/// change.
fn assess(git: &impl RepositoryProbe, repoint: &Repoint) -> Result<Assessment, AppError> {
    let origin = match inspection::checkout(git, repoint.path())? {
        Checkout::Absent => return Ok(Assessment::Settled),
        Checkout::SymbolicLink => return Ok(Assessment::Blocked(BlockedReason::SymbolicLink)),
        Checkout::NotAWorkTree => return Ok(Assessment::Blocked(BlockedReason::NotAWorkTree)),
        Checkout::InsideAnotherRepository => {
            return Ok(Assessment::Blocked(BlockedReason::InsideAnotherRepository));
        }
        Checkout::MissingOrigin => return Ok(Assessment::Blocked(BlockedReason::MissingOrigin)),
        Checkout::Origin(origin) => origin,
    };
    Ok(if origin.matches(repoint.repository().url()) {
        Assessment::Settled
    } else if origin.matches(repoint.previous()) {
        Assessment::Planned
    } else {
        Assessment::Mismatch { actual: origin.to_string() }
    })
}

/// `None` when nothing needs doing: the path is absent or its origin already
/// names the declared URL.
pub(crate) fn converge(
    git: &(impl RepositoryProbe + Removal),
    repoint: &Repoint,
    dry_run: bool,
) -> Result<Option<Entry>, AppError> {
    match assess(git, repoint)? {
        Assessment::Planned if !dry_run => act(git, repoint),
        assessment => Ok(entry(repoint, assessment)),
    }
}

/// Assessed again under the repository lock, so an origin a concurrent grove
/// operation changed after the first look is not overwritten. Removals may
/// already have acted, so a failure blocks rather than discarding their report.
fn act(
    git: &(impl RepositoryProbe + Removal),
    repoint: &Repoint,
) -> Result<Option<Entry>, AppError> {
    let repository = repoint.repository();
    let blocked = |error: AppError| -> Result<Option<Entry>, AppError> {
        let reason = BlockedReason::SetUrlFailed(error.demote()?);
        Ok(Some(Entry::new(repository, Outcome::Blocked { reason })))
    };
    let _lock = match git
        .common_directory(repoint.path())
        .and_then(|common_directory| git.lock_repository(&common_directory))
    {
        Ok(lock) => lock,
        Err(error) => return blocked(error),
    };
    match assess(git, repoint)? {
        Assessment::Planned => match git.set_origin_url(repoint.path(), repository.url()) {
            Ok(()) => {
                let outcome = Outcome::Repointed { url: repository.url().to_string() };
                Ok(Some(Entry::new(repository, outcome)))
            }
            Err(error) => blocked(error),
        },
        assessment => Ok(entry(repoint, assessment)),
    }
}

fn entry(repoint: &Repoint, assessment: Assessment) -> Option<Entry> {
    let repository = repoint.repository();
    match assessment {
        Assessment::Settled => None,
        Assessment::Planned => {
            Some(Entry::new(repository, Outcome::Planned { url: repository.url().to_string() }))
        }
        Assessment::Blocked(reason) => Some(Entry::new(repository, Outcome::Blocked { reason })),
        Assessment::Mismatch { actual } => Some(Entry::blocked_with_details(
            repository,
            Outcome::Blocked { reason: BlockedReason::RemoteUrlMismatch },
            BlockedReasonDetails::RemoteUrlMismatch {
                actual,
                expected: repository.url().to_string(),
            },
        )),
    }
}
