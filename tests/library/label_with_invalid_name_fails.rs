use crate::harness::TestContext;
use rs_cli_tmpl::label_add;
use serial_test::serial;
use std::io;

#[test]
#[serial]
fn label_with_invalid_name_fails() {
    let _ctx = TestContext::new();

    let err = label_add("invalid/name").expect_err("label_add should fail for invalid name");
    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
}
