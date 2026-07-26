use crate::harness::TestContext;

#[test]
fn status_fails_for_unknown_repository_target() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1

[repos.frontend]
path = "frontend"
url = "git@example.com:frontend.git"
"#,
    );

    let result = grove::status(Some(config), vec!["backend".to_string()], false);

    let error = result.expect_err("sync should reject an unknown repository");
    assert_eq!(error.kind(), grove::AppErrorKind::RepositoryNotFound);
    assert_eq!(error.repository_name(), Some("backend"));
}

#[test]
fn refresh_fails_for_unknown_repository_target() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1

[repos.frontend]
path = "frontend"
url = "git@example.com:frontend.git"
"#,
    );

    let result = grove::refresh(
        Some(config),
        vec!["backend".to_string()],
        grove::RefreshOptions::new(false),
    );

    let error = result.expect_err("refresh should reject an unknown repository");
    assert_eq!(error.kind(), grove::AppErrorKind::RepositoryNotFound);
    assert_eq!(error.repository_name(), Some("backend"));
}
