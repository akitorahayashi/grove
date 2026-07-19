use predicates::prelude::*;

use crate::harness::TestContext;

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
