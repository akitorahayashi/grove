use crate::harness::TestContext;
use predicates::prelude::*;
use serial_test::serial;

#[test]
#[serial]
fn delete_command_removes_item() {
    let ctx = TestContext::new();

    ctx.cli().args(["item", "add", "temp", "--content", "value"]).assert().success();

    assert!(ctx.saved_item_path("temp").exists(), "Item should exist before delete");

    ctx.cli()
        .arg("item")
        .arg("delete")
        .arg("temp")
        .assert()
        .success()
        .stdout(predicate::str::contains("Deleted item 'temp'"));

    assert!(!ctx.saved_item_path("temp").exists(), "Item should not exist after delete");
}
