use crate::harness::TestContext;

#[test]
fn list_returns_resolved_repositories() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1

[[repo]]
name = "frontend"
path = "apps/frontend"
url = "git@example.com:frontend.git"
"#,
    );

    let report = grove::list(Some(config)).expect("list succeeds");

    assert_eq!(report.repositories.len(), 1);
    assert_eq!(report.repositories[0].name, "frontend");
    assert!(report.repositories[0].path.ends_with("apps/frontend"));
    assert_eq!(report.repositories[0].url, "git@example.com:frontend.git");
}
