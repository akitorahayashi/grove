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
        .stderr(predicate::str::contains("gv: clone cache (cached)"))
        .stderr(predicate::str::contains("cloned"))
        .stderr(predicate::str::contains("Cloning into"));

    assert!(ctx.workspace().join("cloned").join(".git").exists());
    assert!(!ctx.workspace().join("cloned/.git/objects/info/alternates").exists());
    let entries = fs::read_dir(ctx.cache_root())
        .expect("cache root should exist")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join("url").is_file())
        .count();
    assert_eq!(entries, 1);
}

#[test]
fn clone_forwards_git_options_and_explicit_destination() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");

    ctx.cli()
        .arg("clone")
        .args(["--no-checkout", "--origin", "upstream"])
        .arg(remote.url())
        .arg("configured")
        .assert()
        .success();

    let repository = ctx.workspace().join("configured");
    assert!(repository.join(".git").exists());
    assert!(!repository.join("README.md").exists());
    let config = fs::read_to_string(repository.join(".git/config")).unwrap();
    assert!(config.contains("[remote \"upstream\"]"));
}

#[test]
fn clone_delegates_history_selection_to_git_without_cache_injection() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    remote.add_commit("second.txt", "second\n");
    let url = format!("file://{}", remote.url());

    ctx.cli()
        .arg("clone")
        .args(["--depth", "1"])
        .arg(url)
        .arg("shallow")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "clone cache bypassed: history or object selection semantics",
        ));

    assert!(ctx.workspace().join("shallow/.git/shallow").is_file());
}

#[test]
fn clone_forwards_git_config_options() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");

    ctx.cli()
        .arg("clone")
        .args(["--config", "core.autocrlf=false"])
        .arg(remote.url())
        .arg("configured")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "clone cache bypassed: custom transport or Git configuration",
        ));

    let config = fs::read_to_string(ctx.workspace().join("configured/.git/config")).unwrap();
    assert!(config.contains("autocrlf = false"));
}

#[test]
fn clone_preserves_git_help_and_invalid_option_behavior() {
    let ctx = TestContext::new();

    ctx.cli()
        .arg("clone")
        .arg("-h")
        .assert()
        .code(129)
        .stdout(predicate::str::contains("usage: git clone"))
        .stderr(predicate::str::is_empty());

    ctx.cli()
        .arg("clone")
        .arg("--not-a-real-clone-option")
        .assert()
        .code(129)
        .stderr(predicate::str::contains("unknown option"))
        .stderr(predicate::str::contains("clone cache").not());
}

#[test]
fn clone_preserves_the_operand_separator() {
    let ctx = TestContext::new();

    ctx.cli()
        .arg("clone")
        .args(["--", "--upload-pack=hostile"])
        .assert()
        .code(128)
        .stderr(predicate::str::contains("does not exist"))
        .stderr(predicate::str::contains("unknown option").not());
}

#[test]
fn clone_falls_back_to_git_when_the_cache_is_unavailable() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let blocked = ctx.root().join("blocked-cache");
    fs::write(&blocked, "not a directory\n").unwrap();

    ctx.cli()
        .env("XDG_CACHE_HOME", &blocked)
        .arg("clone")
        .arg(remote.url())
        .arg("cloned")
        .assert()
        .success()
        .stderr(predicate::str::contains("clone cache unavailable; cloned without cache"));

    assert!(ctx.workspace().join("cloned/.git").exists());
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
fn clone_quiet_suppresses_git_progress_and_cache_status() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");

    ctx.cli()
        .arg("clone")
        .args(["--quiet", "--no-quiet", "--quiet"])
        .arg(remote.url())
        .arg("quiet")
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::is_empty());
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
