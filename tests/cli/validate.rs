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

[repos.blog]
path = "blog"
url = "git@example.com:blog.git"
"#,
    );
    ctx.write_config_at(
        "work/grove.toml",
        r#"
version = 1

[repos.frontend]
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

[repos.blog]
path = "blog"
url = "git@example.com:blog.git"
"#,
    );

    ctx.cli()
        .arg("--config")
        .arg(&config)
        .arg("vl")
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
[repos.blog]
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
fn validate_redacts_credentials_from_malformed_toml_errors() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1

[repos.blog]
url = "https://user:credential@example.com/repo.git
"#,
    );

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("validate")
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("invalid TOML"))
        .stderr(predicate::str::contains("https://[redacted]@example.com/repo.git"))
        .stderr(predicate::str::contains("credential").not());
}

#[test]
fn validate_does_not_require_git() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1

[repos.blog]
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

#[cfg(unix)]
#[test]
fn validate_rejects_repository_path_escaping_root_through_symlink() {
    let ctx = TestContext::new();
    let outside = ctx.root().join("outside");
    std::fs::create_dir(&outside).expect("failed to create outside directory");
    std::os::unix::fs::symlink(&outside, ctx.workspace().join("escape"))
        .expect("failed to create escaping symlink");
    let config = ctx.write_config(
        r#"
version = 1

[repos.blog]
path = "escape/blog"
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
        .stderr(predicate::str::contains("repository 'blog' path leaves the grove root"));
}

#[cfg(unix)]
#[test]
fn validate_rejects_duplicate_repository_paths_through_symlink_aliases() {
    let ctx = TestContext::new();
    let target = ctx.workspace().join("actual");
    std::fs::create_dir(&target).expect("failed to create target directory");
    std::os::unix::fs::symlink(&target, ctx.workspace().join("alias"))
        .expect("failed to create alias symlink");
    let config = ctx.write_config(
        r#"
version = 1

[repos.first]
path = "actual/repo"
url = "git@example.com:first.git"

[repos.second]
path = "alias/repo"
url = "git@example.com:second.git"
"#,
    );

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("validate")
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("duplicate repository path"));
}

#[test]
fn validate_rejects_documented_catalog_invariant_violations() {
    let cases = [
        ("unsupported version", "version = 2\n", "unsupported config version 2"),
        (
            "duplicate paths",
            r#"
version = 1
[repos.first]
path = "same"
url = "git@example.com:first.git"
[repos.second]
path = "same"
url = "git@example.com:second.git"
"#,
            "duplicate repository path",
        ),
        (
            "nested paths",
            r#"
version = 1
[repos.first]
path = "parent"
url = "git@example.com:first.git"
[repos.second]
path = "parent/child"
url = "git@example.com:second.git"
"#,
            "repository paths must not be nested",
        ),
        (
            "absolute path",
            r#"
version = 1
[repos.repo]
path = "/tmp/outside"
url = "git@example.com:repo.git"
"#,
            "path must be relative",
        ),
        (
            "root escape",
            r#"
version = 1
[repos.repo]
path = "../outside"
url = "git@example.com:repo.git"
"#,
            "path leaves the grove root",
        ),
        (
            "unknown field",
            r#"
version = 1
unexpected = true
"#,
            "unknown field `unexpected`",
        ),
        (
            "invalid default branch",
            r#"
version = 1
[repos.repo]
path = "repo"
url = "git@example.com:repo.git"
default_branch = "-unsafe"
"#,
            "invalid Git branch name '-unsafe'",
        ),
    ];

    for (_label, contents, expected) in cases {
        let ctx = TestContext::new();
        let config = ctx.write_config(contents);

        ctx.cli()
            .arg("--config")
            .arg(config)
            .arg("validate")
            .assert()
            .failure()
            .stderr(predicate::str::contains(expected));
    }
}

#[test]
fn validate_accepts_repository_key_names_and_default_paths() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1

