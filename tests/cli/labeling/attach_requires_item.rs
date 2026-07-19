use crate::harness::TestContext;
use predicates::prelude::*;
use serial_test::serial;

#[test]
#[serial]
fn labeling_attach_requires_existing_item() {
    let ctx = TestContext::new();

    ctx.cli().args(["label", "add", "urgent"]).assert().success();

    ctx.cli()
        .args(["labeling", "attach", "missing", "urgent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Item 'missing' was not found"));
}
