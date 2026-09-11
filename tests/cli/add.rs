use std::fs;
use std::path::PathBuf;

use predicates::prelude::*;

use crate::harness::{TestContext, run_git};

#[test]
fn add_without_a_path_registers_the_current_worktree() {
    let ctx = TestContext::new();
    ctx.write_config("version = 1\n");
    let remote = ctx.create_remote("backend");
    let repository = initialize_repository(&ctx, "backend", &remote.url());
    let nested = repository.join("nested");
    fs::create_dir(&nested).unwrap();

    ctx.cli()
        .current_dir(&nested)
        .arg("add")
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(
            predicate::str::contains(format!(
                "Config: {}",
                ctx.config_path().canonicalize().unwrap().display()
            ))
            .and(predicate::str::contains("+ backend backend")),
        );

    let contents = fs::read_to_string(ctx.config_path()).unwrap();
    assert!(contents.contains("[repos.backend]\nurl ="));
    assert!(!contents.contains("path ="));
    ctx.cli().arg("validate").assert().success();
}

#[test]
fn add_preserves_operand_order_and_stops_after_the_first_failure() {
    let ctx = TestContext::new();
    ctx.write_config("version = 1\n");
    let remote = ctx.create_remote("repositories");
    initialize_repository(&ctx, "first", &remote.url());
    initialize_repository(&ctx, "missing-origin", "");
    initialize_repository(&ctx, "last", &remote.url());

    let assertion = ctx
        .cli()
        .args(["add", "first", "missing-origin", "last"])
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains(
            "Stopped: 1 written, 0 unchanged, 1 failed, 1 not attempted",
        ));
    let stderr = String::from_utf8_lossy(&assertion.get_output().stderr);
    let written = stderr.find("+ first first").expect("written entry should be logged");
    let failed = stderr.find("x missing-origin").expect("failed entry should be logged");
    assert!(written < failed, "stderr was not emitted in operand order:\n{stderr}");

    let contents = fs::read_to_string(ctx.config_path()).unwrap();
    assert!(contents.contains("[repos.first]"));
    assert!(!contents.contains("[repos.missing-origin]"));
    assert!(!contents.contains("[repos.last]"));
}

#[test]
fn add_keeps_successful_entries_in_operand_order() {
    let ctx = TestContext::new();
    ctx.write_config("version = 1\n");
    let remote = ctx.create_remote("ordered");
    initialize_repository(&ctx, "second", &remote.url());
    initialize_repository(&ctx, "first", &remote.url());

    ctx.cli().args(["add", "second", "first"]).assert().success();

    let contents = fs::read_to_string(ctx.config_path()).unwrap();
    assert!(contents.find("[repos.second]").unwrap() < contents.find("[repos.first]").unwrap());
}

#[test]
fn add_writes_custom_paths_and_is_idempotent() {
    let ctx = TestContext::new();
    ctx.write_config("version = 1\n");
    let remote = ctx.create_remote("backend");
    initialize_repository(&ctx, "services/backend", &remote.url());

    ctx.cli().args(["add", "services/backend", "services/backend"]).assert().success().stderr(
        predicate::str::contains("+ backend services/backend")
            .and(predicate::str::contains("= backend already configured at services/backend")),
    );

    let contents = fs::read_to_string(ctx.config_path()).unwrap();
    assert_eq!(contents.matches("[repos.backend]").count(), 1);
    assert!(contents.contains("path = \"services/backend\""));
}

#[test]
fn add_displays_dot_when_the_repository_is_the_grove_root() {
    let ctx = TestContext::new();
    ctx.write_config("version = 1\n");
    let remote = ctx.create_remote("root");
    run_git(ctx.workspace(), &["init", "-b", "main"]);
    run_git(ctx.workspace(), &["remote", "add", "origin", &remote.url()]);

    ctx.cli().arg("add").assert().success();
    ctx.cli()
        .arg("add")
        .assert()
        .success()
        .stderr(predicate::str::contains("= workspace already configured at ."));

    assert!(fs::read_to_string(ctx.config_path()).unwrap().contains("path = \".\""));
}

#[test]
fn add_accepts_username_only_ssh_origins() {
    let ctx = TestContext::new();
    ctx.write_config("version = 1\n");
    initialize_repository(&ctx, "ssh-repository", "ssh://git@example.com/company/repo.git");

    ctx.cli().args(["add", "ssh-repository"]).assert().success();

    assert!(
        fs::read_to_string(ctx.config_path())
            .unwrap()
            .contains("url = \"ssh://git@example.com/company/repo.git\"")
    );
}

