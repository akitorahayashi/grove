use crate::harness::TestContext;
use rs_cli_tmpl::{item_add, item_delete, label_add, labeling_attach, labeling_find};
use serial_test::serial;

#[test]
#[serial]
fn item_delete_detaches_label_links() {
    let ctx = TestContext::new();

    ctx.with_dir(ctx.work_dir(), || {
        item_add("demo", "example").expect("item_add should succeed");
        label_add("urgent").expect("label_add should succeed");
        labeling_attach("demo", "urgent").expect("labeling_attach should succeed");

        item_delete("demo").expect("item_delete should succeed");

        let items = labeling_find("urgent").expect("labeling_find should succeed");
        assert!(items.is_empty());
    });

    ctx.assert_label_link_missing("demo", "urgent");
}
