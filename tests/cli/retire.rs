use std::fs;

use predicates::prelude::*;

use crate::harness::{RemoteRepository, TestContext};

/// A grove whose base declares `base` and whose override alone declares the
/// `Work` repositories, all placed and cached by an initial sync.
fn synced_grove(ctx: &TestContext, work: &[&RemoteRepository]) {
    let base = ctx.create_remote("base");
    ctx.write_config(&format!("version = 2\n\n[repos]\nbase = \"{}\"\n", base.url()));
    let mut overlay = String::from("[groups.Work]\n");
    for (index, remote) in work.iter().enumerate() {
        overlay.push_str(&format!("work{index} = \"{}\"\n", remote.url()));
    }
    ctx.write_config_at("grove.override.toml", &overlay);
    ctx.cli().arg("sync").assert().success();
}

#[test]
fn retire_dry_run_reports_the_plan_and_changes_nothing() {
    let ctx = TestContext::new();
    let work = ctx.create_remote("work");
    synced_grove(&ctx, &[&work]);

    ctx.cli()
        .args(["retire", "--dry-run"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Would trash 1 repository"))
        .stderr(predicate::str::contains("- Work/work0 would move to the trash"))
        .stderr(predicate::str::contains("clone cache entry would be removed"))
        .stderr(predicate::str::contains("- grove.override.toml would move to the trash"))
        .stderr(predicate::str::contains("base").not());

    assert!(ctx.workspace().join("Work/work0/.git").exists());
    assert!(ctx.workspace().join("grove.override.toml").exists());
    ctx.cli()
        .args(["cache", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("work.git"));
}

#[test]
fn retire_changes_nothing_while_a_repository_is_blocked() {
    let ctx = TestContext::new();
    let work = ctx.create_remote("work");
    let other = ctx.create_remote("other");
    synced_grove(&ctx, &[&work, &other]);
    fs::write(ctx.workspace().join("Work/work1/untracked.txt"), "local\n").unwrap();

    ctx.cli()
        .arg("retire")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Blocked 1 repository"))
        .stderr(predicate::str::contains("x Work/work1 local-only state: uncommitted changes"))
        .stderr(predicate::str::contains("- Work/work0 would move to the trash"))
        .stderr(predicate::str::contains(
            "x grove.override.toml kept until every repository can be retired",
        ));

    assert!(ctx.workspace().join("Work/work0/.git").exists());
    assert!(ctx.workspace().join("Work/work1/untracked.txt").exists());
    assert!(ctx.workspace().join("grove.override.toml").exists());
    ctx.cli()
        .args(["cache", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("work.git"))
        .stdout(predicate::str::contains("other.git"));
}

#[test]
fn retire_fails_before_changing_anything_when_its_preconditions_fail() {
    let ctx = TestContext::new();
    ctx.write_config("version = 2\n");
    ctx.cli()
        .arg("retire")
        .assert()
        .failure()
        .stderr(predicate::str::contains("grove.override.toml: no override to retire"));

    ctx.write_config_at("grove.override.toml", "version = 2\n");
    ctx.cli()
        .args(["retire", "grove.toml"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("grove.toml: not the override of"));

    ctx.write_config("[repos]\n");
    ctx.cli()
        .arg("retire")
        .assert()
        .failure()
        .stderr(predicate::str::contains("retiring it would leave an invalid configuration"));
    assert!(ctx.workspace().join("grove.override.toml").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn retire_moves_the_override_repositories_and_the_override_to_the_trash() {
    let ctx = TestContext::new();
    let work = ctx.create_remote("work");
    synced_grove(&ctx, &[&work]);
    let data_home = ctx.root().join("data-home");
    fs::create_dir_all(&data_home).unwrap();

    ctx.cli()
        .env("XDG_DATA_HOME", &data_home)
        .arg("retire")
        .assert()
        .success()
        .stderr(predicate::str::contains("Retired 1 repository"))
        .stderr(predicate::str::contains("- Work/work0 moved to the trash"))
        .stderr(predicate::str::contains("clone cache entry removed"))
        .stderr(predicate::str::contains("- grove.override.toml moved to the trash"));

    assert!(!ctx.workspace().join("Work/work0").exists());
    assert!(!ctx.workspace().join("grove.override.toml").exists());
    assert!(ctx.workspace().join("base/.git").exists());
    assert!(data_home.join("Trash/files/work0/.git").exists());
    assert!(data_home.join("Trash/files/grove.override.toml").exists());
    ctx.cli()
        .args(["cache", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("work.git").not())
        .stdout(predicate::str::contains("base.git"));
    ctx.cli().arg("validate").assert().success();
}
