//! Boundary for invoking the system git command.

mod branch_update;
mod cache_entry;
mod client;
mod command;
mod default_branch;
mod probe;
mod progress;
mod remote;
mod tracking;
mod update;
mod worktree;

pub(crate) use client::RepositoryLock;
pub use client::{
    CacheEntry, DefaultBranch, GitClient, GitProgressSink, NoopGitProgressSink, RepositoryProbe,
};
pub use command::CommandGitClient;
pub use progress::{GitProgress, parse_git_progress};
pub use remote::urls_match;
pub use tracking::BranchTracking;
pub use update::{GitRefreshOutcome, GitUpdate, GitUpdateBlock, GitUpdateOutcome, Restoration};
pub use worktree::WorktreeStatus;
