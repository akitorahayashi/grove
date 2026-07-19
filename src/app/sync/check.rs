use std::path::PathBuf;

use crate::AppError;
use crate::git::{GitClient, urls_match};
use crate::repositories::RepositoryDefinition;

use super::{BlockedReason, Entry, Outcome, Plan, SkippedReason};

pub(super) enum Decision {
    Entry(Entry),
    Clone,
    Fetch { common_directory: PathBuf, default_branch: String, current_branch: String },
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
        return Ok(Decision::Entry(Entry::new(
            repository,
            Outcome::Blocked { reason: BlockedReason::RemoteUrlMismatch },
        )));
    }

    let Some(current_branch) = git.current_branch(repository.path())? else {
        return Ok(Decision::Entry(Entry::new(
            repository,
            Outcome::Blocked { reason: BlockedReason::DetachedHead },
        )));
    };

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
        if let Some(reason) = default_branch_block_reason(git, repository, &default_branch)? {
            return Ok(Decision::Entry(Entry::new(repository, Outcome::Blocked { reason })));
        }
        return Ok(Decision::Entry(Entry::new(
            repository,
            Outcome::Planned(Plan::Fetch { branch: default_branch }),
        )));
    }

    let common_directory = git.common_directory(repository.path())?;
    Ok(Decision::Fetch { common_directory, default_branch, current_branch })
}

pub(super) fn default_branch_block_reason(
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
