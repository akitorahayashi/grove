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

    assert!(
        matches!(result, Err(grove::AppError::RepositoryNotFound(ref name)) if name == "backend")
    );
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

    assert!(
        matches!(result, Err(grove::AppError::RepositoryNotFound(ref name)) if name == "backend")
    );
}
