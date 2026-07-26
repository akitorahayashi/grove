use std::path::PathBuf;

use crate::AppError;
use crate::git::GitClient;
use crate::inspection::{self, BranchReadiness, Readiness};
use crate::repositories::RepositoryDefinition;

use super::{BlockedReason, Entry, Outcome, Plan, SkippedReason};

pub(super) enum Decision {
    Entry(Entry),
    Clone,
    Fetch {
        common_directory: PathBuf,
        default_branch: String,
    },
    /// A terminal outcome whose repository is still eligible to seed the cache
    /// from its local objects — an existing, URL-matching clone grove leaves
    /// untouched (a dirty working tree or a detached HEAD).
    SeedOnly {
        entry: Entry,
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
        Readiness::DetachedHead => {
            return Ok(seed_only_or_terminal(
                Entry::new(repository, Outcome::Blocked { reason: BlockedReason::DetachedHead }),
                dry_run,
            ));
        }
        Readiness::DirtyTree => {
            return Ok(seed_only_or_terminal(
                Entry::new(
                    repository,
                    Outcome::Skipped { reason: SkippedReason::DirtyWorkingTree },
                ),
                dry_run,
            ));
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

/// A repository grove leaves untouched still seeds the cache during a real run;
/// under `--dry-run` it stays a plain terminal entry with no side effects.
fn seed_only_or_terminal(entry: Entry, dry_run: bool) -> Decision {
    if dry_run { Decision::Entry(entry) } else { Decision::SeedOnly { entry } }
}

fn default_branch_block_reason(
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
        BranchReadiness::Divergence { ahead, behind } if ahead > 0 && behind > 0 => {
            Ok(Some(BlockedReason::Diverged { branch: default_branch.to_string() }))
        }
        BranchReadiness::Divergence { ahead, .. } if ahead > 0 => {
            Ok(Some(BlockedReason::AheadOfOrigin { branch: default_branch.to_string() }))
        }
        BranchReadiness::Divergence { .. } => Ok(None),
    }
}
