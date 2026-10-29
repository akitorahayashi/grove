use predicates::prelude::*;

#[cfg(unix)]
use crate::harness::fsmonitor_marker;
use crate::harness::{
    TestContext, current_branch, git_stdout, path_with_wrapper, repository_state, run_git,
};

#[cfg(unix)]
#[test]
fn refresh_fetch_failure_leaves_original_branch_checked_out() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = ctx.write_single_repository_config("blog", &remote.url(), None);
    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();
    let repository = ctx.workspace().join("blog");
    run_git(&repository, &["switch", "-c", "feature"]);
    let path = path_with_wrapper(
        &ctx,
        "refresh-fetch",
        "if [ \"$1\" = fetch ]; then echo fetch-failed >&2; exit 42; fi",
    );

    ctx.cli()
        .env("PATH", path)
        .arg("--config")
        .arg(config)
        .arg("refresh")
        .assert()
        .failure()
        .stderr(predicate::str::contains("fetch-failed"));

    assert_eq!(current_branch(&repository), "feature");
}

#[cfg(unix)]
#[test]
fn refresh_switch_failure_is_blocked_on_the_original_branch() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = ctx.write_single_repository_config("blog", &remote.url(), None);
    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();
    let repository = ctx.workspace().join("blog");
    run_git(&repository, &["switch", "-c", "feature"]);
    let path = path_with_wrapper(
        &ctx,
        "refresh-switch",
        "if [ \"$1\" = switch ]; then echo switch-failed >&2; exit 42; fi",
    );

    ctx.cli()
        .env("PATH", path)
        .arg("--config")
        .arg(config)
        .arg("refresh")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Blocked 1 repository"))
        .stderr(predicate::str::contains("switch-failed"));

    assert_eq!(current_branch(&repository), "feature");
}

#[cfg(unix)]
#[test]
fn refresh_merge_failure_does_not_restore_original_branch() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = ctx.write_single_repository_config("blog", &remote.url(), None);
    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();
    let repository = ctx.workspace().join("blog");
    run_git(&repository, &["switch", "-c", "feature"]);
    remote.add_commit("remote.txt", "remote\n");
    let path = path_with_wrapper(
        &ctx,
        "refresh-merge",
        "if [ \"$1\" = merge ]; then echo merge-failed >&2; exit 42; fi",
    );

    ctx.cli()
        .env("PATH", path)
        .arg("--config")
        .arg(config)
        .arg("refresh")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Refreshed 1 repository"))
        .stderr(predicate::str::contains("Blocked 1 repository"))
        .stderr(predicate::str::contains("x blog switched to main from feature; update failed:"))
        .stderr(predicate::str::contains("merge-failed"));

    assert_eq!(current_branch(&repository), "main");
}

#[test]
fn refresh_blocks_multiple_linked_worktrees_before_switching() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("shared");
    let initial_config = ctx.write_single_repository_config("primary", &remote.url(), None);
    ctx.cli().arg("--config").arg(&initial_config).arg("sync").assert().success();

    let primary = ctx.workspace().join("primary");
    let linked = ctx.workspace().join("linked");
    run_git(&primary, &["worktree", "add", "-b", "feature-linked", linked.to_str().unwrap()]);
    remote.add_commit("remote.txt", "remote\n");

    let config = ctx.write_config(&format!(
        r#"
version = 2

[repos.primary]
path = "primary"
url = "{}"

[repos.linked]
path = "linked"
url = "{}"
"#,
        remote.url(),
        remote.url()
    ));

    ctx.cli()
        .arg("--config")
        .arg(&config)
        .arg("refresh")
        .arg("--dry-run")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Blocked 2 repositories"))
        .stderr(predicate::str::contains(
            "multiple selected linked worktrees cannot all stay on 'main'",
        ));

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("refresh")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Blocked 2 repositories"))
        .stderr(predicate::str::contains(
            "multiple selected linked worktrees cannot all stay on 'main'",
        ))
        .stderr(predicate::str::contains("Refreshed ").not());

    assert_eq!(current_branch(&primary), "main");
    assert_eq!(current_branch(&linked), "feature-linked");
    assert_ne!(
        git_stdout(&primary, &["rev-parse", "main"]),
        git_stdout(&primary, &["rev-parse", "origin/main"])
    );
}