#[test]
fn add_override_creates_a_versionless_sibling() {
    let ctx = TestContext::new();
    let base = ctx.write_config_at("custom.toml", "version = 1\n");
    let remote = ctx.create_remote("local");
    initialize_repository(&ctx, "local", &remote.url());
    let override_path = base.canonicalize().unwrap().with_file_name("custom.override.toml");

    ctx.cli()
        .args(["--config", base.to_str().unwrap(), "add", "--override", "local"])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!("Config: {}", override_path.display())));

    assert_eq!(fs::read_to_string(base).unwrap(), "version = 1\n");
    let contents = fs::read_to_string(override_path).unwrap();
    assert!(!contents.contains("version"));
    assert!(contents.contains("[repos.local]"));
}

#[test]
fn add_override_dry_run_keeps_both_files_absent_of_changes() {
    let ctx = TestContext::new();
    ctx.write_config("version = 1\n");
    let remote = ctx.create_remote("planned");
    initialize_repository(&ctx, "planned", &remote.url());
    let override_path = ctx.workspace().join("grove.override.toml");

    ctx.cli()
        .args(["add", "--override", "--dry-run", "planned"])
        .assert()
        .success()
        .stderr(predicate::str::contains("+ planned would add planned"));

    assert_eq!(fs::read_to_string(ctx.config_path()).unwrap(), "version = 1\n");
    assert!(!override_path.exists());
}

#[test]
fn add_dry_run_reports_prior_plans_when_a_later_target_fails() {
    let ctx = TestContext::new();
    ctx.write_config("version = 1\n");
    let remote = ctx.create_remote("dry-run");
    initialize_repository(&ctx, "planned", &remote.url());
    initialize_repository(&ctx, "missing-origin", "");

    ctx.cli().args(["add", "--dry-run", "planned", "missing-origin"]).assert().failure().stderr(
        predicate::str::contains("Stopped: 1 planned, 0 unchanged, 1 failed, 0 not attempted"),
    );

    assert_eq!(fs::read_to_string(ctx.config_path()).unwrap(), "version = 1\n");
}

#[test]
fn add_rejects_unsafe_origin_without_disclosing_it() {
    let ctx = TestContext::new();
    ctx.write_config("version = 1\n");
    let repository = initialize_repository(
        &ctx,
        "unsafe-origin",
        "https://credential:secret@example.com/repo.git?access_token=hidden",
    );

    ctx.cli().arg("add").arg(&repository).assert().failure().stderr(
        predicate::str::contains("contains credentials")
            .and(predicate::str::contains("secret").not())
            .and(predicate::str::contains("hidden").not()),
    );

    assert_eq!(fs::read_to_string(ctx.config_path()).unwrap(), "version = 1\n");
}

#[test]
fn add_rejects_invalid_names_and_repositories_outside_the_grove_root() {
    let ctx = TestContext::new();
    ctx.write_config("version = 1\n");
    let remote = ctx.create_remote("rejected");
    initialize_repository(&ctx, "invalid name", &remote.url());
    let outside = ctx.root().join("outside");
    fs::create_dir(&outside).unwrap();
    run_git(&outside, &["init", "-b", "main"]);
    run_git(&outside, &["remote", "add", "origin", &remote.url()]);

    ctx.cli()
        .args(["add", "invalid name"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("cannot derive a repository name"));
    ctx.cli()
        .arg("add")
        .arg(outside)
        .assert()
        .failure()
        .stderr(predicate::str::contains("leaves the grove root"));

    assert_eq!(fs::read_to_string(ctx.config_path()).unwrap(), "version = 1\n");
}

#[test]
fn add_short_alias_registers_a_repository() {
    let ctx = TestContext::new();
    ctx.write_config("version = 1\n");
    let remote = ctx.create_remote("aliased");
    initialize_repository(&ctx, "aliased", &remote.url());

    ctx.cli().args(["a", "aliased"]).assert().success();

    assert!(fs::read_to_string(ctx.config_path()).unwrap().contains("[repos.aliased]"));
}

fn initialize_repository(ctx: &TestContext, relative: &str, origin: &str) -> PathBuf {
    let repository = ctx.workspace().join(relative);
    fs::create_dir_all(&repository).unwrap();
    run_git(&repository, &["init", "-b", "main"]);
    if !origin.is_empty() {
        run_git(&repository, &["remote", "add", "origin", origin]);
    }
    repository
}
