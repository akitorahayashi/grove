use crate::harness::TestContext;
use rs_cli_tmpl::{item_add, item_delete};
use serial_test::serial;

#[test]
#[serial]
fn delete_removes_item() {
    let ctx = TestContext::new();

    ctx.with_dir(ctx.work_dir(), || {
        item_add("temp", "value").expect("add should succeed");
    });

    assert!(ctx.saved_item_path("temp").exists(), "Item should exist before delete");

    ctx.with_dir(ctx.work_dir(), || {
        item_delete("temp").expect("delete should succeed");
    });

    assert!(!ctx.saved_item_path("temp").exists(), "Item should be removed after delete");
}
