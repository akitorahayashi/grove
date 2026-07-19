use crate::harness::TestContext;
use predicates::prelude::*;
use serial_test::serial;

#[test]
#[serial]
fn labeling_attach_requires_existing_label() {
    let ctx = TestContext::new();

    ctx.cli().args(["item", "add", "demo", "--content", "example"]).assert().success();

    ctx.cli()
        .args(["labeling", "attach", "demo", "missing"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Label 'missing' was not found"));
}
