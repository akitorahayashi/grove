//! Assessment of `removed` declarations, shared by sync (which acts on it)
//! and status (which previews it), and the diagnostics both report.
//!
//! Absence from the configuration never implies removal; only an explicit
//! declaration reaches this module, and even then a directory grove cannot
//! prove it owns, or whose loss would take local-only work with it, is left
//! in place.

use std::collections::HashSet;
use std::path::Path;

use crate::AppError;
use crate::config::ResolvedConfig;
use crate::git::{Removal, RepositoryProbe};
use crate::repositories::{RemovedRepository, RepositoryDefinition};

/// What sync would do with a removed declaration's old path, decided by the
/// first matching row of the documented judgment table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Assessment {
    Absent,
    Blocked(BlockedReason),
    Move { to: String },
    Trash,
}

/// Why sync leaves a removed declaration's old path in place.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BlockedReason {
    SymbolicLink,
    NotAWorkTree,
    InsideAnotherRepository,
    MissingOrigin,
    RemoteUrlMismatch,
    LinkedWorktree,
    HasLinkedWorktrees,
    LocalState(Vec<LocalState>),
    MoveFailed(String),
    TrashFailed(String),
}

/// Work that exists only in the old repository and would be lost with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LocalState {
    UncommittedChanges,
    Stash,
    UnpushedCommits,
}

impl BlockedReason {
    pub fn message(&self) -> String {
        match self {
            Self::SymbolicLink => "path is a symbolic link".to_string(),
            Self::NotAWorkTree => crate::inspection::destination_not_git_repository().to_string(),
            Self::InsideAnotherRepository => "path is inside another Git repository".to_string(),
            Self::MissingOrigin => crate::inspection::missing_origin().to_string(),
            Self::RemoteUrlMismatch => {
                "remote URL matches neither the removed nor the moved_to URL".to_string()
            }
            Self::LinkedWorktree => "path is a linked worktree".to_string(),
            Self::HasLinkedWorktrees => "repository has linked worktrees".to_string(),
            Self::LocalState(states) => {
                let states = states.iter().copied().map(LocalState::as_str).collect::<Vec<_>>();
                format!("local-only state: {}", states.join(", "))
            }
            Self::MoveFailed(message) => format!("move failed: {message}"),
            Self::TrashFailed(message) => format!("trash failed: {message}"),
        }
    }
}

impl LocalState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UncommittedChanges => "uncommitted changes",
            Self::Stash => "stash",
            Self::UnpushedCommits => "unpushed commits",
        }
    }
}

/// The removed declarations a command covers: all of them, or with explicit
/// targets only those moving to a selected repository, so `gv sync <repo>`
/// never touches anything unrelated to `<repo>`.
pub(crate) fn select<'a>(
    config: &'a ResolvedConfig,
    selected: &[&RepositoryDefinition],
    targeted: bool,
) -> Vec<&'a RemovedRepository> {
    let names = selected.iter().map(|repository| repository.name()).collect::<HashSet<_>>();
    config
        .removed()
        .iter()
        .filter(|removed| !targeted || removed.moved_to().is_some_and(|to| names.contains(to)))
        .collect()
}

/// The live repository a removed declaration moves to.
pub(crate) fn destination<'a>(
    config: &'a ResolvedConfig,
    removed: &RemovedRepository,
) -> Option<&'a RepositoryDefinition> {
    let target = removed.moved_to()?;
    config.repositories().iter().find(|repository| repository.name() == target)
}

/// The probe order mirrors `inspection::ownership`: ownership is settled with
/// `rev-parse` and `config --get`, which never run repository-configured
/// commands, before `git status` may invoke a `core.fsmonitor` hook.
pub(crate) fn assess(
    git: &(impl RepositoryProbe + Removal),
    removed: &RemovedRepository,
    destination: Option<&RepositoryDefinition>,
) -> Result<Assessment, AppError> {
    let path = removed.path();
    let metadata = match path.symlink_metadata() {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Assessment::Absent),
        Err(err) => return Err(err.into()),
    };
    if metadata.file_type().is_symlink() {
        return Ok(Assessment::Blocked(BlockedReason::SymbolicLink));
    }
    if !metadata.is_dir() {
        return Ok(Assessment::Blocked(BlockedReason::NotAWorkTree));
    }
    match git.worktree_root(path)? {
        None => return Ok(Assessment::Blocked(BlockedReason::NotAWorkTree)),
        Some(root) if root != path.canonicalize()? => {
            return Ok(Assessment::Blocked(BlockedReason::InsideAnotherRepository));
        }
        Some(_) => {}
    }
    let Some(origin) = git.remote_url(path)? else {
        return Ok(Assessment::Blocked(BlockedReason::MissingOrigin));
    };
    let owned = origin.same_identity(removed.url())
        || destination.is_some_and(|destination| origin.same_identity(destination.url()));
    if !owned {
        return Ok(Assessment::Blocked(BlockedReason::RemoteUrlMismatch));
    }

    if git.is_linked_worktree(path)? {
        return Ok(Assessment::Blocked(BlockedReason::LinkedWorktree));
    }
    if git.has_linked_worktrees(path)? {
        return Ok(Assessment::Blocked(BlockedReason::HasLinkedWorktrees));
    }

    if let Some(destination) = destination
        && !exists(destination.path())?
    {
        return Ok(Assessment::Move { to: destination.display_path().to_string() });
    }

    let mut states = Vec::new();
    if !git.worktree_status(path)?.is_some_and(|status| status.is_clean()) {
        states.push(LocalState::UncommittedChanges);
    }
    if git.has_stash(path)? {
        states.push(LocalState::Stash);
    }
    if git.has_unpushed_commits(path)? {
        states.push(LocalState::UnpushedCommits);
    }
    if !states.is_empty() {
        return Ok(Assessment::Blocked(BlockedReason::LocalState(states)));
    }

    Ok(Assessment::Trash)
}

fn exists(path: &Path) -> Result<bool, AppError> {
    match path.symlink_metadata() {
        Ok(_) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(err) => Err(err.into()),
    }
}
