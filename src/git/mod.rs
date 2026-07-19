//! Boundary for invoking the system git command.

mod client;
mod command;
mod default_branch;
mod remote;
mod update;
mod working_tree;

pub use client::{BranchDivergence, GitClient};
pub use command::CommandGitClient;
pub use remote::urls_match;
pub use update::GitUpdate;
