use std::fs;

use predicates::prelude::*;

use crate::harness::TestContext;

#[test]
fn init_creates_grove_toml_in_current_directory() {
    let ctx = TestContext::new();

    ctx.cli().arg("init").assert().success().stdout(predicate::str::contains("created"));

    let contents = fs::read_to_string(ctx.config_path()).expect("failed to read grove.toml");
    assert!(contents.contains("version = 1"));
    assert!(contents.contains("REPLACE_WITH_CHILD_DIRECTORY/grove.toml"));
    assert!(contents.contains("git@github.com:REPLACE_WITH_OWNER/REPLACE_WITH_REPOSITORY.git"));
    assert!(contents.contains("https://github.com/REPLACE_WITH_OWNER/REPLACE_WITH_REPOSITORY.git"));
}

#[test]
fn init_short_alias_creates_grove_toml() {
    let ctx = TestContext::new();

    ctx.cli().arg("i").assert().success();

    let contents = fs::read_to_string(ctx.config_path()).expect("failed to read grove.toml");
    assert!(contents.contains("version = 1"));
}

#[test]
fn init_does_not_overwrite_existing_grove_toml() {
    let ctx = TestContext::new();
    ctx.write_config("version = 1\n");

    ctx.cli().arg("init").assert().failure().stderr(predicate::str::contains("File exists"));

    let contents = fs::read_to_string(ctx.config_path()).expect("failed to read grove.toml");
    assert_eq!(contents, "version = 1\n");
}

#[test]
fn init_rejects_config_option() {
    let ctx = TestContext::new();

    ctx.cli()
        .args(["--config", "custom.toml", "init"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--config cannot be used with init"));
}
