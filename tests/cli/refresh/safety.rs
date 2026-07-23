use predicates::prelude::*;

use super::{current_branch, git_stdout, single_repository_config};
use crate::harness::{TestContext, path_with_wrapper, run_git};

#[cfg(unix)]
#[test]
fn refresh_fetch_failure_leaves_original_branch_checked_out() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = single_repository_config(&ctx, "blog", &remote.url(), None);
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
    let config = single_repository_config(&ctx, "blog", &remote.url(), None);
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
    let config = single_repository_config(&ctx, "blog", &remote.url(), None);
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
