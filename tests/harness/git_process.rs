use std::path::Path;

use super::{TestContext, run_git};

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
