use std::path::Path;

use predicates::prelude::*;

use crate::harness::{TestContext, run_git};

#[test]
fn refresh_missing_repository_fails_without_cloning() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = single_repository_config(&ctx, "blog", &remote.url(), None);

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("refresh")
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Blocked 1 repository"))
        .stderr(predicate::str::contains("x blog repository is missing; run gv sync to clone it"));

    assert!(!ctx.workspace().join("blog").exists());
}

#[test]
fn refresh_alias_updates_only_selected_repository_and_stays_on_default_branch() {
    let ctx = TestContext::new();
    let first = ctx.create_remote("first");
    let second = ctx.create_remote("second");
    let config = ctx.write_config(&format!(
        r#"
version = 1

[repos.first]
path = "first"
url = "{}"

[repos.second]
path = "second"
url = "{}"
"#,
        first.url(),
        second.url()
    ));
    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();
    for name in ["first", "second"] {
        run_git(&ctx.workspace().join(name), &["switch", "-c", "feature"]);
    }
    first.add_commit("remote.txt", "remote\n");

    let feature_revision = git_stdout(&ctx.workspace().join("first"), &["rev-parse", "feature"]);
    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("rf")
        .arg("first")
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Checked 1 repository"))
        .stderr(predicate::str::contains("Fetched 1 repository"))
        .stderr(predicate::str::contains("Refreshed 1 repository"))
        .stderr(predicate::str::contains("~ first main"))
        .stderr(predicate::str::contains("from feature"));

    assert_eq!(current_branch(&ctx.workspace().join("first")), "main");
    assert_eq!(current_branch(&ctx.workspace().join("second")), "feature");
    assert_eq!(
        git_stdout(&ctx.workspace().join("first"), &["rev-parse", "main"]),
        git_stdout(&ctx.workspace().join("first"), &["rev-parse", "origin/main"])
    );
    assert_eq!(
        git_stdout(&ctx.workspace().join("first"), &["rev-parse", "feature"]),
        feature_revision
    );
}

#[test]
fn refresh_switches_to_equal_default_branch_and_reports_switch() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = single_repository_config(&ctx, "blog", &remote.url(), None);
    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();
    let repository = ctx.workspace().join("blog");
    run_git(&repository, &["switch", "-c", "feature/login"]);
    let feature_revision = git_stdout(&repository, &["rev-parse", "feature/login"]);

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("refresh")
        .assert()
        .success()
        .stderr(predicate::str::contains("Refreshed 1 repository"))
        .stderr(predicate::str::contains("> blog main from feature/login"))
        .stderr(predicate::str::contains("~ blog").not());

    assert_eq!(current_branch(&repository), "main");
    assert_eq!(git_stdout(&repository, &["rev-parse", "feature/login"]), feature_revision);
}

#[test]
fn refresh_fast_forwards_default_branch_that_is_already_checked_out() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = single_repository_config(&ctx, "blog", &remote.url(), None);
    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();
    let repository = ctx.workspace().join("blog");
    remote.add_commit("remote.txt", "remote\n");

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("refresh")
        .assert()
        .success()
        .stderr(predicate::str::contains("~ blog main"))
        .stderr(predicate::str::contains(" from ").not());

    assert_eq!(current_branch(&repository), "main");
    assert_eq!(
        git_stdout(&repository, &["rev-parse", "main"]),
        git_stdout(&repository, &["rev-parse", "origin/main"])
    );
}

#[test]
fn refresh_omits_current_default_branch_from_report() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = single_repository_config(&ctx, "blog", &remote.url(), None);
    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("refresh")
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Checked 1 repository"))
        .stderr(predicate::str::contains("Fetched 1 repository"))
        .stderr(predicate::str::contains("~ blog").not())
        .stderr(predicate::str::contains("> blog").not());
}

#[test]
fn refresh_processes_independent_repositories_after_a_skip() {
    let ctx = TestContext::new();
    let dirty = ctx.create_remote("dirty");
    let ready = ctx.create_remote("ready");
    let config = ctx.write_config(&format!(
        r#"
version = 1

[repos.dirty]
path = "dirty"
url = "{}"

[repos.ready]
path = "ready"
url = "{}"
"#,
        dirty.url(),
        ready.url()
    ));
    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();
    std::fs::write(ctx.workspace().join("dirty/draft.txt"), "dirty\n").unwrap();
    run_git(&ctx.workspace().join("ready"), &["switch", "-c", "feature"]);

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("refresh")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Skipped 1 repository"))
        .stderr(predicate::str::contains("! dirty dirty working tree"))
        .stderr(predicate::str::contains("> ready main from feature"));

    assert_eq!(current_branch(&ctx.workspace().join("dirty")), "main");
    assert_eq!(current_branch(&ctx.workspace().join("ready")), "main");
}

#[test]
fn refresh_blocks_detached_head_without_mutation() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = single_repository_config(&ctx, "blog", &remote.url(), None);
    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();
    let repository = ctx.workspace().join("blog");
    let revision = git_stdout(&repository, &["rev-parse", "HEAD"]);
    run_git(&repository, &["checkout", "--detach"]);

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("refresh")
        .assert()
        .failure()
        .stderr(predicate::str::contains("x blog detached HEAD cannot be refreshed safely"));

    assert_eq!(git_stdout(&repository, &["rev-parse", "HEAD"]), revision);
    assert!(current_branch(&repository).is_empty());
}

