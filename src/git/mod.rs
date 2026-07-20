//! Boundary for invoking the system git command.

mod client;
mod command;
mod default_branch;
mod progress;
mod remote;
mod update;

pub use client::{GitClient, GitProgressSink, NoopGitProgressSink};
pub use command::CommandGitClient;
pub use progress::{GitProgress, parse_git_progress};
pub use remote::urls_match;
pub use update::{GitRefreshOutcome, GitUpdate, GitUpdateBlock, GitUpdateOutcome, Restoration};