#[test]
fn refresh_rejects_inherited_repository_selectors_without_touching_either_repository() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = ctx.write_single_repository_config("blog", &remote.url(), None);
    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();
    let repository = ctx.workspace().join("blog");
    let other = ctx.root().join("other");
    run_git(ctx.root(), &["clone", &remote.url(), other.to_str().unwrap()]);
    run_git(&repository, &["switch", "-c", "feature"]);
    run_git(&other, &["switch", "-c", "feature"]);
    remote.add_commit("second.txt", "second\n");
    let before = (repository_state(&repository), repository_state(&other));

    ctx.cli()
        .env("GIT_DIR", other.join(".git"))
        .env("GIT_WORK_TREE", &repository)
        .arg("--config")
        .arg(config)
        .arg("refresh")
        .assert()
        .failure()
        .stderr(predicate::str::contains("GIT_DIR"))
        .stderr(predicate::str::contains("GIT_WORK_TREE"))
        .stderr(predicate::str::contains("unset them"))
        .stderr(predicate::str::contains(other.to_str().unwrap()).not());

    assert_eq!((repository_state(&repository), repository_state(&other)), before);
}

/// A declared directory inside an undeclared clone shares the clone's origin,
/// so only the worktree-root comparison distinguishes it from a repository
/// grove owns.
#[cfg(unix)]
#[test]
fn refresh_blocks_a_declared_directory_inside_another_worktree() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("outer");
    let outer = ctx.workspace().join("outer");
    run_git(ctx.workspace(), &["clone", &remote.url(), outer.to_str().unwrap()]);
    run_git(&outer, &["switch", "-c", "feature"]);
    std::fs::create_dir(outer.join("sub")).unwrap();
    remote.add_commit("second.txt", "second\n");
    run_git(&outer, &["fetch", "origin"]);
    let config = ctx.write_config(&format!(
        "version = 2\n[repos.sub]\npath = \"outer/sub\"\nurl = \"{}\"\n",
        remote.url()
    ));
    let before = repository_state(&outer);
    let marker = fsmonitor_marker(&ctx, &outer);

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("refresh")
        .arg("sub")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not a Git repository"));

    assert!(!marker.exists(), "refresh ran the enclosing repository's fsmonitor hook");
    assert_eq!(repository_state(&outer), before);
}

#[cfg(unix)]
#[test]
fn refresh_updates_a_repository_declared_through_an_in_root_symlink() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let real = ctx.workspace().join("real");
    run_git(ctx.workspace(), &["clone", &remote.url(), real.to_str().unwrap()]);
    std::os::unix::fs::symlink(&real, ctx.workspace().join("alias")).unwrap();
    remote.add_commit("second.txt", "second\n");
    let config = ctx.write_config(&format!(
        "version = 2\n[repos.blog]\npath = \"alias\"\nurl = \"{}\"\n",
        remote.url()
    ));

    ctx.cli().arg("--config").arg(config).arg("refresh").assert().success();

    assert_eq!(
        git_stdout(&real, &["rev-parse", "refs/heads/main"]),
        git_stdout(std::path::Path::new(&remote.url()), &["rev-parse", "refs/heads/main"])
    );
}

#[test]
fn refresh_updates_a_repository_at_the_grove_root() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    run_git(ctx.workspace(), &["clone", &remote.url(), "."]);
    std::fs::write(ctx.workspace().join(".git/info/exclude"), "grove.toml\n").unwrap();
    remote.add_commit("second.txt", "second\n");
    let config = ctx.write_config(&format!(
        "version = 2\n[repos.blog]\npath = \".\"\nurl = \"{}\"\n",
        remote.url()
    ));

    ctx.cli().arg("--config").arg(config).arg("refresh").assert().success();

    assert!(ctx.workspace().join("second.txt").is_file());
}

