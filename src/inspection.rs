//! Repository readiness probing shared by the sync, refresh, status, and
//! replace use cases, and the canonical diagnostics for the conditions they
//! share.
//!
//! The probes return neutral findings; each use case maps them to its own
//! outcome vocabulary. Owning the shared message strings here keeps the
//! per-use-case reason enums from drifting apart.

use std::io;
use std::path::Path;

use crate::AppError;
use crate::git::{BranchTracking, RepositoryProbe};
use crate::repositories::{BranchName, RemoteUrl, RepositoryDefinition};

/// Whether grove owns the repository at an existing path: a worktree rooted at
/// the declared path whose origin matches the declaration.
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
/// decline.
pub(crate) fn ownership(
    git: &impl RepositoryProbe,
    repository: &RepositoryDefinition,
) -> Result<Ownership, AppError> {
    if !is_worktree_root(git, repository)? {
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

/// Whether the declared path is itself a worktree root. Git answers from any
/// directory inside a worktree, so a declared subdirectory would otherwise
/// lend its declaration the enclosing repository's identity, and a mutation
/// would act on that repository. Settled with `rev-parse` alone, so it is safe
/// to ask before any probe that may run repository-configured commands.
pub(crate) fn is_worktree_root(
    git: &impl RepositoryProbe,
    repository: &RepositoryDefinition,
) -> Result<bool, AppError> {
    if !repository.path().is_dir() {
        return Ok(false);
    }
    let Some(root) = git.worktree_root(repository.path())? else {
        return Ok(false);
    };
    Ok(root == repository.path().canonicalize()?)
}

/// What occupies a path grove would move, trash, or repoint as a whole
/// checkout, up to the origin that settles who owns it.
pub(crate) enum Checkout {
    Absent,
    SymbolicLink,
    NotAWorkTree,
    InsideAnotherRepository,
    MissingOrigin,
    Origin(RemoteUrl),
}

/// The final component is taken as written, so a symbolic link there is
/// reported rather than followed into the directory it reaches. Like
/// `ownership`, it probes with `rev-parse` and `config --get` alone, before
/// any caller reaches a probe that may run repository-configured commands.
pub(crate) fn checkout(git: &impl RepositoryProbe, path: &Path) -> Result<Checkout, AppError> {
    let metadata = match path.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(Checkout::Absent),
        Err(err) => return Err(err.into()),
    };
    if metadata.file_type().is_symlink() {
        return Ok(Checkout::SymbolicLink);
    }
    if !metadata.is_dir() {
        return Ok(Checkout::NotAWorkTree);
    }
    match git.worktree_root(path)? {
        None => return Ok(Checkout::NotAWorkTree),
        Some(root) if root != path.canonicalize()? => {
            return Ok(Checkout::InsideAnotherRepository);
        }
        Some(_) => {}
    }
    Ok(git.remote_url(path)?.map_or(Checkout::MissingOrigin, Checkout::Origin))
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

pub(crate) fn symbolic_link() -> &'static str {
    "path is a symbolic link"
}

pub(crate) fn inside_another_repository() -> &'static str {
    "path is inside another Git repository"
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
