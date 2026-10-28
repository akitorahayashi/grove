pub(crate) mod git_process;
pub(crate) mod test_context;

#[cfg(unix)]
pub(crate) use git_process::fsmonitor_marker;
#[cfg(unix)]
pub(crate) use git_process::path_with_wrapper;
pub(crate) use git_process::{commit_file, git_command, repository_state};
pub(crate) use test_context::{RemoteRepository, TestContext, current_branch, git_stdout, run_git};
