use predicates::prelude::*;

use crate::harness::TestContext;

#[test]
fn help_lists_mvp_commands() {
    let ctx = TestContext::new();

    ctx.cli()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("init"))
        .stdout(predicate::str::contains("sync"))
        .stdout(predicate::str::contains("status"))
        .stdout(predicate::str::contains("list"));
}

#[test]
fn version_uses_grove_package() {
    let ctx = TestContext::new();

    ctx.cli().arg("--version").assert().success().stdout(predicate::str::contains("gv 0.1.0"));
}
