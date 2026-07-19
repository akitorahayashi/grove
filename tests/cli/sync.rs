use predicates::prelude::*;

use crate::harness::{TestContext, run_git};

#[test]
fn sync_dry_run_plans_missing_clone_without_creating_destination() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = ctx.write_config(&format!(
        r#"
version = 1

[[repo]]
name = "blog"
path = "blog"
url = "{}"
"#,
        remote.url()
    ));

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(predicate::str::contains("PLANNED"))
        .stdout(predicate::str::contains("clone"));

    assert!(!ctx.workspace().join("blog").exists());
}

#[test]
fn sync_clones_missing_repository() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = ctx.write_config(&format!(
        r#"
version = 1

[[repo]]
name = "blog"
path = "blog"
url = "{}"
"#,
        remote.url()
    ));

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .arg("blog")
        .assert()
        .success()
        .stdout(predicate::str::contains("CLONED"))
        .stdout(predicate::str::contains("blog"));

    assert!(ctx.workspace().join("blog").join(".git").exists());
}

#[test]
fn sync_short_alias_plans_missing_clone() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = ctx.write_config(&format!(
        r#"
version = 1

[[repo]]
name = "blog"
path = "blog"
url = "{}"
"#,
        remote.url()
    ));

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("s")
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(predicate::str::contains("PLANNED"))
        .stdout(predicate::str::contains("clone"));
}

#[test]
fn sync_updates_default_branch_and_restores_current_branch() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("frontend");
    let config = ctx.write_config(&format!(
        r#"
version = 1

[[repo]]
name = "frontend"
path = "frontend"
url = "{}"
"#,
        remote.url()
    ));

    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();
    run_git(&ctx.workspace().join("frontend"), &["switch", "-c", "feature/login"]);
    remote.add_commit("feature.txt", "remote change\n");

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .arg("frontend")
        .assert()
        .success()
        .stdout(predicate::str::contains("UPDATED"))
        .stdout(predicate::str::contains("main"));

    let output = std::process::Command::new("git")
        .current_dir(ctx.workspace().join("frontend"))
        .args(["branch", "--show-current"])
        .output()
        .expect("failed to inspect current branch");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "feature/login");
}
