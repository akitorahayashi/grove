//! Repository readiness probing shared by the sync, refresh, and status use
//! cases, and the canonical diagnostics for the conditions they share.
//!
//! The probes return neutral findings; each use case maps them to its own
//! outcome vocabulary. Owning the shared message strings here keeps the
//! per-use-case reason enums from drifting apart.

use crate::AppError;
use crate::git::{RepositoryProbe, urls_match};
use crate::repositories::RepositoryDefinition;

/// A repository's operability at an existing path, independent of any use
/// case's vocabulary. The missing-path decision (clone vs. block) is left to
/// each use case, as is whether to compute the Git common directory.
pub(crate) enum Readiness {
    NotAWorkTree,
    MissingOrigin,
    UrlMismatch { actual: String, expected: String },
    DetachedHead,
    DirtyTree,
    NoDefaultBranch,
    Ready { default_branch: String },
}

pub(crate) fn inspect(
    git: &impl RepositoryProbe,
    repository: &RepositoryDefinition,
) -> Result<Readiness, AppError> {
    if !repository.path().is_dir() || !git.is_work_tree(repository.path())? {
        return Ok(Readiness::NotAWorkTree);
    }

    let Some(actual_url) = git.remote_url(repository.path())? else {
        return Ok(Readiness::MissingOrigin);
    };
    if !urls_match(&actual_url, repository.url()) {
        return Ok(Readiness::UrlMismatch {
            actual: actual_url.to_string(),
            expected: repository.url().to_string(),
        });
    }

    if git.current_branch(repository.path())?.is_none() {
        return Ok(Readiness::DetachedHead);
    }

    if !git.working_tree_clean(repository.path())? {
        return Ok(Readiness::DirtyTree);
    }

    let Some(default_branch) =
        git.default_branch(repository.path(), repository.default_branch())?
    else {
        return Ok(Readiness::NoDefaultBranch);
    };

    Ok(Readiness::Ready { default_branch })
}

/// The default branch's standing against its upstream.
pub(crate) enum BranchReadiness {
    MissingLocal,
    MissingRemote,
    Divergence { ahead: u32, behind: u32 },
}

pub(crate) fn branch_readiness(
    git: &impl RepositoryProbe,
    repository: &RepositoryDefinition,
    branch: &str,
) -> Result<BranchReadiness, AppError> {
    if !git.local_branch_exists(repository.path(), branch)? {
        return Ok(BranchReadiness::MissingLocal);
    }
    if !git.remote_branch_exists(repository.path(), branch)? {
        return Ok(BranchReadiness::MissingRemote);
    }
    let divergence = git.branch_divergence(repository.path(), branch)?;
    Ok(BranchReadiness::Divergence { ahead: divergence.ahead(), behind: divergence.behind() })
}

/// The structured counterpart of a diagnostic that a report entry carries
/// beyond its message string, so the CLI can render the offending values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BlockedReasonDetails {
    RemoteUrlMismatch { actual: String, expected: String },
}

pub(crate) fn destination_not_git_repository() -> &'static str {
    "destination exists but is not a Git repository"
}

pub(crate) fn missing_origin() -> &'static str {
    "remote origin is missing"
}

pub(crate) fn remote_url_mismatch() -> &'static str {
    "remote URL does not match grove.toml"
}

pub(crate) fn missing_remote_default_branch() -> &'static str {
    "remote default branch cannot be determined"
}

pub(crate) fn missing_local_branch(branch: &str) -> String {
    format!("local default branch '{branch}' is missing")
}

pub(crate) fn missing_remote_branch(branch: &str) -> String {
    format!("remote default branch 'origin/{branch}' is missing")
}

pub(crate) fn diverged(branch: &str) -> String {
    format!("{branch} has diverged from origin/{branch}")
}

pub(crate) fn ahead_of_origin(branch: &str) -> String {
    format!("{branch} is ahead of origin/{branch}")
}
