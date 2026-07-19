use predicates::prelude::*;

use crate::harness::{TestContext, run_git};

#[test]
fn sync_dry_run_plans_missing_clone_without_creating_destination() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = ctx.write_config(&format!(
        r#"
version = 1

[[repo]]
name = "blog"
path = "blog"
url = "{}"
"#,
        remote.url()
    ));

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Would clone 1 repository"))
        .stderr(predicate::str::contains("+ blog"));

    assert!(!ctx.workspace().join("blog").exists());
}

#[test]
fn sync_clones_missing_repository() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = ctx.write_config(&format!(
        r#"
version = 1

[[repo]]
name = "blog"
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
fn sync_register_zoxide_adds_cloned_repository() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let zoxide = FakeZoxide::new(&ctx);
    let config = ctx.write_config(&format!(
        r#"
version = 1

[[repo]]
name = "blog"
path = "blog"
url = "{}"
"#,
        remote.url()
    ));

    zoxide
        .command(ctx.cli())
        .arg("--config")
        .arg(config)
        .arg("sync")
        .arg("-z")
        .arg("blog")
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Zoxide"))
        .stderr(predicate::str::contains("+ blog added"));

    let database = std::fs::read_to_string(zoxide.database()).expect("failed to read zoxide db");
    assert!(
        database
            .lines()
            .any(|line| line == resolved_repository_path(&ctx, "blog").display().to_string())
    );
}

#[test]
fn sync_register_zoxide_reports_existing_entry_without_adding() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let zoxide = FakeZoxide::new(&ctx);
    let config = ctx.write_config(&format!(
        r#"
version = 1

[[repo]]
name = "blog"
path = "blog"
url = "{}"
"#,
        remote.url()
    ));
    std::fs::create_dir_all(&zoxide.data).expect("failed to create fake zoxide data");
    std::fs::write(
        zoxide.database(),
        format!("{}\n", resolved_repository_path(&ctx, "blog").display()),
    )
    .expect("failed to seed zoxide db");

    zoxide
        .command(ctx.cli())
        .arg("--config")
        .arg(config)
        .arg("sync")
        .arg("--register-zoxide")
        .arg("blog")
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Zoxide"))
        .stderr(predicate::str::contains("= blog already registered"))
        .stderr(predicate::str::contains("+ blog added").not());

    let database = std::fs::read_to_string(zoxide.database()).expect("failed to read zoxide db");
    assert_eq!(database.lines().count(), 1);
}

#[test]
fn sync_register_zoxide_adds_existing_repository_when_missing_from_zoxide() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let zoxide = FakeZoxide::new(&ctx);
    let config = ctx.write_config(&format!(
        r#"
version = 1

[[repo]]
name = "blog"
path = "blog"
url = "{}"
"#,
        remote.url()
    ));
    ctx.cli().arg("--config").arg(&config).arg("sync").arg("blog").assert().success();

    zoxide
        .command(ctx.cli())
        .arg("--config")
        .arg(config)
        .arg("sync")
        .arg("-z")
        .arg("blog")
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Zoxide"))
        .stderr(predicate::str::contains("+ blog added"));

    let database = std::fs::read_to_string(zoxide.database()).expect("failed to read zoxide db");
    assert!(
        database
            .lines()
            .any(|line| line == resolved_repository_path(&ctx, "blog").display().to_string())
    );
}

#[test]
fn sync_register_zoxide_reports_when_add_does_not_register_repository() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let zoxide = FakeZoxide::new(&ctx);
    let config = ctx.write_config(&format!(
        r#"
version = 1

[[repo]]
name = "blog"
path = "blog"
url = "{}"
"#,
        remote.url()
    ));

    zoxide
        .command(ctx.cli())
        .env("_ZO_EXCLUDE_DIRS", resolved_repository_path(&ctx, "blog"))
        .arg("--config")
        .arg(config)
        .arg("sync")
        .arg("-z")
        .arg("blog")
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Zoxide"))
        .stderr(predicate::str::contains("x blog zoxide did not register the repository"));

    assert!(!zoxide.database().exists());
}

#[test]
fn sync_register_zoxide_reports_unavailable_zoxide() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let zoxide = FakeZoxide::new(&ctx).unavailable();
    let config = ctx.write_config(&format!(
        r#"
version = 1

[[repo]]
name = "blog"
path = "blog"
url = "{}"
"#,
        remote.url()
    ));

    zoxide
        .command(ctx.cli())
        .arg("--config")
        .arg(config)
        .arg("sync")
        .arg("-z")
        .arg("blog")
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Zoxide"))
        .stderr(predicate::str::contains("x zoxide unavailable"));
}

