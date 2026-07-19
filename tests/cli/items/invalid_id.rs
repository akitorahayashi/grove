use crate::harness::TestContext;
use predicates::prelude::*;
use serial_test::serial;

#[test]
#[serial]
fn add_with_invalid_id_fails() {
    let ctx = TestContext::new();

    ctx.cli()
        .args(["item", "add", "invalid/id", "--content", "value"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid item identifier"));
}
