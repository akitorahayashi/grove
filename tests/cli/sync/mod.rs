use predicates::prelude::*;

use crate::harness::{TestContext, commit_file, path_with_wrapper, run_git};

mod cache;
mod planning;
mod progress;
mod zoxide;

#[test]
fn sync_clones_missing_repository() {
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

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .arg("blog")
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Checked 1 repository"))
        .stderr(predicate::str::contains("Prepared 1 repository"))
        .stderr(predicate::str::contains("+ blog"))
        .stderr(predicate::str::contains("\u{1b}[").not())
        .stderr(predicate::str::contains("⠙").not());

    assert!(ctx.workspace().join("blog").join(".git").exists());
}

#[test]
fn sync_uses_change_colors_when_color_is_forced() {
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

    ctx.cli()
        .env_remove("NO_COLOR")
        .env("CLICOLOR_FORCE", "1")
        .arg("--config")
        .arg(config)
        .arg("sync")
        .arg("--dry-run")
        .assert()
        .success()
        .stderr(predicate::str::contains("\u{1b}[32m+\u{1b}["));
}

#[test]
fn sync_updates_default_branch_and_restores_current_branch() {
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
    run_git(&ctx.workspace().join("frontend"), &["switch", "-c", "feature/login"]);
    remote.add_commit("feature.txt", "remote change\n");

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .arg("frontend")
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Updated 1 repository"))
        .stderr(predicate::str::contains("~ frontend main"));

    let output = std::process::Command::new("git")
        .current_dir(ctx.workspace().join("frontend"))
        .args(["branch", "--show-current"])
        .output()
        .expect("failed to inspect current branch");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "feature/login");
}

#[cfg(unix)]
#[test]
fn sync_reports_completed_update_when_original_branch_restoration_fails() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = ctx.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\n",
        remote.url()
    ));
    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();
    let repository = ctx.workspace().join("blog");
    run_git(&repository, &["switch", "-c", "feature"]);
    remote.add_commit("remote.txt", "remote\n");
    let path = path_with_wrapper(
        &ctx,
        "sync-restore",
        "if [ \"$1\" = switch ] && [ \"${3:-}\" = feature ]; then echo restoration-failed >&2; exit 42; fi",
    );

    ctx.cli()
        .env("PATH", path)
        .arg("--config")
        .arg(config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Updated 1 repository"))
        .stderr(predicate::str::contains("main"))
        .stderr(predicate::str::contains("restoring the original branch failed"))
        .stderr(predicate::str::contains("restoration-failed"));

    let branch = std::process::Command::new("git")
        .current_dir(&repository)
        .args(["branch", "--show-current"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&branch.stdout).trim(), "main");
}

#[cfg(unix)]
#[test]
fn sync_reports_merge_failure_and_successful_restoration() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = ctx.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\n",
        remote.url()
    ));
    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();
    let repository = ctx.workspace().join("blog");
    run_git(&repository, &["switch", "-c", "feature"]);
    remote.add_commit("remote.txt", "remote\n");
    let path = path_with_wrapper(
        &ctx,
        "sync-merge",
        "if [ \"$1\" = merge ]; then echo merge-failed >&2; exit 42; fi",
    );

    ctx.cli()
        .env("PATH", path)
        .arg("--config")
        .arg(config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("merge-failed"))
        .stderr(predicate::str::contains("restored the original branch"));

    let branch = std::process::Command::new("git")
        .current_dir(&repository)
        .args(["branch", "--show-current"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&branch.stdout).trim(), "feature");
}

#[test]
fn sync_omits_current_repository_rows_when_nothing_changed() {
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

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .arg("blog")
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Checked 1 repository"))
        .stderr(predicate::str::contains("+ blog").not())
        .stderr(predicate::str::contains("~ blog").not());
}

#[test]
fn sync_reports_skipped_repositories_and_exits_with_failure() {
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
    std::fs::write(ctx.workspace().join("blog").join("draft.txt"), "local\n")
        .expect("failed to dirty work tree");

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .arg("blog")
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Skipped 1 repository"))
        .stderr(predicate::str::contains("! blog dirty working tree"));
}

