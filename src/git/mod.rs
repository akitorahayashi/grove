//! Boundary for invoking the system git command.

mod client;
mod command;
mod default_branch;
mod progress;
mod remote;
mod update;
mod working_tree;

pub use client::{BranchDivergence, GitClient, GitProgressSink, NoopGitProgressSink};
pub use command::CommandGitClient;
pub use progress::{GitProgress, GitProgressParser};
pub use remote::{redact_url_for_display, urls_match};
pub use update::GitUpdate;
