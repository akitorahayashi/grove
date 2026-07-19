use crate::harness::TestContext;
use rs_cli_tmpl::{item_add, item_list};
use serial_test::serial;

#[test]
#[serial]
fn list_returns_items() {
    let ctx = TestContext::new();

    ctx.with_dir(ctx.work_dir(), || {
        item_add("first", "one").expect("add should succeed");
        item_add("second", "two").expect("add should succeed");
        let mut items = item_list().expect("list should succeed");
        items.sort();
        assert_eq!(items, vec!["first".to_string(), "second".to_string()]);
    });
}