#[test]
fn refresh_blocks_ahead_and_diverged_branches_before_switching() {
    let ctx = TestContext::new();
    let ahead = ctx.create_remote("ahead");
    let diverged = ctx.create_remote("diverged");
    let config = ctx.write_config(&format!(
        r#"
version = 1

[repos.ahead]
path = "ahead"
url = "{}"

[repos.diverged]
path = "diverged"
url = "{}"
"#,
        ahead.url(),
        diverged.url()
    ));
    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();

    for name in ["ahead", "diverged"] {
        let repository = ctx.workspace().join(name);
        commit_local(&repository, "local.txt");
        run_git(&repository, &["switch", "-c", "feature"]);
    }
    diverged.add_commit("remote.txt", "remote\n");

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("refresh")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Blocked 2 repositories"))
        .stderr(predicate::str::contains("x ahead main is ahead of origin/main"))
        .stderr(predicate::str::contains("x diverged main has diverged from origin/main"));

    assert_eq!(current_branch(&ctx.workspace().join("ahead")), "feature");
    assert_eq!(current_branch(&ctx.workspace().join("diverged")), "feature");
}

#[test]
fn refresh_blocks_missing_configured_branch_without_creating_it() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let initial_config = single_repository_config(&ctx, "blog", &remote.url(), None);
    ctx.cli().arg("--config").arg(&initial_config).arg("sync").assert().success();
    let configured = single_repository_config(&ctx, "blog", &remote.url(), Some("trunk"));

    ctx.cli()
        .arg("--config")
        .arg(configured)
        .arg("refresh")
        .assert()
        .failure()
        .stderr(predicate::str::contains("local default branch 'trunk' is missing"));

    assert!(git_stdout(&ctx.workspace().join("blog"), &["branch", "--list", "trunk"]).is_empty());
}

#[test]
fn refresh_blocks_missing_remote_configured_branch_without_switching() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let initial_config = single_repository_config(&ctx, "blog", &remote.url(), None);
    ctx.cli().arg("--config").arg(&initial_config).arg("sync").assert().success();
    let repository = ctx.workspace().join("blog");
    run_git(&repository, &["switch", "-c", "trunk"]);
    run_git(&repository, &["switch", "main"]);
    let configured = single_repository_config(&ctx, "blog", &remote.url(), Some("trunk"));

    ctx.cli()
        .arg("--config")
        .arg(configured)
        .arg("refresh")
        .assert()
        .failure()
        .stderr(predicate::str::contains("remote default branch 'origin/trunk' is missing"));

    assert_eq!(current_branch(&repository), "main");
    assert!(!git_stdout(&repository, &["branch", "--list", "trunk"]).is_empty());
}

#[test]
fn refresh_reports_invalid_destinations_origin_and_default_branch() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("remote");
    let initial_config = single_repository_config(&ctx, "no-default", &remote.url(), None);
    ctx.cli().arg("--config").arg(&initial_config).arg("sync").assert().success();
    run_git(
        &ctx.workspace().join("no-default"),
        &["symbolic-ref", "--delete", "refs/remotes/origin/HEAD"],
    );

    std::fs::create_dir(ctx.workspace().join("not-git")).unwrap();
    let no_origin = ctx.workspace().join("no-origin");
    std::fs::create_dir(&no_origin).unwrap();
    run_git(&no_origin, &["init", "-b", "main"]);
    commit_local(&no_origin, "README.md");

    let config = ctx.write_config(&format!(
        r#"
version = 1

[repos.not-git]
path = "not-git"
url = "{}"

[repos.no-origin]
path = "no-origin"
url = "{}"

[repos.no-default]
path = "no-default"
url = "{}"
"#,
        remote.url(),
        remote.url(),
        remote.url()
    ));

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("refresh")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Blocked 3 repositories"))
        .stderr(predicate::str::contains(
            "x not-git destination exists but is not a Git repository",
        ))
        .stderr(predicate::str::contains("x no-origin remote origin is missing"))
        .stderr(predicate::str::contains(
            "x no-default remote default branch cannot be determined",
        ));
}

#[test]
fn refresh_dry_run_does_not_fetch_or_switch() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = single_repository_config(&ctx, "blog", &remote.url(), None);
    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();
    let repository = ctx.workspace().join("blog");
    run_git(&repository, &["switch", "-c", "feature"]);
    let tracking_revision = git_stdout(&repository, &["rev-parse", "origin/main"]);
    remote.add_commit("remote.txt", "remote\n");

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("refresh")
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Would fetch and refresh 1 repository"))
        .stderr(predicate::str::contains("Checked ").not())
        .stderr(predicate::str::contains("Fetching repositories").not());

    assert_eq!(current_branch(&repository), "feature");
    assert_eq!(git_stdout(&repository, &["rev-parse", "origin/main"]), tracking_revision);
}

