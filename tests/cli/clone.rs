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
    let git_help_code = std::process::Command::new("git")
        .args(["clone", "-h"])
        .output()
        .unwrap()
        .status
        .code()
        .unwrap();

    ctx.cli()
        .arg("clone")
        .arg("-h")
        .assert()
        .code(git_help_code)
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
    let config = ctx.write_config("version = 2\n");

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

#[test]
fn clone_resolves_a_cached_repository_by_shorthand() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    ctx.cli().arg("clone").arg(remote.url()).arg("first").assert().success();

    ctx.cli()
        .arg("clone")
        .args(["blog", "copy"])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!("gv: clone resolved blog to {}", remote.url())))
        .stderr(predicate::str::contains("(from cache)"));

    let config = fs::read_to_string(ctx.workspace().join("copy/.git/config")).unwrap();
    assert!(config.contains(&format!("url = {}", remote.url())));
}

#[test]
fn clone_rejects_an_ambiguous_shorthand_and_accepts_a_longer_one() {
    let ctx = TestContext::new();
    let first = ctx.create_remote("a/blog");
    let second = ctx.create_remote("b/blog");
    ctx.cli().arg("clone").arg(first.url()).arg("a-first").assert().success();
    ctx.cli().arg("clone").arg(second.url()).arg("b-first").assert().success();

    ctx.cli()
        .arg("clone")
        .arg("blog")
        .assert()
        .failure()
        .stderr(predicate::str::contains("'blog' matches several cache entries"))
        .stderr(predicate::str::contains(first.url()))
        .stderr(predicate::str::contains(second.url()))
        .stderr(predicate::str::contains("Cloning into").not());
    assert!(!ctx.workspace().join("blog").exists());

    ctx.cli()
        .arg("clone")
        .arg("a/blog")
        .assert()
        .success()
        .stderr(predicate::str::contains(format!("gv: clone resolved a/blog to {}", first.url())));
    assert!(ctx.workspace().join("blog/.git").exists());
}

#[test]
fn clone_fails_for_a_shorthand_without_a_cache_entry() {
    let ctx = TestContext::new();

    ctx.cli()
        .arg("clone")
        .arg("owner/nothing")
        .assert()
        .failure()
        .stderr(predicate::str::contains("no cache entry matches 'owner/nothing'"))
        .stderr(predicate::str::contains("Cloning into").not());
    assert!(!ctx.workspace().join("nothing").exists());
}

#[test]
fn clone_keeps_an_existing_local_path_as_a_local_clone() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    ctx.cli().arg("clone").arg(remote.url()).assert().success();

    ctx.cli()
        .arg("clone")
        .args(["blog", "copy"])
        .assert()
        .success()
        .stderr(predicate::str::contains("clone resolved").not());

    let config = fs::read_to_string(ctx.workspace().join("copy/.git/config")).unwrap();
    assert!(!config.contains(&format!("url = {}", remote.url())));
}

#[test]
fn clone_resolves_a_shorthand_even_when_the_cache_is_bypassed() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    ctx.cli().arg("clone").arg(remote.url()).arg("first").assert().success();

    ctx.cli()
        .arg("clone")
        .args(["--depth", "1", "blog", "shallow"])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!("gv: clone resolved blog to {}", remote.url())))
        .stderr(predicate::str::contains("clone cache bypassed"));

    assert!(ctx.workspace().join("shallow/.git").exists());
}
