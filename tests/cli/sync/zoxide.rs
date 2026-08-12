use predicates::prelude::*;

use crate::harness::TestContext;

#[test]
fn sync_register_zoxide_adds_existing_repository_when_missing_from_zoxide() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let zoxide = FakeZoxide::new(&ctx);
    let config = ctx.write_config(&format!(
        r#"
version = 1

[repos.blog]
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

[repos.blog]
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

[repos.blog]
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

[repos.blog]
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

[repos.blog]
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
fn sync_register_zoxide_queries_database_at_most_twice() {
    let ctx = TestContext::new();
    let first = ctx.create_remote("first");
    let second = ctx.create_remote("second");
    let zoxide = FakeZoxide::new(&ctx);
    let config = ctx.write_config(&format!(
        r#"
version = 1

[repos.first]
path = "first"
url = "{}"

[repos.second]
path = "second"
url = "{}"
"#,
        first.url(),
        second.url()
    ));

    zoxide.command(ctx.cli()).arg("--config").arg(config).arg("sync").arg("-z").assert().success();

    let invocations = std::fs::read_to_string(zoxide.invocations()).unwrap();
    assert_eq!(invocations.lines().filter(|line| *line == "query --list --all").count(), 2);
}

#[test]
fn sync_rejects_zoxide_missing_required_add_capability_before_add() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let zoxide = FakeZoxide::new(&ctx).missing_add_capability();
    let config = ctx.write_config(&format!(
        r#"
version = 1

[repos.blog]
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
        .assert()
        .failure()
        .stderr(predicate::str::contains("required capability `zoxide add --help`"));

    assert!(!zoxide.database().exists());
    let invocations = std::fs::read_to_string(zoxide.invocations()).unwrap();
    assert!(!invocations.lines().any(|line| line.starts_with("add ") && line != "add --help"));
}

#[test]
fn sync_register_zoxide_adds_cloned_repository() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let zoxide = FakeZoxide::new(&ctx);
    let config = ctx.write_config(&format!(
        r#"
version = 1

[repos.blog]
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

[repos.blog]
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

struct FakeZoxide {
    bin: std::path::PathBuf,
    data: std::path::PathBuf,
    unavailable: bool,
    missing_add_capability: bool,
}

impl FakeZoxide {
    fn new(ctx: &TestContext) -> Self {
        Self {
            bin: ctx.root().join("fake-bin"),
            data: ctx.root().join("zoxide-data"),
            unavailable: false,
            missing_add_capability: false,
        }
    }

    fn unavailable(mut self) -> Self {
        self.unavailable = true;
        self
    }

    fn missing_add_capability(mut self) -> Self {
        self.missing_add_capability = true;
        self
    }

    fn database(&self) -> std::path::PathBuf {
        self.data.join("db")
    }

    fn invocations(&self) -> std::path::PathBuf {
        self.data.join("invocations")
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
        } else if self.missing_add_capability {
            r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$_ZO_DATA_DIR/invocations"
if [ "$1" = "--version" ]; then
  echo "zoxide 0.10.0"
  exit 0
fi
if [ "$1" = "query" ] && [ "${2:-}" = "--help" ]; then
  exit 0
fi
if [ "$1" = "add" ] && [ "${2:-}" = "--help" ]; then
  echo "add unavailable" >&2
  exit 1
fi
exit 1
"#
        } else {
            r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$_ZO_DATA_DIR/invocations"
if [ "$1" = "--version" ]; then
  echo "zoxide 0.10.0"
  exit 0
fi
if [ "$1" = "query" ]; then
  if [ "${2:-}" = "--help" ]; then
    exit 0
  fi
  if [ -f "$_ZO_DATA_DIR/db" ]; then
    cat "$_ZO_DATA_DIR/db"
  fi
  exit 0
fi
if [ "$1" = "add" ]; then
  if [ "${2:-}" = "--help" ]; then
    exit 0
  fi
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
