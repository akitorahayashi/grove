//! Boundary for invoking the system git command.

mod branch_update;
mod cache_entry;
mod client;
mod clone;
mod command;
mod environment;
mod probe;
mod progress;
mod removal;
mod symbolic_ref;
mod tracking;
mod update;
mod worktree;

pub(crate) use client::RepositoryLock;
pub use client::{
    CacheEntry, DefaultBranch, GitClient, GitProgressSink, NoopGitProgressSink, Removal,
    RepositoryProbe,
};
pub(crate) use clone::{CacheDecision as CloneCacheDecision, CloneCommand, CloneInvocation};
pub use command::CommandGitClient;
#[cfg(test)]
pub(crate) use command::tests as process_fixtures;
pub use progress::{GitProgress, parse_git_progress};
pub use tracking::BranchTracking;
pub use update::{GitRefreshOutcome, GitUpdate, GitUpdateBlock, GitUpdateOutcome, Restoration};
pub use worktree::WorktreeStatus;