#[test]
fn sync_reports_blocked_repositories_and_exits_with_failure() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let replacement = ctx.create_remote("replacement");
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

    let mismatched_config = ctx.write_config(&format!(
        r#"
version = 1

[repos.blog]
path = "blog"
url = "{}"
"#,
        replacement.url()
    ));

    ctx.cli()
        .arg("--config")
        .arg(mismatched_config)
        .arg("sync")
        .arg("blog")
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Blocked 1 repository"))
        .stderr(predicate::str::contains("x blog remote URL does not match grove.toml"))
        .stderr(predicate::str::contains(format!("actual:   {}", remote.url())))
        .stderr(predicate::str::contains(format!("expected: {}", replacement.url())));
}

#[test]
fn sync_redacts_credentials_in_remote_url_mismatch_details() {
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
        .arg("sync")
        .arg("blog")
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("x blog remote URL does not match grove.toml"))
        .stderr(predicate::str::contains(
            "actual:   https://[redacted]@example.com/org/repo.git?access_token=[redacted]&branch=main",
        ))
        .stderr(predicate::str::contains(
            "expected: https://[redacted]@example.com/org/repo.git?password=[redacted]&branch=main",
        ))
        .stderr(predicate::str::contains("ghp_actual").not())
        .stderr(predicate::str::contains("actual_token").not())
        .stderr(predicate::str::contains("ghp_expected").not())
        .stderr(predicate::str::contains("expected_secret").not());
}

#[test]
fn sync_dry_run_redacts_credentials_and_secret_query_values() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1

[repos.blog]
path = "blog"
url = "https://user:credential@example.com/repo.git?access_token=secret-value&branch=main"
"#,
    );

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .arg("--dry-run")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "https://[redacted]@example.com/repo.git?access_token=[redacted]&branch=main",
        ))
        .stderr(predicate::str::contains("credential").not())
        .stderr(predicate::str::contains("secret-value").not());
}

#[cfg(unix)]
#[test]
fn sync_redacts_url_echoed_by_clone_failure() {
    use std::os::unix::fs::PermissionsExt;

    let ctx = TestContext::new();
    let bin = ctx.root().join("fake-git-bin");
    std::fs::create_dir(&bin).unwrap();
    let git = bin.join("git");
    std::fs::write(
        &git,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo "git version 2.40.0"
  exit 0
fi
if [ "$1" = "clone" ]; then
  echo "fatal: clone failed for $4" >&2
  exit 1
fi
exit 1
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&git).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&git, permissions).unwrap();
    let config = ctx.write_config(
        r#"
version = 1

[repos.blog]
path = "blog"
url = "https://user:credential@example.com/repo.git?password=secret-value"
"#,
    );

    ctx.cli()
        .env("PATH", bin)
        .arg("--config")
        .arg(config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "https://[redacted]@example.com/repo.git?password=[redacted]",
        ))
        .stderr(predicate::str::contains("credential").not())
        .stderr(predicate::str::contains("secret-value").not());
}

#[test]
fn sync_redacts_credentials_in_successful_clone_output() {
    use std::os::unix::fs::PermissionsExt;

    let ctx = TestContext::new();
    let bin = ctx.root().join("successful-fake-git-bin");
    std::fs::create_dir(&bin).unwrap();
    let git = bin.join("git");
    std::fs::write(
        &git,
        r#"#!/bin/sh
PATH="/usr/bin:/bin:$PATH"
if [ "$1" = --version ]; then echo 'git version 2.40.0'; fi
if [ "$1" = clone ]; then for arg in "$@"; do dest="$arg"; done; mkdir -p "$dest"; fi
if [ "$1" = symbolic-ref ]; then echo main; fi
exit 0
"#,
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&git).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&git, permissions).unwrap();
    let config = ctx.write_config(
        r#"
version = 1
[repos.blog]
path = "blog"
url = "https://user:credential@example.com/repo.git?api_key=secret-value"
"#,
    );

    ctx.cli()
        .env("PATH", bin)
        .arg("--config")
        .arg(config)
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "https://[redacted]@example.com/repo.git?api_key=[redacted]",
        ))
        .stderr(predicate::str::contains("credential").not())
        .stderr(predicate::str::contains("secret-value").not());
}

#[cfg(unix)]
#[test]
fn sync_redacts_credentials_echoed_by_fetch_failure() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = ctx.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\n",
        remote.url()
    ));
    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();
    let path = path_with_wrapper(
        &ctx,
        "sync-fetch",
        "if [ \"$1\" = fetch ]; then echo 'fatal: GIT+SSH://user:credential@example.com/repo.git?%54OKEN=secret-value' >&2; exit 42; fi",
    );

    ctx.cli()
        .env("PATH", path)
        .arg("--config")
        .arg(config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "GIT+SSH://[redacted]@example.com/repo.git?%54OKEN=[redacted]",
        ))
        .stderr(predicate::str::contains("credential").not())
        .stderr(predicate::str::contains("secret-value").not());
}

