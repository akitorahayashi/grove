use predicates::prelude::*;

use crate::harness::TestContext;

#[test]
fn cache_list_shows_headers_when_nothing_cached() {
    let ctx = TestContext::new();

    ctx.cli()
        .arg("cache")
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("URL").and(predicate::str::contains("UPDATED")));
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
fn cache_list_emits_color_when_forced() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");

    ctx.cli().arg("clone").arg(remote.url()).arg("cloned").assert().success();

    ctx.cli()
        .env_remove("NO_COLOR")
        .env("CLICOLOR_FORCE", "1")
        .arg("cache")
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("\u{1b}[1m"));
}

#[test]
fn cache_clean_removes_all_entries() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");

    ctx.cli().arg("clone").arg(remote.url()).arg("cloned").assert().success();

    ctx.cli()
        .arg("cache")
        .arg("clean")
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed 1 cache entry"));

    ctx.cli()
        .arg("cache")
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("blog.git").not());
}

#[test]
fn cache_clean_by_name_removes_matching_entry() {
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
        .arg("clean")
        .arg("blog")
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed 1 cache entry"));
}

#[test]
fn cache_clean_unknown_name_fails() {
    let ctx = TestContext::new();
    let config = ctx.write_config("version = 1\n");

    ctx.cli()
        .arg("--config")
        .arg(&config)
        .arg("cache")
        .arg("clean")
        .arg("missing")
        .assert()
        .failure()
        .stderr(predicate::str::contains("missing"));
}
