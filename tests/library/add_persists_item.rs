use crate::harness::TestContext;
use rs_cli_tmpl::item_add;
use serial_test::serial;

#[test]
#[serial]
fn add_persists_item() {
    let ctx = TestContext::new();

    ctx.with_dir(ctx.work_dir(), || {
        item_add("sample", "hello world").expect("library add should succeed");
    });

    ctx.assert_saved_item_contains("sample", "hello world");
}
