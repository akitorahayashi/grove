use predicates::prelude::*;

use crate::harness::{TestContext, run_git};

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn sync_progress_completes_when_stderr_is_a_terminal() {
    use std::fs::File;
    use std::io::Read;
    use std::os::fd::FromRawFd;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");
    let config = ctx.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\n",
        remote.url()
    ));
    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();

    let path = install_git_wrapper(
        &ctx,
        "if [ \"$1\" = rev-parse ] && [ \"${2:-}\" = --is-inside-work-tree ]; then sleep 0.3; fi",
    );
    let mut master = -1;
    let mut slave = -1;
    let opened = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(opened, 0, "failed to open pseudo-terminal");
    let mut master = unsafe { File::from_raw_fd(master) };
    let slave = unsafe { File::from_raw_fd(slave) };
    let rendered = std::thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0; 4096];
        loop {
            match master.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => output.extend_from_slice(&buffer[..read]),
                Err(error) if error.raw_os_error() == Some(libc::EIO) => break,
                Err(error) => panic!("failed to read pseudo-terminal: {error}"),
            }
        }
        output
    });

    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("gv"))
        .current_dir(ctx.workspace())
        .env("XDG_CACHE_HOME", ctx.cache_home())
        .env("PATH", path)
        .arg("--config")
        .arg(config)
        .arg("sync")
        .stdout(Stdio::null())
        .stderr(Stdio::from(slave))
        .spawn()
        .expect("failed to run gv");

    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().expect("failed to poll gv") {
            break status;
        }
        if Instant::now() >= deadline {
            child.kill().expect("failed to kill deadlocked gv");
            child.wait().expect("failed to reap deadlocked gv");
            let rendered = rendered.join().expect("pseudo-terminal reader panicked");
            panic!(
                "gv did not finish while rendering progress to a terminal:\n{}",
                String::from_utf8_lossy(&rendered)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    };

    let _rendered = rendered.join().expect("pseudo-terminal reader panicked");
    assert!(status.success(), "gv exited with {status}");
}

#[test]
fn sync_dry_run_plans_missing_clone_without_creating_destination() {
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
        .arg("--dry-run")
        .assert()
        .success()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Would clone 1 repository"))
        .stderr(predicate::str::contains("+ blog"));

    assert!(!ctx.workspace().join("blog").exists());
}

#[test]
fn sync_dry_run_never_requires_or_creates_a_cache_root() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"https://example.com/blog.git\"\n",
    );

    ctx.cli()
        .env_remove("XDG_CACHE_HOME")
        .env_remove("HOME")
        .arg("--config")
        .arg(config)
        .arg("sync")
        .arg("--dry-run")
        .assert()
        .success()
        .stderr(predicate::str::contains("Would clone 1 repository"))
        .stderr(predicate::str::contains("cache directory").not());

    assert!(!ctx.workspace().join("blog").exists());
}

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
fn sync_short_alias_plans_missing_clone() {
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
    let path = install_git_wrapper(
        &ctx,
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
    let path =
        install_git_wrapper(&ctx, "if [ \"$1\" = merge ]; then echo merge-failed >&2; exit 42; fi");

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

#[cfg(unix)]
#[test]
fn sync_seeds_cache_for_existing_uncached_repository() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");

    // A repository already on disk, cloned outside grove, with no cache entry.
    let destination = ctx.workspace().join("blog");
    run_git(ctx.workspace(), &["clone", &remote.url(), destination.to_str().unwrap()]);
    assert_eq!(
        std::fs::read_dir(ctx.cache_root()).map(|dir| dir.count()).unwrap_or(0),
        0,
        "no cache entry exists before sync",
    );

    let config = ctx.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\n",
        remote.url()
    ));

    ctx.cli().arg("--config").arg(&config).arg("sync").assert().success();

    // Sync seeded the cache from the existing clone.
    ctx.cli()
        .arg("cache")
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("blog.git"));
}

