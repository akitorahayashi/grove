use crate::harness::test_context::TestContext;
use rs_cli_tmpl::item_add;
use serial_test::serial;
use std::io;

#[test]
#[serial]
fn add_with_invalid_id_fails() {
    let _ctx = TestContext::new();
    let err = item_add("invalid/id", "content").expect_err("add should fail for invalid id");
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}
