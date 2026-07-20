use predicates::prelude::*;

use crate::harness::{TestContext, run_git};

#[test]
fn status_reports_missing_repositories_without_fetching() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1

[repos.blog]
path = "personal/blog"
url = "git@example.com:blog.git"
"#,
    );

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("NAME"))
        .stdout(predicate::str::contains("blog"))
        .stdout(predicate::str::contains("personal/blog"))
        .stdout(predicate::str::contains("missing"));
}

#[test]
fn status_fetch_updates_remote_tracking_information() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("frontend");
    let config = ctx.write_config(&format!(
        r#"
version = 1

[repos.frontend]
path = "frontend"
url = "{}"
"#,
        remote.url()
    ));

    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();
    remote.add_commit("change.txt", "change\n");

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("status")
        .arg("--fetch")
        .assert()
        .success()
        .stdout(predicate::str::contains("frontend"))
        .stdout(predicate::str::contains("behind 1"));
}

#[test]
fn status_target_outputs_detail_sections() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1

[repos.blog]
path = "personal/blog"
url = "git@example.com:blog.git"
"#,
    );

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("status")
        .arg("blog")
        .assert()
        .success()
        .stdout(predicate::str::contains("blog"))
        .stdout(predicate::str::contains("Repository"))
        .stdout(predicate::str::contains("Path:"))
        .stdout(predicate::str::contains("personal/blog"))
        .stdout(predicate::str::contains("Config:"))
        .stdout(predicate::str::contains("Status"))
        .stdout(predicate::str::contains("State:"))
        .stdout(predicate::str::contains("missing"))
        .stdout(predicate::str::contains("NAME").not());
}

#[test]
fn status_uses_repository_name_as_default_path() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1

[repos.blog]
url = "git@example.com:blog.git"
"#,
    );

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("status")
        .arg("blog")
        .assert()
        .success()
        .stdout(predicate::str::is_match(r"(?m)^  Path:\s+blog$").unwrap());
}

#[test]
fn status_target_reports_remote_mismatch_diagnostics() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let initial_config = ctx.write_config(&format!(
        r#"
version = 1

[repos.blog]
path = "blog"
url = "{}"
"#,
        remote.url()
    ));

    ctx.cli().arg("--config").arg(&initial_config).arg("sync").assert().success();
    run_git(
        &ctx.workspace().join("blog"),
        &[
            "remote",
            "set-url",
            "origin",
            "https://user:ghp_actual@example.com/org/repo.git?access_token=actual_token&branch=main",
        ],
    );
    let mismatched_config = ctx.write_config(
        r#"
version = 1

[repos.blog]
path = "blog"
url = "https://user:ghp_expected@example.com/org/repo.git?password=expected_secret&branch=main"
"#,
    );

    ctx.cli()
        .arg("--config")
        .arg(mismatched_config)
        .arg("status")
        .arg("blog")
        .assert()
        .success()
        .stdout(predicate::str::contains("Diagnostics"))
        .stdout(predicate::str::contains("Remote URL does not match grove.toml"))
        .stdout(predicate::str::contains(
            "Actual:   https://[redacted]@example.com/org/repo.git?access_token=[redacted]&branch=main",
        ))
        .stdout(predicate::str::contains(
            "Expected: https://[redacted]@example.com/org/repo.git?password=[redacted]&branch=main",
        ))
        .stdout(predicate::str::contains("ghp_actual").not())
        .stdout(predicate::str::contains("actual_token").not())
        .stdout(predicate::str::contains("ghp_expected").not())
        .stdout(predicate::str::contains("expected_secret").not());
}

#[test]
fn status_target_reports_missing_local_default_branch() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = ctx.write_config(&format!(
        r#"
version = 1

[repos.blog]
path = "blog"
url = "{}"
"#,
        remote.url()
    ));

    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();
    let repository = ctx.workspace().join("blog");
    run_git(&repository, &["switch", "-c", "feature"]);
    run_git(&repository, &["branch", "-D", "main"]);

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("status")
        .arg("blog")
        .assert()
        .success()
        .stdout(predicate::str::contains("Default:"))
        .stdout(predicate::str::contains("main"))
        .stdout(predicate::str::contains("Tracking:"))
        .stdout(predicate::str::contains("local branch missing"))
        .stdout(predicate::str::contains("up to date").not());
}

#[test]
fn status_target_reports_missing_remote_default_branch() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = ctx.write_config(&format!(
        r#"
version = 1

[repos.blog]
path = "blog"
url = "{}"
"#,
        remote.url()
    ));

    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();
    run_git(&ctx.workspace().join("blog"), &["update-ref", "-d", "refs/remotes/origin/main"]);

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("status")
        .arg("blog")
        .assert()
        .success()
        .stdout(predicate::str::contains("Default:"))
        .stdout(predicate::str::contains("main"))
        .stdout(predicate::str::contains("Tracking:"))
        .stdout(predicate::str::contains("remote branch missing"))
        .stdout(predicate::str::contains("up to date").not());
}

#[test]
fn status_table_preserves_fetch_failure_message() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = ctx.write_config(&format!(
        r#"
version = 1

[repos.blog]
path = "blog"
url = "{}"
"#,
        remote.url()
    ));

    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();
    run_git(&ctx.workspace().join("blog"), &["remote", "set-url", "origin", "/does/not/exist"]);

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("status")
        .arg("--fetch")
        .assert()
        .success()
        .stdout(predicate::str::contains("fetch-failed:"))
        .stdout(predicate::str::contains("fetch-failed  ").not());
}

#[test]
fn status_short_alias_reports_repository_status() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1

[repos.blog]
path = "personal/blog"
url = "git@example.com:blog.git"
"#,
    );

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("st")
        .assert()
        .success()
        .stdout(predicate::str::contains("personal/blog"))
        .stdout(predicate::str::contains("missing"));
}

#[cfg(unix)]
#[test]
fn status_rejects_git_older_than_required_version_before_inspection() {
    use std::os::unix::fs::PermissionsExt;

    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1

[repos.blog]
path = "blog"
url = "git@example.com:blog.git"
"#,
    );
    let bin = ctx.root().join("old-git-bin");
    std::fs::create_dir(&bin).unwrap();
    let git = bin.join("git");
    std::fs::write(&git, "#!/bin/sh\necho 'git version 2.22.0'\n").unwrap();
    let mut permissions = std::fs::metadata(&git).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&git, permissions).unwrap();

    ctx.cli()
        .env("PATH", bin)
        .arg("--config")
        .arg(config)
        .arg("status")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Git 2.23.0 or newer is required"));
}

#[test]
fn status_reports_missing_git_before_inspection() {
    let ctx = TestContext::new();
    let config = ctx.write_config("version = 1\n");

    ctx.cli()
        .env("PATH", "")
        .arg("--config")
        .arg(config)
        .arg("status")
        .assert()
        .failure()
        .stderr(predicate::str::contains("git is not available"));
}
