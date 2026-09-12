use predicates::prelude::*;

use crate::harness::TestContext;

#[test]
fn sync_ignore_overrides_clones_from_the_base_destination() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = ctx.write_single_repository_config("blog", &remote.url(), None);
    ctx.write_config_at(
        "grove.override.toml",
        r#"
[repos.blog]
path = "overridden"
"#,
    );

    ctx.cli()
        .arg("--config")
        .arg(config)
        .args(["sync", "--ignore-overrides", "blog"])
        .assert()
        .success();

    assert!(ctx.workspace().join("blog").join(".git").exists());
    assert!(!ctx.workspace().join("overridden").exists());
}

#[test]
fn sync_ignore_overrides_uses_base_includes_and_ignores_their_overrides() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("child");
    let config = ctx.write_config(
        r#"
version = 2
include = ["work/grove.toml"]
"#,
    );
    ctx.write_config_at(
        "grove.override.toml",
        r#"
include = []
"#,
    );
    ctx.write_config_at(
        "work/grove.toml",
        &format!(
            r#"
version = 2

[repos.child]
url = "{}"
"#,
            remote.url()
        ),
    );
    ctx.write_config_at(
        "work/grove.override.toml",
        r#"
[repos.child]
path = "../overridden"
"#,
    );

    ctx.cli().arg("--config").arg(config).args(["sync", "-i", "child"]).assert().success();

    assert!(ctx.workspace().join("work/child").join(".git").exists());
    assert!(!ctx.workspace().join("overridden").exists());
}

#[test]
fn sync_ignore_overrides_does_not_parse_a_malformed_override() {
    let ctx = TestContext::new();
    let config = ctx.write_single_repository_config("blog", "git@example.com:blog.git", None);
    ctx.write_config_at("grove.override.toml", "[repos.blog\npath = \"overridden\"\n");

    ctx.cli()
        .arg("--config")
        .arg(&config)
        .args(["sync", "--ignore-overrides", "--dry-run"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Would clone 1 repository"));

    ctx.cli()
        .arg("--config")
        .arg(config)
        .args(["sync", "--dry-run"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("grove.override.toml"))
        .stderr(predicate::str::contains("invalid TOML"));
}

#[cfg(unix)]
#[test]
fn sync_ignore_overrides_does_not_resolve_a_broken_override_symlink() {
    let ctx = TestContext::new();
    let config = ctx.write_single_repository_config("blog", "git@example.com:blog.git", None);
    std::os::unix::fs::symlink(
        ctx.workspace().join("missing-override-target.toml"),
        ctx.workspace().join("grove.override.toml"),
    )
    .expect("failed to create broken override symlink");

    ctx.cli()
        .arg("--config")
        .arg(config)
        .args(["sync", "-iz", "--dry-run"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Would clone 1 repository"));
}