[repos.frontend]
url = "git@example.com:frontend.git"

[repos."company.service"]
url = "git@example.com:company.service.git"
"#,
    );

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("validate")
        .assert()
        .success()
        .stdout(predicate::str::contains("Validated 2 repositories"))
        .stderr(predicate::str::is_empty());
}

#[test]
fn validate_rejects_missing_repository_url() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1

[repos.blog]
"#,
    );

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("validate")
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("missing required field 'repos.blog.url'"));
}

#[test]
fn validate_rejects_invalid_repository_key_names() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1

[repos."bad name"]
url = "git@example.com:bad.git"
"#,
    );

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("validate")
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("invalid repository name: bad name"));
}

#[test]
fn validate_rejects_legacy_repository_arrays() {
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
        .arg(config)
        .arg("validate")
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("unsupported field 'repo'"))
        .stderr(predicate::str::contains("[repos.<name>]"));
}

#[test]
fn validate_rejects_duplicate_names_across_includes() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1
include = ["work/grove.toml"]

[repos.same]
url = "git@example.com:first.git"
"#,
    );
    ctx.write_config_at(
        "work/grove.toml",
        r#"
version = 1

[repos.same]
url = "git@example.com:second.git"
"#,
    );

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("validate")
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("duplicate repository name 'same'"));
}

#[test]
fn validate_rejects_duplicate_and_nested_includes() {
    let duplicate = TestContext::new();
    duplicate.write_config_at("child.toml", "version = 1\n");
    let config =
        duplicate.write_config("version = 1\ninclude = [\"child.toml\", \"child.toml\"]\n");
    duplicate
        .cli()
        .arg("--config")
        .arg(config)
        .arg("validate")
        .assert()
        .failure()
        .stderr(predicate::str::contains("duplicate configuration file"));

    let nested = TestContext::new();
    nested.write_config_at("grandchild.toml", "version = 1\n");
    nested.write_config_at("child.toml", "version = 1\ninclude = [\"grandchild.toml\"]\n");
    let config = nested.write_config("version = 1\ninclude = [\"child.toml\"]\n");
    nested
        .cli()
        .arg("--config")
        .arg(config)
        .arg("validate")
        .assert()
        .failure()
        .stderr(predicate::str::contains("nested includes are not allowed"));
}

#[cfg(unix)]
#[test]
fn validate_accepts_symlink_target_inside_root_and_rejects_nested_alias() {
    let accepted = TestContext::new();
    let target = accepted.workspace().join("actual");
    std::fs::create_dir(&target).unwrap();
    std::os::unix::fs::symlink(&target, accepted.workspace().join("alias")).unwrap();
    let config = accepted.write_config(
        r#"
version = 1
[repos.repo]
path = "alias/repo"
url = "git@example.com:repo.git"
"#,
    );
    accepted
        .cli()
        .arg("--config")
        .arg(config)
        .arg("validate")
        .assert()
        .success()
        .stdout(predicate::str::contains("Validated 1 repository"));

    let nested = TestContext::new();
    let target = nested.workspace().join("actual");
    std::fs::create_dir(&target).unwrap();
    std::os::unix::fs::symlink(&target, nested.workspace().join("alias")).unwrap();
    let config = nested.write_config(
        r#"
version = 1
[repos.parent]
path = "actual/repo"
url = "git@example.com:parent.git"
[repos.child]
path = "alias/repo/child"
url = "git@example.com:child.git"
"#,
    );
    nested
        .cli()
        .arg("--config")
        .arg(config)
        .arg("validate")
        .assert()
        .failure()
        .stderr(predicate::str::contains("repository paths must not be nested"));
}

#[test]
fn validate_accepts_valid_default_branch_with_slash() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1
[repos.repo]
path = "repo"
url = "git@example.com:repo.git"
default_branch = "release/stable"
"#,
    );

    ctx.cli().arg("--config").arg(config).arg("validate").assert().success();
}
