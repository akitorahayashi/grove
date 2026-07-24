use predicates::prelude::*;

use crate::harness::TestContext;

#[test]
fn sync_dry_run_plans_missing_clone_without_creating_destination() {
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
fn sync_dry_run_never_requires_or_creates_a_cache_root() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"https://example.com/blog.git\"\n",
    );

    ctx.cli()
        .env_remove("XDG_CACHE_HOME")
        .env_remove("HOME")
        .arg("--config")
        .arg(config)
        .arg("sync")
        .arg("--dry-run")
        .assert()
        .success()
        .stderr(predicate::str::contains("Would clone 1 repository"))
        .stderr(predicate::str::contains("cache directory").not());

    assert!(!ctx.workspace().join("blog").exists());
}