#[test]
fn sync_seeds_cache_from_dirty_existing_repository() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");

    // An existing clone with an uncommitted change, and no cache entry.
    let destination = ctx.workspace().join("blog");
    run_git(ctx.workspace(), &["clone", &remote.url(), destination.to_str().unwrap()]);
    std::fs::write(destination.join("README.md"), "local edit\n").unwrap();

    let config = ctx.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\n",
        remote.url()
    ));

    // A dirty repository is left untouched (a skip exits non-zero) yet still
    // seeds the cache from its objects.
    ctx.cli()
        .arg("--config")
        .arg(&config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("dirty working tree"));

    ctx.cli()
        .arg("cache")
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("blog.git"));

    // The uncommitted change was preserved.
    assert_eq!(std::fs::read_to_string(destination.join("README.md")).unwrap(), "local edit\n");
}

#[test]
fn sync_seeds_cache_from_diverged_existing_repository() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");

    // An existing clone with a local-only commit: ahead of origin, so grove
    // blocks the update. There is no cache entry.
    let destination = ctx.workspace().join("blog");
    run_git(ctx.workspace(), &["clone", &remote.url(), destination.to_str().unwrap()]);
    std::fs::write(destination.join("local.txt"), "local\n").unwrap();
    run_git(&destination, &["add", "local.txt"]);
    run_git(
        &destination,
        &["-c", "user.name=T", "-c", "user.email=t@e.x", "commit", "-m", "local"],
    );

    let config = ctx.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\n",
        remote.url()
    ));

    ctx.cli()
        .arg("--config")
        .arg(&config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("ahead of origin"));

    ctx.cli()
        .arg("cache")
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("blog.git"));
}

#[test]
fn sync_seeds_cache_from_detached_head_existing_repository() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("blog");

    // An existing clone on a detached HEAD: grove leaves it untouched, but it
    // still seeds the cache. There is no cache entry.
    let destination = ctx.workspace().join("blog");
    run_git(ctx.workspace(), &["clone", &remote.url(), destination.to_str().unwrap()]);
    run_git(&destination, &["checkout", "--detach"]);

    let config = ctx.write_config(&format!(
        "version = 1\n[repos.blog]\npath = \"blog\"\nurl = \"{}\"\n",
        remote.url()
    ));

    ctx.cli()
        .arg("--config")
        .arg(&config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("detached HEAD"));

    ctx.cli()
        .arg("cache")
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("blog.git"));
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
    let path = install_git_wrapper(
        &ctx,
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
    commit_local(&ahead.workspace().join("blog"), "ahead.txt");
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
    commit_local(&diverged.workspace().join("blog"), "local.txt");
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

fn commit_local(repository: &std::path::Path, file: &str) {
    std::fs::write(repository.join(file), "local\n").unwrap();
    run_git(repository, &["add", file]);
    run_git(
        repository,
        &["-c", "user.name=Grove Test", "-c", "user.email=grove@example.com", "commit", "-m", file],
    );
}

#[cfg(unix)]
fn install_git_wrapper(ctx: &TestContext, behavior: &str) -> std::ffi::OsString {
    use std::os::unix::fs::PermissionsExt;

    let command = std::process::Command::new("sh").args(["-c", "command -v git"]).output().unwrap();
    let real_git = String::from_utf8_lossy(&command.stdout).trim().to_string();
    let bin = ctx.root().join("git-wrapper-bin");
    std::fs::create_dir(&bin).unwrap();
    let wrapper = bin.join("git");
    std::fs::write(&wrapper, format!("#!/bin/sh\n{behavior}\nexec \"{real_git}\" \"$@\"\n"))
        .unwrap();
    let mut permissions = std::fs::metadata(&wrapper).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&wrapper, permissions).unwrap();
    let mut paths =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect::<Vec<_>>();
    paths.insert(0, bin);
    std::env::join_paths(paths).unwrap()
}
