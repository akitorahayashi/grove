use std::fs;

use predicates::prelude::*;

use crate::harness::TestContext;

#[test]
fn clone_places_repository_and_populates_cache() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");

    ctx.cli()
        .arg("clone")
        .arg(remote.url())
        .arg("cloned")
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("+ "))
        .stderr(predicate::str::contains("cloned"))
        .stderr(predicate::str::contains("(cached)"));

    assert!(ctx.workspace().join("cloned").join(".git").exists());
    let entries = fs::read_dir(ctx.cache_root())
        .expect("cache root should exist")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join("url").is_file())
        .count();
    assert_eq!(entries, 1);
}

#[test]
fn clone_infers_destination_from_url() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");

    ctx.cli().arg("clone").arg(remote.url()).assert().success();

    assert!(ctx.workspace().join("blog").join(".git").exists());
}

#[test]
fn clone_reuses_cache_on_second_run() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");

    ctx.cli().arg("clone").arg(remote.url()).arg("first").assert().success();

    ctx.cli()
        .arg("clone")
        .arg(remote.url())
        .arg("second")
        .assert()
        .success()
        .stderr(predicate::str::contains("(from cache)"));

    assert!(ctx.workspace().join("second").join(".git").exists());
}

#[test]
fn clone_rejects_existing_non_empty_destination() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let destination = ctx.workspace().join("occupied");
    fs::create_dir_all(&destination).unwrap();
    fs::write(destination.join("keep.txt"), "existing\n").unwrap();

    ctx.cli()
        .arg("clone")
        .arg(remote.url())
        .arg("occupied")
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn clone_rejects_config_flag() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = ctx.write_config("version = 1\n");

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("clone")
        .arg(remote.url())
        .arg("cloned")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--config cannot be used with clone"));
}
