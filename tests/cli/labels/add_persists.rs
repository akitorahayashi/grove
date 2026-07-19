use crate::harness::TestContext;
use predicates::prelude::*;
use serial_test::serial;

#[test]
#[serial]
fn label_command_persists_label() {
    let ctx = TestContext::new();

    ctx.cli()
        .args(["label", "add", "urgent"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added label 'urgent'"));

    ctx.assert_saved_label_exists("urgent");
}
