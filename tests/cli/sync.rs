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
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Would clone 1 repository"))
        .stderr(predicate::str::contains("+ blog"));

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
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Checked 1 repository"))
        .stderr(predicate::str::contains("Cloned 1 repository"))
        .stderr(predicate::str::contains("+ blog"));

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
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Would clone 1 repository"))
        .stderr(predicate::str::contains("+ blog"));
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
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Updated 1 repository"))
        .stderr(predicate::str::contains("~ frontend main"));

    let output = std::process::Command::new("git")
        .current_dir(ctx.workspace().join("frontend"))
        .args(["branch", "--show-current"])
        .output()
        .expect("failed to inspect current branch");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "feature/login");
}

#[test]
fn sync_omits_current_repository_rows_when_nothing_changed() {
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

    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .arg("blog")
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Checked 1 repository"))
        .stderr(predicate::str::contains("+ blog").not())
        .stderr(predicate::str::contains("~ blog").not());
}

#[test]
fn sync_reports_skipped_repositories_and_exits_with_failure() {
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

    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();
    std::fs::write(ctx.workspace().join("blog").join("draft.txt"), "local\n")
        .expect("failed to dirty work tree");

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .arg("blog")
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Skipped 1 repository"))
        .stderr(predicate::str::contains("! blog dirty working tree"));
}

#[test]
fn sync_reports_blocked_repositories_and_exits_with_failure() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let replacement = ctx.create_remote("replacement");
    let initial_config = ctx.write_config(&format!(
        r#"
version = 1

[[repo]]
name = "blog"
path = "blog"
url = "{}"
"#,
        remote.url()
    ));

    ctx.cli().arg("--config").arg(&initial_config).arg("sync").assert().success();

    let mismatched_config = ctx.write_config(&format!(
        r#"
version = 1

[[repo]]
name = "blog"
path = "blog"
url = "{}"
"#,
        replacement.url()
    ));

    ctx.cli()
        .arg("--config")
        .arg(mismatched_config)
        .arg("sync")
        .arg("blog")
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Blocked 1 repository"))
        .stderr(predicate::str::contains("x blog remote URL does not match grove.toml"));
}
