use crate::harness::TestContext;
use predicates::prelude::*;
use serial_test::serial;

#[test]
#[serial]
fn add_list_delete_roundtrip_succeeds() {
    let ctx = TestContext::new();

    ctx.cli()
        .args(["item", "add", "workflow", "--content", "example"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Added item 'workflow'"));

    ctx.cli()
        .args(["item", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("- workflow"));

    ctx.cli()
        .args(["item", "delete", "workflow"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Deleted item 'workflow'"));

    ctx.cli().args(["item", "list"]).assert().success().stdout(predicate::str::contains("(none)"));
}
