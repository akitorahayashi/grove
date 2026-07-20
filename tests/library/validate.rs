use crate::harness::TestContext;

#[test]
fn validate_returns_config_summary() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1

[[repo]]
name = "frontend"
path = "frontend"
url = "git@example.com:frontend.git"
"#,
    );

    let report = grove::validate(Some(config)).expect("valid config should pass");

    assert_eq!(report.repository_count(), 1);
    assert_eq!(report.config_path(), ctx.config_path().canonicalize().unwrap());
}