#[test]
fn sync_escapes_control_characters_in_repository_paths() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1

[repos.blog]
path = "folder\n\u001b[31m"
url = "git@example.com:blog.git"
"#,
    );

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .arg("--dry-run")
        .assert()
        .success()
        .stderr(predicate::str::contains("folder\\n\\u{1b}[31m"))
        .stderr(predicate::str::contains("\u{1b}[31m").not());
}

#[cfg(unix)]
#[test]
fn sync_rejects_missing_destination_below_symlink_escaping_root() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let outside = ctx.root().join("outside");
    std::fs::create_dir(&outside).expect("failed to create outside directory");
    std::os::unix::fs::symlink(&outside, ctx.workspace().join("escape"))
        .expect("failed to create escaping symlink");
    let config = ctx.write_config(&format!(
        r#"
version = 1

[repos.blog]
path = "escape/blog"
url = "{}"
"#,
        remote.url()
    ));

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .arg("blog")
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("repository 'blog' path leaves the grove root"));

    assert!(!outside.join("blog").exists());
}

#[cfg(unix)]
#[test]
fn sync_accepts_existing_repository_through_in_root_symlink() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    std::fs::create_dir(ctx.workspace().join("actual")).unwrap();
    let initial = ctx.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"actual/blog\"\nurl = \"{}\"\n",
        remote.url()
    ));
    ctx.cli().arg("--config").arg(&initial).arg("sync").assert().success();
    std::os::unix::fs::symlink(ctx.workspace().join("actual"), ctx.workspace().join("alias"))
        .unwrap();
    let aliased = ctx.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"alias/blog\"\nurl = \"{}\"\n",
        remote.url()
    ));

    ctx.cli().arg("--config").arg(aliased).arg("sync").assert().success();
}

#[cfg(unix)]
#[test]
fn sync_rejects_existing_repository_symlink_outside_root_without_mutation() {
    let ctx = TestContext::new();
    let outside = ctx.root().join("outside-repository");
    run_git(ctx.root(), &["init", "-b", "main", outside.to_str().unwrap()]);
    std::fs::write(outside.join("marker"), "unchanged\n").unwrap();
    std::os::unix::fs::symlink(&outside, ctx.workspace().join("blog")).unwrap();
    let config = ctx.write_config(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"git@example.com:blog.git\"\n",
    );

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("path leaves the grove root"));

    assert_eq!(std::fs::read_to_string(outside.join("marker")).unwrap(), "unchanged\n");
}

#[test]
fn sync_blocks_non_repository_missing_origin_and_detached_head() {
    let non_repository = TestContext::new();
    std::fs::create_dir(non_repository.workspace().join("blog")).unwrap();
    let config = non_repository.write_config(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"git@example.com:blog.git\"\n",
    );
    non_repository
        .cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("destination exists but is not a Git repository"));

    let missing_origin = TestContext::new();
    let remote = missing_origin.create_remote("blog");
    let config = missing_origin.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\n",
        remote.url()
    ));
    missing_origin.cli().arg("--config").arg(&config).arg("sync").assert().success();
    run_git(&missing_origin.workspace().join("blog"), &["remote", "remove", "origin"]);
    missing_origin
        .cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("remote origin is missing"));

    let detached = TestContext::new();
    let remote = detached.create_remote("blog");
    let config = detached.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\n",
        remote.url()
    ));
    detached.cli().arg("--config").arg(&config).arg("sync").assert().success();
    run_git(&detached.workspace().join("blog"), &["checkout", "--detach"]);
    detached
        .cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("detached HEAD"));
}

#[test]
fn sync_blocks_a_bare_repository_at_the_destination() {
    let ctx = TestContext::new();
    let destination = ctx.workspace().join("blog");
    run_git(ctx.workspace(), &["init", "--bare", destination.to_str().unwrap()]);
    let config = ctx.write_config(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"git@example.com:blog.git\"\n",
    );

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("destination exists but is not a Git repository"));
}

