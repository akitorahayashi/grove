use crate::harness::TestContext;
use predicates::prelude::*;
use serial_test::serial;

#[test]
#[serial]
fn add_command_persists_item() {
    let ctx = TestContext::new();

    ctx.cli()
        .arg("item")
        .arg("add")
        .arg("demo")
        .arg("--content")
        .arg("example value")
        .assert()
        .success()
        .stdout(predicate::str::contains("Added item 'demo'"));

    ctx.assert_saved_item_contains("demo", "example value");
}