#[test]
fn refresh_dry_run_blocks_locally_visible_ahead_branch() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = single_repository_config(&ctx, "blog", &remote.url(), None);
    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();
    let repository = ctx.workspace().join("blog");
    commit_local(&repository, "local.txt");
    run_git(&repository, &["switch", "-c", "feature"]);

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("refresh")
        .arg("--dry-run")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Blocked 1 repository"))
        .stderr(predicate::str::contains("main is ahead of origin/main"))
        .stderr(predicate::str::contains("Would fetch and refresh").not());

    assert_eq!(current_branch(&repository), "feature");
}

#[test]
fn refresh_redacts_remote_url_mismatch_details() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let initial_config = single_repository_config(&ctx, "blog", &remote.url(), None);
    ctx.cli().arg("--config").arg(&initial_config).arg("sync").assert().success();
    let repository = ctx.workspace().join("blog");
    run_git(
        &repository,
        &[
            "remote",
            "set-url",
            "origin",
            "https://user:actual-secret@example.com/org/repo.git?token=actual-token",
        ],
    );
    let config = single_repository_config(
        &ctx,
        "blog",
        "https://user:expected-secret@example.com/org/repo.git?token=expected-token",
        None,
    );

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("refresh")
        .assert()
        .failure()
        .stderr(predicate::str::contains("actual:"))
        .stderr(predicate::str::contains("expected:"))
        .stderr(predicate::str::contains("actual-secret").not())
        .stderr(predicate::str::contains("actual-token").not())
        .stderr(predicate::str::contains("expected-secret").not())
        .stderr(predicate::str::contains("expected-token").not());
}

#[cfg(unix)]
#[test]
fn refresh_fetch_failure_leaves_original_branch_checked_out() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = single_repository_config(&ctx, "blog", &remote.url(), None);
    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();
    let repository = ctx.workspace().join("blog");
    run_git(&repository, &["switch", "-c", "feature"]);
    let path =
        install_git_wrapper(&ctx, "if [ \"$1\" = fetch ]; then echo fetch-failed >&2; exit 42; fi");

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
    let config = single_repository_config(&ctx, "blog", &remote.url(), None);
    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();
    let repository = ctx.workspace().join("blog");
    run_git(&repository, &["switch", "-c", "feature"]);
    let path = install_git_wrapper(
        &ctx,
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
    let config = single_repository_config(&ctx, "blog", &remote.url(), None);
    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();
    let repository = ctx.workspace().join("blog");
    run_git(&repository, &["switch", "-c", "feature"]);
    remote.add_commit("remote.txt", "remote\n");
    let path =
        install_git_wrapper(&ctx, "if [ \"$1\" = merge ]; then echo merge-failed >&2; exit 42; fi");

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
    let initial_config = single_repository_config(&ctx, "primary", &remote.url(), None);
    ctx.cli().arg("--config").arg(&initial_config).arg("sync").assert().success();

    let primary = ctx.workspace().join("primary");
    let linked = ctx.workspace().join("linked");
    run_git(&primary, &["worktree", "add", "-b", "feature-linked", linked.to_str().unwrap()]);
    remote.add_commit("remote.txt", "remote\n");

    let config = ctx.write_config(&format!(
        r#"
version = 1

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

fn single_repository_config(
    ctx: &TestContext,
    name: &str,
    url: &str,
    default_branch: Option<&str>,
) -> std::path::PathBuf {
    let configured_branch =
        default_branch.map(|branch| format!("default_branch = \"{branch}\"\n")).unwrap_or_default();
    ctx.write_config(&format!(
        "version = 1\n[repos.{name}]\npath = \"{name}\"\nurl = \"{url}\"\n{configured_branch}"
    ))
}

fn current_branch(repository: &Path) -> String {
    git_stdout(repository, &["branch", "--show-current"])
}

fn git_stdout(repository: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .current_dir(repository)
        .args(args)
        .output()
        .expect("failed to inspect Git repository");
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn commit_local(repository: &Path, file: &str) {
    std::fs::write(repository.join(file), "local\n").unwrap();
    run_git(repository, &["add", file]);
    run_git(
        repository,
        &["-c", "user.name=Grove Test", "-c", "user.email=grove@example.com", "commit", "-m", file],
    );
}

#[cfg(unix)]
fn install_git_wrapper(ctx: &TestContext, behavior: &str) -> std::ffi::OsString {
    use std::os::unix::fs::PermissionsExt;

    let command = std::process::Command::new("sh").args(["-c", "command -v git"]).output().unwrap();
    let real_git = String::from_utf8_lossy(&command.stdout).trim().to_string();
    let bin = ctx.root().join("refresh-git-wrapper-bin");
    std::fs::create_dir(&bin).unwrap();
    let wrapper = bin.join("git");
    std::fs::write(&wrapper, format!("#!/bin/sh\n{behavior}\nexec \"{real_git}\" \"$@\"\n"))
        .unwrap();
    let mut permissions = std::fs::metadata(&wrapper).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&wrapper, permissions).unwrap();
    let mut paths =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect::<Vec<_>>();
    paths.insert(0, bin);
    std::env::join_paths(paths).unwrap()
}