/// `GIT_CONFIG` swaps the file `git config` reads, so without the preflight
/// the ownership probe would see the configured URL while `git fetch` still
/// contacted the repository's actual origin.
#[cfg(unix)]
#[test]
fn refresh_rejects_an_inherited_git_config_that_would_fake_the_origin() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let actual = ctx.write_single_repository_config("blog", &remote.url(), None);
    ctx.cli().arg("--config").arg(&actual).arg("sync").assert().success();
    let repository = ctx.workspace().join("blog");
    let configured = "https://example.com/configured.git";
    let config = ctx.write_single_repository_config("blog", configured, None);
    let fake = ctx.root().join("fake-config");
    std::fs::write(&fake, format!("[remote \"origin\"]\n\turl = {configured}\n")).unwrap();
    let marker = ctx.root().join("fetch-ran");
    let path = path_with_wrapper(
        &ctx,
        "refresh-git-config",
        &format!("if [ \"$1\" = fetch ]; then : > \"{}\"; fi", marker.display()),
    );
    let before = repository_state(&repository);

    ctx.cli()
        .env("PATH", path)
        .env("GIT_CONFIG", &fake)
        .arg("--config")
        .arg(config)
        .arg("refresh")
        .assert()
        .failure()
        .stderr(predicate::str::contains("GIT_CONFIG"))
        .stderr(predicate::str::contains(fake.to_str().unwrap()).not());

    assert!(!marker.exists(), "refresh fetched an origin the configuration does not declare");
    assert_eq!(repository_state(&repository), before);
}

/// A grove whose root directory carries `name`, holding one clone that is a
/// commit behind its remote. Git prints such paths byte for byte.
fn grove_under(ctx: &TestContext, name: &std::ffi::OsStr) -> (std::path::PathBuf, String) {
    let remote = ctx.create_remote("blog");
    let root = ctx.root().join(name);
    std::fs::create_dir(&root).unwrap();
    run_git(&root, &["clone", &remote.url(), "blog"]);
    std::fs::write(
        root.join("grove.toml"),
        format!("version = 2\n[repos]\nblog = \"{}\"\n", remote.url()),
    )
    .unwrap();
    remote.add_commit("second.txt", "second\n");
    let tip = git_stdout(std::path::Path::new(&remote.url()), &["rev-parse", "refs/heads/main"]);
    (root, tip)
}

fn assert_status_and_refresh_manage(ctx: &TestContext, root: &std::path::Path, tip: &str) {
    ctx.cli()
        .arg("--config")
        .arg(root.join("grove.toml"))
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("clean"));
    ctx.cli().arg("--config").arg(root.join("grove.toml")).arg("refresh").assert().success();

    assert_eq!(git_stdout(&root.join("blog"), &["rev-parse", "refs/heads/main"]), tip);
}

#[test]
fn a_repository_under_a_directory_name_with_a_newline_is_managed() {
    let ctx = TestContext::new();
    let (root, tip) = grove_under(&ctx, std::ffi::OsStr::new("grove\nroot"));

    assert_status_and_refresh_manage(&ctx, &root, &tip);
}

/// Linux only: APFS refuses directory names that are not valid UTF-8.
#[cfg(target_os = "linux")]
#[test]
fn a_repository_under_a_non_utf8_directory_name_is_managed() {
    use std::os::unix::ffi::OsStrExt;

    let ctx = TestContext::new();
    let (root, tip) = grove_under(&ctx, std::ffi::OsStr::from_bytes(b"grove-\xff"));

    assert_status_and_refresh_manage(&ctx, &root, &tip);
}
