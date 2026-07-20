use predicates::prelude::*;

use crate::harness::TestContext;

#[test]
fn validate_reports_config_summary() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1

include = [
  "work/grove.toml",
]

[[repo]]
name = "blog"
path = "blog"
url = "git@example.com:blog.git"
"#,
    );
    ctx.write_config_at(
        "work/grove.toml",
        r#"
version = 1

[[repo]]
name = "frontend"
path = "frontend"
url = "git@example.com:frontend.git"
"#,
    );
    let config = config.canonicalize().expect("failed to resolve config path");

    ctx.cli()
        .arg("--config")
        .arg(&config)
        .arg("validate")
        .assert()
        .success()
        .stdout(predicate::str::contains("Validated 2 repositories"))
        .stdout(predicate::str::contains(format!("Config: {}", config.display())))
        .stderr(predicate::str::is_empty());
}

#[test]
fn validate_short_alias_reports_config_summary() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1

[[repo]]
name = "blog"
path = "blog"
url = "git@example.com:blog.git"
"#,
    );

    ctx.cli()
        .arg("--config")
        .arg(&config)
        .arg("v")
        .assert()
        .success()
        .stdout(predicate::str::contains("Validated 1 repository"))
        .stderr(predicate::str::is_empty());
}

#[test]
fn validate_fails_for_invalid_config() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
[[repo]]
name = "blog"
path = "blog"
url = "git@example.com:blog.git"
"#,
    );

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("validate")
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("missing required field 'version'"));
}

#[test]
fn validate_does_not_require_git() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1

[[repo]]
name = "blog"
path = "blog"
url = "git@example.com:blog.git"
"#,
    );

    ctx.cli()
        .env("PATH", "")
        .arg("--config")
        .arg(config)
        .arg("validate")
        .assert()
        .success()
        .stdout(predicate::str::contains("Validated 1 repository"));
}