#[test]
fn sync_blocks_missing_default_and_configured_branches() {
    let missing_default = TestContext::new();
    let remote = missing_default.create_remote("blog");
    let config = missing_default.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\n",
        remote.url()
    ));
    missing_default.cli().arg("--config").arg(&config).arg("sync").assert().success();
    run_git(
        &missing_default.workspace().join("blog"),
        &["symbolic-ref", "--delete", "refs/remotes/origin/HEAD"],
    );
    missing_default
        .cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("remote default branch cannot be determined"));

    let missing_local = TestContext::new();
    let remote = missing_local.create_remote("blog");
    let initial = missing_local.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\n",
        remote.url()
    ));
    missing_local.cli().arg("--config").arg(&initial).arg("sync").assert().success();
    let config = missing_local.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\ndefault_branch = \"ghost\"\n",
        remote.url()
    ));
    missing_local
        .cli()
        .arg("--config")
        .arg(&config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("local default branch 'ghost' is missing"));

    let missing_remote = TestContext::new();
    let remote = missing_remote.create_remote("blog");
    let initial = missing_remote.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\n",
        remote.url()
    ));
    missing_remote.cli().arg("--config").arg(&initial).arg("sync").assert().success();
    run_git(&missing_remote.workspace().join("blog"), &["branch", "ghost"]);
    let configured = missing_remote.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\ndefault_branch = \"ghost\"\n",
        remote.url()
    ));
    missing_remote
        .cli()
        .arg("--config")
        .arg(configured)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("remote default branch 'origin/ghost' is missing"));
}

#[test]
fn sync_blocks_ahead_and_diverged_default_branches() {
    let ahead = TestContext::new();
    let remote = ahead.create_remote("blog");
    let config = ahead.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\n",
        remote.url()
    ));
    ahead.cli().arg("--config").arg(&config).arg("sync").assert().success();
    commit_file(&ahead.workspace().join("blog"), "ahead.txt");
    ahead
        .cli()
        .arg("--config")
        .arg(&config)
        .arg("sync")
        .arg("--dry-run")
        .assert()
        .failure()
        .stderr(predicate::str::contains("main is ahead of origin/main"));
    ahead
        .cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("main is ahead of origin/main"));

    let diverged = TestContext::new();
    let remote = diverged.create_remote("blog");
    let config = diverged.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\n",
        remote.url()
    ));
    diverged.cli().arg("--config").arg(&config).arg("sync").assert().success();
    commit_file(&diverged.workspace().join("blog"), "local.txt");
    remote.add_commit("remote.txt", "remote\n");
    run_git(&diverged.workspace().join("blog"), &["fetch", "origin"]);
    diverged
        .cli()
        .arg("--config")
        .arg(&config)
        .arg("sync")
        .arg("--dry-run")
        .assert()
        .failure()
        .stderr(predicate::str::contains("main has diverged from origin/main"));
    diverged
        .cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("main has diverged from origin/main"));
}

#[test]
fn sync_reports_fetch_and_clone_failures() {
    let clone_failure = TestContext::new();
    let config = clone_failure
        .write_config("version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"/does/not/exist\"\n");
    clone_failure
        .cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("clone").and(predicate::str::contains("does/not/exist")));

    let fetch_failure = TestContext::new();
    let remote = fetch_failure.create_remote("blog");
    let initial = fetch_failure.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\n",
        remote.url()
    ));
    fetch_failure.cli().arg("--config").arg(&initial).arg("sync").assert().success();
    run_git(
        &fetch_failure.workspace().join("blog"),
        &["remote", "set-url", "origin", "/does/not/exist"],
    );
    let config = fetch_failure
        .write_config("version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"/does/not/exist\"\n");
    fetch_failure
        .cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("fetch"));
}

#[test]
fn configured_default_branch_overrides_stale_origin_head() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = ctx.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\n",
        remote.url()
    ));
    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();
    run_git(
        &ctx.workspace().join("blog"),
        &["symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/stale"],
    );
    remote.add_commit("remote.txt", "remote\n");
    let configured = ctx.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\ndefault_branch = \"main\"\n",
        remote.url()
    ));

    ctx.cli()
        .arg("--config")
        .arg(configured)
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("~ blog main"));
}

#[test]
fn sync_discovers_config_from_a_subdirectory_and_names_it() {
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
    let config = config.canonicalize().expect("failed to resolve config path");
    let nested = ctx.workspace().join("nested").join("deeper");
    std::fs::create_dir_all(&nested).expect("failed to create nested directory");

    ctx.cli()
        .current_dir(&nested)
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains(format!("Config: {}", config.display())))
        .stderr(predicate::str::contains("+ blog"));

    assert!(ctx.workspace().join("blog").join(".git").exists());
}
