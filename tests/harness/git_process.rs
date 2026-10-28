use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use super::{TestContext, run_git};

/// Global and system configuration replaced with nothing, so a developer's
/// signing, hooks, aliases, or templates reach neither fixtures nor `gv`.
pub(crate) const ISOLATED_CONFIGURATION: [(&str, &str); 2] =
    [("GIT_CONFIG_GLOBAL", "/dev/null"), ("GIT_CONFIG_NOSYSTEM", "1")];

/// Every inherited `GIT_*` variable, including repository selectors such as
/// `GIT_DIR` that would otherwise point a child at an ambient repository.
/// Children drop them individually; the test process environment is never
/// mutated, because tests run in parallel.
pub(crate) fn inherited_git_variables() -> impl Iterator<Item = OsString> {
    std::env::vars_os()
        .map(|(name, _)| name)
        .filter(|name| name.to_str().is_some_and(|name| name.starts_with("GIT_")))
}

pub(crate) fn git_command(directory: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(directory);
    for name in inherited_git_variables() {
        command.env_remove(name);
    }
    command.envs(ISOLATED_CONFIGURATION);
    command
}

pub(crate) fn commit_file(repository: &Path, file: &str) {
    std::fs::write(repository.join(file), "local\n").expect("failed to write local commit file");
    run_git(repository, &["add", file]);
    run_git(
        repository,
        &["-c", "user.name=Grove Test", "-c", "user.email=grove@example.com", "commit", "-m", file],
    );
}

#[cfg(unix)]
pub(crate) fn path_with_wrapper(
    ctx: &TestContext,
    fixture: &str,
    behavior: &str,
) -> std::ffi::OsString {
    use std::os::unix::fs::PermissionsExt;

    let command = std::process::Command::new("sh")
        .args(["-c", "command -v git"])
        .output()
        .expect("failed to locate git");
    let real_git = String::from_utf8_lossy(&command.stdout).trim().to_string();
    let bin = ctx.root().join(format!("{fixture}-git-bin"));
    std::fs::create_dir(&bin).expect("failed to create Git wrapper directory");
    let wrapper = bin.join("git");
    std::fs::write(&wrapper, format!("#!/bin/sh\n{behavior}\nexec \"{real_git}\" \"$@\"\n"))
        .expect("failed to write Git wrapper");
    let mut permissions =
        std::fs::metadata(&wrapper).expect("failed to inspect Git wrapper").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&wrapper, permissions).expect("failed to chmod Git wrapper");
    let mut paths =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect::<Vec<_>>();
    paths.insert(0, bin);
    std::env::join_paths(paths).expect("failed to join PATH")
}

/// Configures `repository`'s `core.fsmonitor` as a hook that leaves the
/// returned marker behind. `git status` runs that hook while the ownership
/// probes do not, so the marker shows whether grove reached `git status`.
#[cfg(unix)]
pub(crate) fn fsmonitor_marker(ctx: &TestContext, repository: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let marker = ctx.root().join("fsmonitor-ran");
    let hook = ctx.root().join("fsmonitor-hook");
    std::fs::write(&hook, format!("#!/bin/sh\n: > \"{}\"\nexit 1\n", marker.display()))
        .expect("failed to write fsmonitor hook");
    let mut permissions =
        std::fs::metadata(&hook).expect("failed to inspect fsmonitor hook").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&hook, permissions).expect("failed to chmod fsmonitor hook");
    run_git(repository, &["config", "core.fsmonitor", hook.to_str().unwrap()]);
    marker
}

/// Refs, the checked-out branch, and working-tree changes: what a managed
/// Git command that reached the wrong repository would alter.
pub(crate) fn repository_state(repository: &Path) -> String {
    let output = git_command(repository)
        .args(["for-each-ref", "--format=%(refname) %(objectname)"])
        .output()
        .expect("failed to list references");
    let status = git_command(repository)
        .args(["status", "--porcelain=v2", "--branch", "--untracked-files=all"])
        .output()
        .expect("failed to inspect working tree");
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&status.stdout)
    )
}
