use predicates::prelude::*;

use super::{current_branch, git_stdout, single_repository_config};
use crate::harness::{TestContext, commit_file, run_git};

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
    commit_file(&repository, "local.txt");
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
