use crate::harness::TestContext;
use predicates::prelude::*;
use serial_test::serial;

#[test]
#[serial]
fn labeling_attach_list_find_detach_roundtrip_succeeds() {
    let ctx = TestContext::new();

    ctx.cli().args(["item", "add", "demo", "--content", "example"]).assert().success();
    ctx.cli().args(["label", "add", "urgent"]).assert().success();

    ctx.cli()
        .args(["labeling", "attach", "demo", "urgent"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Attached label 'urgent' to item 'demo'"));

    ctx.assert_label_link_exists("demo", "urgent");

    ctx.cli().args(["labeling", "list", "demo"]).assert().success().stdout(
        predicate::str::contains("Labels for item 'demo'")
            .and(predicate::str::contains("- urgent")),
    );

    ctx.cli().args(["labeling", "find", "--label", "urgent"]).assert().success().stdout(
        predicate::str::contains("Items with label 'urgent'")
            .and(predicate::str::contains("- demo")),
    );

    ctx.cli()
        .args(["labeling", "detach", "demo", "urgent"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Detached label 'urgent' from item 'demo'"));

    ctx.assert_label_link_missing("demo", "urgent");

    ctx.cli()
        .args(["labeling", "list", "demo"])
        .assert()
        .success()
        .stdout(predicate::str::contains("(none)"));
}
