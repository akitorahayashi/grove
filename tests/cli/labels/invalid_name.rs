use crate::harness::TestContext;
use predicates::prelude::*;
use serial_test::serial;

#[test]
#[serial]
fn label_with_invalid_name_fails() {
    let ctx = TestContext::new();

    ctx.cli()
        .args(["label", "add", "invalid/name"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid label name"));
}
