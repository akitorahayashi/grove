use crate::harness::TestContext;
use predicates::prelude::*;
use serial_test::serial;

#[test]
#[serial]
fn version_flag_works() {
    let ctx = TestContext::new();

    ctx.cli()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
#[serial]
fn help_lists_visible_aliases() {
    let ctx = TestContext::new();

    // Keep in sync with aliases defined in the CLI builder.
    ctx.cli().arg("--help").assert().success().stdout(
        predicate::str::contains("[aliases: i]")
            .and(predicate::str::contains("[aliases: l]"))
            .and(predicate::str::contains("[aliases: ln]")),
    );
}
