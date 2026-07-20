use std::path::PathBuf;

use crate::AppError;
use crate::app::inspection::{self, BranchReadiness, Readiness};
use crate::git::GitClient;
use crate::repositories::RepositoryDefinition;

use super::{BlockedReason, BlockedReasonDetails, Entry, Outcome, Plan, SkippedReason};

pub(super) enum Decision {
    Entry(Entry),
    Fetch { common_directory: PathBuf, default_branch: String },
}

pub(super) fn repository(
    git: &impl GitClient,
    repository: &RepositoryDefinition,
    dry_run: bool,
) -> Result<Decision, AppError> {
    if !repository.path().exists() {
        return blocked(repository, BlockedReason::MissingRepository);
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
                BlockedReasonDetails::RemoteUrlMismatch { actual, expected },
            )));
        }
        Readiness::DetachedHead => return blocked(repository, BlockedReason::DetachedHead),
        Readiness::DirtyTree => {
            return Ok(Decision::Entry(Entry::new(
                repository,
                Outcome::Skipped { reason: SkippedReason::DirtyWorkingTree },
            )));
        }
        Readiness::NoDefaultBranch => {
            return blocked(repository, BlockedReason::MissingRemoteDefaultBranch);
        }
        Readiness::Ready { default_branch } => default_branch,
    };

    if dry_run {
        if let Some(reason) = refresh_block_reason(git, repository, &default_branch)? {
            return Ok(Decision::Entry(Entry::new(repository, Outcome::Blocked { reason })));
        }
        return Ok(Decision::Entry(Entry::new(
            repository,
            Outcome::Planned(Plan::new(default_branch)),
        )));
    }

    let common_directory = git.common_directory(repository.path())?;
    Ok(Decision::Fetch { common_directory, default_branch })
}

fn blocked(repository: &RepositoryDefinition, reason: BlockedReason) -> Result<Decision, AppError> {
    Ok(Decision::Entry(Entry::new(repository, Outcome::Blocked { reason })))
}

pub(super) fn refresh_block_reason(
    git: &impl GitClient,
    repository: &RepositoryDefinition,
    default_branch: &str,
) -> Result<Option<BlockedReason>, AppError> {
    match inspection::branch_readiness(git, repository, default_branch)? {
        BranchReadiness::MissingLocal => {
            Ok(Some(BlockedReason::MissingLocalBranch { branch: default_branch.to_string() }))
        }
        BranchReadiness::MissingRemote => {
            Ok(Some(BlockedReason::MissingRemoteBranch { branch: default_branch.to_string() }))
        }
        BranchReadiness::Divergence { ahead, behind } => {
            if ahead > 0 && behind > 0 {
                Ok(Some(BlockedReason::Diverged { branch: default_branch.to_string() }))
            } else if ahead > 0 {
                Ok(Some(BlockedReason::AheadOfOrigin { branch: default_branch.to_string() }))
            } else {
                Ok(None)
            }
        }
    }
}
