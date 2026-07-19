use crate::harness::TestContext;
use rs_cli_tmpl::{label_add, label_delete};
use serial_test::serial;

#[test]
#[serial]
fn label_delete_removes_label() {
    let ctx = TestContext::new();

    ctx.with_dir(ctx.work_dir(), || {
        label_add("temporary").expect("label_add should succeed");
    });

    assert!(ctx.saved_label_path("temporary").exists(), "Label should exist before delete");

    ctx.with_dir(ctx.work_dir(), || {
        label_delete("temporary").expect("label_delete should succeed");
    });

    assert!(!ctx.saved_label_path("temporary").exists(), "Label should not exist after delete");
}
