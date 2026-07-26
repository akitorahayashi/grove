use predicates::prelude::*;

use crate::harness::{TestContext, run_git};

#[cfg(unix)]
#[test]
fn sync_seeds_cache_for_existing_uncached_repository() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");

    let destination = ctx.workspace().join("blog");
    run_git(ctx.workspace(), &["clone", &remote.url(), destination.to_str().unwrap()]);
    assert_eq!(
        std::fs::read_dir(ctx.cache_root()).map(|dir| dir.count()).unwrap_or(0),
        0,
        "no cache entry exists before sync",
    );

    let config = ctx.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\n",
        remote.url()
    ));

    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();

    ctx.cli()
        .arg("cache")
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("blog.git"));
}

#[test]
fn sync_seeds_cache_from_dirty_existing_repository() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");

    let destination = ctx.workspace().join("blog");
    run_git(ctx.workspace(), &["clone", &remote.url(), destination.to_str().unwrap()]);
    std::fs::write(destination.join("README.md"), "local edit\n").unwrap();

    let config = ctx.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\n",
        remote.url()
    ));

    ctx.cli()
        .arg("--config")
        .arg(&config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("dirty working tree"));

    ctx.cli()
        .arg("cache")
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("blog.git"));

    assert_eq!(std::fs::read_to_string(destination.join("README.md")).unwrap(), "local edit\n");
}

#[test]
fn sync_seeds_cache_from_diverged_existing_repository() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");

    let destination = ctx.workspace().join("blog");
    run_git(ctx.workspace(), &["clone", &remote.url(), destination.to_str().unwrap()]);
    std::fs::write(destination.join("local.txt"), "local\n").unwrap();
    run_git(&destination, &["add", "local.txt"]);
    run_git(
        &destination,
        &["-c", "user.name=T", "-c", "user.email=t@e.x", "commit", "-m", "local"],
    );

    let config = ctx.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\n",
        remote.url()
    ));

    ctx.cli()
        .arg("--config")
        .arg(&config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("ahead of origin"));

    ctx.cli()
        .arg("cache")
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("blog.git"));
}

#[test]
fn sync_seeds_cache_from_detached_head_existing_repository() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");

    let destination = ctx.workspace().join("blog");
    run_git(ctx.workspace(), &["clone", &remote.url(), destination.to_str().unwrap()]);
    run_git(&destination, &["checkout", "--detach"]);

    let config = ctx.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\n",
        remote.url()
    ));

    ctx.cli()
        .arg("--config")
        .arg(&config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("detached HEAD"));

    ctx.cli()
        .arg("cache")
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("blog.git"));
}
