use crate::harness::TestContext;
use predicates::prelude::*;
use serial_test::serial;

#[test]
#[serial]
fn label_add_list_delete_roundtrip_succeeds() {
    let ctx = TestContext::new();

    ctx.cli()
        .args(["label", "add", "workflow"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added label 'workflow'"));

    ctx.cli()
        .args(["label", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("- workflow"));

    ctx.cli()
        .args(["label", "delete", "workflow"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Deleted label 'workflow'"));

    ctx.cli().args(["label", "list"]).assert().success().stdout(predicate::str::contains("(none)"));
}