#[test]
fn sync_dry_run_register_zoxide_reports_planned_registration_without_running_zoxide() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let zoxide = FakeZoxide::new(&ctx).unavailable();
    let config = ctx.write_config(&format!(
        r#"
version = 1

[[repo]]
name = "blog"
path = "blog"
url = "{}"
"#,
        remote.url()
    ));

    zoxide
        .command(ctx.cli())
        .arg("--config")
        .arg(config)
        .arg("sync")
        .arg("--dry-run")
        .arg("-z")
        .arg("blog")
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Zoxide"))
        .stderr(predicate::str::contains("? blog would register"));

    assert!(!ctx.workspace().join("blog").exists());
}

#[test]
fn sync_dry_run_register_zoxide_reports_existing_repository_without_running_zoxide() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let zoxide = FakeZoxide::new(&ctx).unavailable();
    let config = ctx.write_config(&format!(
        r#"
version = 1

[[repo]]
name = "blog"
path = "blog"
url = "{}"
"#,
        remote.url()
    ));
    ctx.cli().arg("--config").arg(&config).arg("sync").arg("blog").assert().success();

    zoxide
        .command(ctx.cli())
        .arg("--config")
        .arg(config)
        .arg("sync")
        .arg("--dry-run")
        .arg("-z")
        .arg("blog")
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Zoxide"))
        .stderr(predicate::str::contains("? blog would register"));
}

#[test]
fn sync_short_alias_plans_missing_clone() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = ctx.write_config(&format!(
        r#"
version = 1

[[repo]]
name = "blog"
path = "blog"
url = "{}"
"#,
        remote.url()
    ));

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("s")
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Would clone 1 repository"))
        .stderr(predicate::str::contains("+ blog"));
}

struct FakeZoxide {
    bin: std::path::PathBuf,
    data: std::path::PathBuf,
    unavailable: bool,
}

impl FakeZoxide {
    fn new(ctx: &TestContext) -> Self {
        Self {
            bin: ctx.root().join("fake-bin"),
            data: ctx.root().join("zoxide-data"),
            unavailable: false,
        }
    }

    fn unavailable(mut self) -> Self {
        self.unavailable = true;
        self
    }

    fn database(&self) -> std::path::PathBuf {
        self.data.join("db")
    }

    fn command(&self, mut command: assert_cmd::Command) -> assert_cmd::Command {
        self.install();
        command.env("PATH", self.path());
        command.env("_ZO_DATA_DIR", &self.data);
        command
    }

    fn path(&self) -> std::ffi::OsString {
        let original = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = std::env::split_paths(&original).collect::<Vec<_>>();
        paths.insert(0, self.bin.clone());
        std::env::join_paths(paths).expect("failed to join PATH")
    }

    fn install(&self) {
        std::fs::create_dir_all(&self.bin).expect("failed to create fake zoxide bin");
        std::fs::create_dir_all(&self.data).expect("failed to create fake zoxide data");
        let script = if self.unavailable {
            r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  echo "zoxide unavailable" >&2
  exit 1
fi
exit 1
"#
        } else {
            r#"#!/bin/sh
set -eu
if [ "$1" = "--version" ]; then
  echo "zoxide 0.10.0"
  exit 0
fi
if [ "$1" = "query" ]; then
  if [ -f "$_ZO_DATA_DIR/db" ]; then
    cat "$_ZO_DATA_DIR/db"
  fi
  exit 0
fi
if [ "$1" = "add" ]; then
  if [ "${_ZO_EXCLUDE_DIRS:-}" = "$2" ]; then
    exit 0
  fi
  printf '%s\n' "$2" >> "$_ZO_DATA_DIR/db"
  exit 0
fi
exit 1
"#
        };
        let path = self.bin.join("zoxide");
        std::fs::write(&path, script).expect("failed to write fake zoxide");
        make_executable(&path);
    }
}

fn resolved_repository_path(ctx: &TestContext, name: &str) -> std::path::PathBuf {
    std::fs::canonicalize(ctx.workspace()).expect("failed to resolve workspace path").join(name)
}

#[cfg(unix)]
fn make_executable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;

    let mut permissions =
        std::fs::metadata(path).expect("failed to inspect fake zoxide").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("failed to chmod fake zoxide");
}

#[test]
fn sync_uses_uv_change_colors_when_color_is_forced() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = ctx.write_config(&format!(
        r#"
version = 1

[[repo]]
name = "blog"
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

[[repo]]
name = "frontend"
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

#[test]
fn sync_omits_current_repository_rows_when_nothing_changed() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = ctx.write_config(&format!(
        r#"
version = 1

[[repo]]
name = "blog"
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

[[repo]]
name = "blog"
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

[[repo]]
name = "blog"
path = "blog"
url = "{}"
"#,
        remote.url()
    ));

    ctx.cli().arg("--config").arg(&initial_config).arg("sync").assert().success();

    let mismatched_config = ctx.write_config(&format!(
        r#"
version = 1

[[repo]]
name = "blog"
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

[[repo]]
name = "blog"
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

[[repo]]
name = "blog"
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
