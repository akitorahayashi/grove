use crate::harness::TestContext;
use predicates::prelude::*;
use serial_test::serial;

#[test]
#[serial]
fn delete_nonexistent_item_fails() {
    let ctx = TestContext::new();

    ctx.cli()
        .arg("item")
        .arg("delete")
        .arg("nonexistent")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Item 'nonexistent' was not found"));
}
