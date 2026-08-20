//! Repository readiness probing shared by the sync, refresh, and status use
//! cases, and the canonical diagnostics for the conditions they share.
//!
//! The probes return neutral findings; each use case maps them to its own
//! outcome vocabulary. Owning the shared message strings here keeps the
//! per-use-case reason enums from drifting apart.

use crate::AppError;
use crate::git::{BranchTracking, RepositoryProbe};
use crate::repositories::{BranchName, RepositoryDefinition};

/// Whether grove owns the repository at an existing path: a work tree whose
/// origin matches the declaration.
pub(crate) enum Ownership {
    NotAWorkTree,
    MissingOrigin,
    UrlMismatch { actual: String, expected: String },
    Owned,
}

/// The side-effect-free ownership gate shared by `inspect` and the status
/// fetch preflight.
///
/// The probe order is a contract, not an optimization target. `git status`
/// honors the repository's own `core.fsmonitor`, which Git may run as an
/// external hook command, while `rev-parse` and `config --get` never invoke
/// one. Establishing ownership from those two first keeps sync and refresh
/// from running repository-configured behavior in a repository they go on to
/// decline. `worktree_status` alone would report the absent work tree, so
/// `is_work_tree` looks redundant in `inspect`; it is the side-effect-free
/// gate that makes the later observation safe to reach.
pub(crate) fn ownership(
    git: &impl RepositoryProbe,
    repository: &RepositoryDefinition,
) -> Result<Ownership, AppError> {
    if !repository.path().is_dir() || !git.is_work_tree(repository.path())? {
        return Ok(Ownership::NotAWorkTree);
    }

    let Some(actual_url) = git.remote_url(repository.path())? else {
        return Ok(Ownership::MissingOrigin);
    };
    if !actual_url.matches(repository.url()) {
        return Ok(Ownership::UrlMismatch {
            actual: actual_url.to_string(),
            expected: repository.url().to_string(),
        });
    }
    Ok(Ownership::Owned)
}

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
    Ready { default_branch: BranchName },
}

pub(crate) fn inspect(
    git: &impl RepositoryProbe,
    repository: &RepositoryDefinition,
) -> Result<Readiness, AppError> {
    match ownership(git, repository)? {
        Ownership::NotAWorkTree => return Ok(Readiness::NotAWorkTree),
        Ownership::MissingOrigin => return Ok(Readiness::MissingOrigin),
        Ownership::UrlMismatch { actual, expected } => {
            return Ok(Readiness::UrlMismatch { actual, expected });
        }
        Ownership::Owned => {}
    }

    let Some(worktree) = git.worktree_status(repository.path())? else {
        return Ok(Readiness::NotAWorkTree);
    };
    if worktree.branch().is_none() {
        return Ok(Readiness::DetachedHead);
    }

    if !worktree.is_clean() {
        return Ok(Readiness::DirtyTree);
    }

    let Some(default_branch) =
        git.default_branch(repository.path(), repository.default_branch())?
    else {
        return Ok(Readiness::NoDefaultBranch);
    };

    Ok(Readiness::Ready { default_branch })
}

/// The default branch's standing against its upstream, already classified by
/// grove's safety invariant: local-only commits are never rewritten, so any
/// branch that is ahead cannot be advanced. Owning the classification here
/// keeps the sync and refresh previews and the status view from restating the
/// thresholds; `git/branch_update.rs` re-derives them under lock as the
/// enforcement side.
pub(crate) enum BranchReadiness {
    MissingLocal,
    MissingRemote,
    Diverged { ahead: u32, behind: u32 },
    AheadOfOrigin { ahead: u32 },
    FastForwardable { behind: u32 },
}

pub(crate) fn branch_readiness(
    git: &impl RepositoryProbe,
    repository: &RepositoryDefinition,
    branch: &BranchName,
) -> Result<BranchReadiness, AppError> {
    Ok(match git.branch_tracking(repository.path(), branch)? {
        BranchTracking::MissingLocal => BranchReadiness::MissingLocal,
        BranchTracking::MissingRemote => BranchReadiness::MissingRemote,
        BranchTracking::Divergence { ahead, behind } if ahead > 0 && behind > 0 => {
            BranchReadiness::Diverged { ahead, behind }
        }
        BranchTracking::Divergence { ahead, .. } if ahead > 0 => {
            BranchReadiness::AheadOfOrigin { ahead }
        }
        BranchTracking::Divergence { behind, .. } => BranchReadiness::FastForwardable { behind },
    })
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
