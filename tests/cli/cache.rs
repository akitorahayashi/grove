use predicates::prelude::*;

use crate::harness::TestContext;

#[test]
fn cache_list_shows_headers_when_nothing_cached() {
    let ctx = TestContext::new();

    let output = ctx.cli().arg("cache").arg("list").output().expect("cache list should run");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("cache list output should be UTF-8");
    assert_eq!(
        stdout
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>(),
        ["URL", "UPDATED"]
    );
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
fn cache_list_orders_entries_by_url() {
    let ctx = TestContext::new();
    let zeta = ctx.create_remote("zeta");
    let alpha = ctx.create_remote("alpha");
    ctx.cli().arg("clone").arg(zeta.url()).arg("zeta").assert().success();
    ctx.cli().arg("clone").arg(alpha.url()).arg("alpha").assert().success();

    let output = ctx.cli().arg("cache").arg("list").output().expect("cache list should run");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("cache list output should be UTF-8");
    assert!(stdout.find("alpha.git").unwrap() < stdout.find("zeta.git").unwrap());
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
fn cache_list_redacts_generic_schemes_and_encoded_secret_keys() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    ctx.cli().arg("clone").arg(remote.url()).arg("cloned").assert().success();
    let entry = std::fs::read_dir(ctx.cache_root())
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.join("url").is_file())
        .unwrap();
    std::fs::write(
        entry.join("url"),
        "GIT+SSH://user:credential@example.com/repo.git?access%5Ftoken=secret-value",
    )
    .unwrap();

    ctx.cli()
        .arg("cache")
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "GIT+SSH://[redacted]@example.com/repo.git?access%5Ftoken=[redacted]",
        ))
        .stdout(predicate::str::contains("credential").not())
        .stdout(predicate::str::contains("secret-value").not());
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
