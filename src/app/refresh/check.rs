use std::path::PathBuf;

use crate::AppError;
use crate::git::{GitClient, urls_match};
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
        return Ok(Decision::Entry(Entry::new(
            repository,
            Outcome::Blocked { reason: BlockedReason::MissingRepository },
        )));
    }

    if !repository.path().is_dir() || !git.is_work_tree(repository.path())? {
        return Ok(Decision::Entry(Entry::new(
            repository,
            Outcome::Blocked { reason: BlockedReason::DestinationNotGitRepository },
        )));
    }

    let Some(actual_url) = git.remote_url(repository.path())? else {
        return Ok(Decision::Entry(Entry::new(
            repository,
            Outcome::Blocked { reason: BlockedReason::MissingOrigin },
        )));
    };
    if !urls_match(&actual_url, repository.url()) {
        return Ok(Decision::Entry(Entry::blocked_with_details(
            repository,
            BlockedReason::RemoteUrlMismatch,
            BlockedReasonDetails::RemoteUrlMismatch {
                actual: actual_url.to_string(),
                expected: repository.url().to_string(),
            },
        )));
    }

    if git.current_branch(repository.path())?.is_none() {
        return Ok(Decision::Entry(Entry::new(
            repository,
            Outcome::Blocked { reason: BlockedReason::DetachedHead },
        )));
    }

    if !git.working_tree_clean(repository.path())? {
        return Ok(Decision::Entry(Entry::new(
            repository,
            Outcome::Skipped { reason: SkippedReason::DirtyWorkingTree },
        )));
    }

    let Some(default_branch) =
        git.default_branch(repository.path(), repository.default_branch())?
    else {
        return Ok(Decision::Entry(Entry::new(
            repository,
            Outcome::Blocked { reason: BlockedReason::MissingRemoteDefaultBranch },
        )));
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

pub(super) fn refresh_block_reason(
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

    let divergence = git.branch_divergence(repository.path(), default_branch)?;
    if divergence.ahead() > 0 && divergence.behind() > 0 {
        return Ok(Some(BlockedReason::Diverged { branch: default_branch.to_string() }));
    }
    if divergence.ahead() > 0 {
        return Ok(Some(BlockedReason::AheadOfOrigin { branch: default_branch.to_string() }));
    }
    Ok(None)
}
