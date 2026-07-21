use predicates::prelude::*;

use crate::harness::TestContext;

#[test]
fn cache_list_reports_empty_when_nothing_cached() {
    let ctx = TestContext::new();

    ctx.cli()
        .arg("cache")
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("No cached repositories"));
}

#[test]
fn cache_list_shows_entry_after_clone() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");

    ctx.cli().arg("clone").arg(remote.url()).arg("cloned").assert().success();

    ctx.cli()
        .arg("cache")
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("blog.git"));
}

#[test]
fn cache_clear_removes_all_entries() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");

    ctx.cli().arg("clone").arg(remote.url()).arg("cloned").assert().success();

    ctx.cli()
        .arg("cache")
        .arg("clear")
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed 1 cache entry"));

    ctx.cli()
        .arg("cache")
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("No cached repositories"));
}

#[test]
fn cache_clear_by_name_removes_matching_entry() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = ctx.write_config(&format!(
        r#"
version = 1

[repos.blog]
path = "blog"
url = "{}"
"#,
        remote.url()
    ));

    ctx.cli().arg("--config").arg(&config).arg("sync").arg("blog").assert().success();

    ctx.cli()
        .arg("--config")
        .arg(&config)
        .arg("cache")
        .arg("clear")
        .arg("blog")
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed 1 cache entry"));
}

#[test]
fn cache_clear_unknown_name_fails() {
    let ctx = TestContext::new();
    let config = ctx.write_config("version = 1\n");

    ctx.cli()
        .arg("--config")
        .arg(&config)
        .arg("cache")
        .arg("clear")
        .arg("missing")
        .assert()
        .failure()
        .stderr(predicate::str::contains("missing"));
}
