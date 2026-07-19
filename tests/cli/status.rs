use predicates::prelude::*;

use crate::harness::{TestContext, run_git};

#[test]
fn status_reports_missing_repositories_without_fetching() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1

[[repo]]
name = "blog"
path = "personal/blog"
url = "git@example.com:blog.git"
"#,
    );

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("NAME"))
        .stdout(predicate::str::contains("blog"))
        .stdout(predicate::str::contains("personal/blog"))
        .stdout(predicate::str::contains("missing"));
}

#[test]
fn status_fetch_updates_remote_tracking_information() {
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
    remote.add_commit("change.txt", "change\n");

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("status")
        .arg("--fetch")
        .assert()
        .success()
        .stdout(predicate::str::contains("frontend"))
        .stdout(predicate::str::contains("behind 1"));
}

#[test]
fn status_target_outputs_detail_sections() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1

[[repo]]
name = "blog"
path = "personal/blog"
url = "git@example.com:blog.git"
"#,
    );

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("status")
        .arg("blog")
        .assert()
        .success()
        .stdout(predicate::str::contains("blog"))
        .stdout(predicate::str::contains("Repository"))
        .stdout(predicate::str::contains("Path:"))
        .stdout(predicate::str::contains("personal/blog"))
        .stdout(predicate::str::contains("Config:"))
        .stdout(predicate::str::contains("Status"))
        .stdout(predicate::str::contains("State:"))
        .stdout(predicate::str::contains("missing"))
        .stdout(predicate::str::contains("NAME").not());
}

#[test]
fn status_target_reports_remote_mismatch_diagnostics() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
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
    run_git(
        &ctx.workspace().join("blog"),
        &[
            "remote",
            "set-url",
            "origin",
            "https://user:ghp_actual@example.com/org/repo.git?access_token=actual_token&branch=main",
        ],
    );
    let mismatched_config = ctx.write_config(
        r#"
version = 1

[[repo]]
name = "blog"
path = "blog"
url = "https://user:ghp_expected@example.com/org/repo.git?password=expected_secret&branch=main"
"#,
    );

    ctx.cli()
        .arg("--config")
        .arg(mismatched_config)
        .arg("status")
        .arg("blog")
        .assert()
        .success()
        .stdout(predicate::str::contains("Diagnostics"))
        .stdout(predicate::str::contains("Remote URL does not match grove.toml"))
        .stdout(predicate::str::contains(
            "Actual:   https://[redacted]@example.com/org/repo.git?access_token=[redacted]&branch=main",
        ))
        .stdout(predicate::str::contains(
            "Expected: https://[redacted]@example.com/org/repo.git?password=[redacted]&branch=main",
        ))
        .stdout(predicate::str::contains("ghp_actual").not())
        .stdout(predicate::str::contains("actual_token").not())
        .stdout(predicate::str::contains("ghp_expected").not())
        .stdout(predicate::str::contains("expected_secret").not());
}

#[test]
fn status_short_alias_reports_repository_status() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1

[[repo]]
name = "blog"
path = "personal/blog"
url = "git@example.com:blog.git"
"#,
    );

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("st")
        .assert()
        .success()
        .stdout(predicate::str::contains("personal/blog"))
        .stdout(predicate::str::contains("missing"));
}
