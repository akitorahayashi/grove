use crate::harness::TestContext;
use rs_cli_tmpl::label_add;
use serial_test::serial;

#[test]
#[serial]
fn label_add_persists_label() {
    let ctx = TestContext::new();

    ctx.with_dir(ctx.work_dir(), || {
        label_add("urgent").expect("label_add should succeed");
    });

    ctx.assert_saved_label_exists("urgent");
}
