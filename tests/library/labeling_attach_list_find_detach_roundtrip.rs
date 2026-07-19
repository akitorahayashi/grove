use crate::harness::TestContext;
use rs_cli_tmpl::{
    item_add, label_add, labeling_attach, labeling_detach, labeling_find, labeling_list,
};
use serial_test::serial;

#[test]
#[serial]
fn labeling_attach_list_find_detach_roundtrip() {
    let ctx = TestContext::new();

    ctx.with_dir(ctx.work_dir(), || {
        item_add("demo", "example").expect("item_add should succeed");
        label_add("urgent").expect("label_add should succeed");

        labeling_attach("demo", "urgent").expect("labeling_attach should succeed");

        let labels = labeling_list("demo").expect("labeling_list should succeed");
        assert_eq!(labels, vec!["urgent".to_string()]);

        let items = labeling_find("urgent").expect("labeling_find should succeed");
        assert_eq!(items, vec!["demo".to_string()]);

        labeling_detach("demo", "urgent").expect("labeling_detach should succeed");

        let labels_after_detach =
            labeling_list("demo").expect("labeling_list should succeed after detach");
        assert!(labels_after_detach.is_empty());
    });

    ctx.assert_label_link_missing("demo", "urgent");
}
