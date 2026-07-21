use std::path::PathBuf;

use crate::AppError;
use crate::app::inspection::{self, Readiness};
use crate::git::GitClient;
use crate::repositories::RepositoryDefinition;

use super::{BlockedReason, Entry, Outcome, Plan, SkippedReason};

pub(super) enum Decision {
    Entry(Entry),
    Clone,
    Fetch {
        common_directory: PathBuf,
        default_branch: String,
    },
    /// A terminal outcome that still contributes a cache seed — an existing
    /// repository grove leaves untouched (a dirty working tree) but whose
    /// objects can still populate the cache.
    SeedOnly {
        entry: Entry,
        common_directory: PathBuf,
        default_branch: String,
    },
}

pub(super) fn repository(
    git: &impl GitClient,
    repository: &RepositoryDefinition,
    dry_run: bool,
) -> Result<Decision, AppError> {
    if !repository.path().exists() {
        if dry_run {
            return Ok(Decision::Entry(Entry::new(
                repository,
                Outcome::Planned(Plan::Clone { url: repository.url().to_string() }),
            )));
        }

        return Ok(Decision::Clone);
    }

    let default_branch = match inspection::inspect(git, repository)? {
        Readiness::NotAWorkTree => {
            return blocked(repository, BlockedReason::DestinationNotGitRepository);
        }
        Readiness::MissingOrigin => return blocked(repository, BlockedReason::MissingOrigin),
        Readiness::UrlMismatch { actual, expected } => {
            return Ok(Decision::Entry(Entry::blocked_with_details(
                repository,
                Outcome::Blocked { reason: BlockedReason::RemoteUrlMismatch },
                super::BlockedReasonDetails::RemoteUrlMismatch { actual, expected },
            )));
        }
        Readiness::DetachedHead => return blocked(repository, BlockedReason::DetachedHead),
        Readiness::DirtyTree => {
            let entry = Entry::new(
                repository,
                Outcome::Skipped { reason: SkippedReason::DirtyWorkingTree },
            );
            // A dirty repository is left untouched, but its objects still seed
            // the cache. Seeding needs a branch to track and the object store;
            // when the default branch cannot be resolved, skip without seeding.
            if dry_run {
                return Ok(Decision::Entry(entry));
            }
            let Some(default_branch) =
                git.default_branch(repository.path(), repository.default_branch())?
            else {
                return Ok(Decision::Entry(entry));
            };
            let common_directory = git.common_directory(repository.path())?;
            return Ok(Decision::SeedOnly { entry, common_directory, default_branch });
        }
        Readiness::NoDefaultBranch => {
            return blocked(repository, BlockedReason::MissingRemoteDefaultBranch);
        }
        Readiness::Ready { default_branch } => default_branch,
    };

    if dry_run {
        if let Some(reason) = default_branch_block_reason(git, repository, &default_branch)? {
            return Ok(Decision::Entry(Entry::new(repository, Outcome::Blocked { reason })));
        }
        return Ok(Decision::Entry(Entry::new(
            repository,
            Outcome::Planned(Plan::Fetch { branch: default_branch }),
        )));
    }

    let common_directory = git.common_directory(repository.path())?;
    Ok(Decision::Fetch { common_directory, default_branch })
}

fn blocked(repository: &RepositoryDefinition, reason: BlockedReason) -> Result<Decision, AppError> {
    Ok(Decision::Entry(Entry::new(repository, Outcome::Blocked { reason })))
}

fn default_branch_block_reason(
    git: &impl GitClient,
    repository: &RepositoryDefinition,
    default_branch: &str,
) -> Result<Option<BlockedReason>, AppError> {
    if !git.local_branch_exists(repository.path(), default_branch)? {
        return Ok(Some(BlockedReason::MissingLocalBranch { branch: default_branch.to_string() }));
    }
    if !git.remote_branch_exists(repository.path(), default_branch)? {
        return Ok(Some(BlockedReason::MissingRemoteBranch { branch: default_branch.to_string() }));
    }
    Ok(None)
}
