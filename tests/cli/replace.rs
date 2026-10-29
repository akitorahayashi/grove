use std::fs;
use std::path::{Path, PathBuf};

use predicates::prelude::*;

use crate::harness::{RemoteRepository, TestContext, git_stdout, run_git};

/// A candidate outside the grove, as a provisioning tool renders it.
fn candidate(ctx: &TestContext, contents: &str) -> PathBuf {
    let path = ctx.root().join("candidate").join("grove.toml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, contents).unwrap();
    path
}

fn repositories(entries: &[(&str, &RemoteRepository)]) -> String {
    let mut contents = String::from("version = 2\n\n[repos]\n");
    for (name, remote) in entries {
        contents.push_str(&format!("{name} = \"{}\"\n", remote.url()));
    }
    contents
}

/// A grove declaring `entries`, placed and cached by an initial sync.
fn synced_grove(ctx: &TestContext, entries: &[(&str, &RemoteRepository)]) -> String {
    let contents = repositories(entries);
    ctx.write_config(&contents);
    ctx.cli().arg("sync").assert().success();
    contents
}

/// A renamed hosting repository: a bare copy of `old` under a new URL.
fn rename_remote(ctx: &TestContext, old: &RemoteRepository, name: &str) -> String {
    let renamed = ctx.root().join("remotes").join(format!("{name}.git"));
    run_git(ctx.root(), &["clone", "--quiet", "--bare", &old.url(), renamed.to_str().unwrap()]);
    renamed.display().to_string()
}

fn origin(repository: &Path) -> String {
    git_stdout(repository, &["config", "--get", "remote.origin.url"])
}

fn configuration(ctx: &TestContext) -> String {
    fs::read_to_string(ctx.config_path()).unwrap()
}

fn cached(ctx: &TestContext) -> String {
    let output = ctx.cli().args(["cache", "list"]).output().unwrap();
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn replace_creates_an_absent_named_destination_without_cloning() {
    let ctx = TestContext::new();
    let tool = ctx.create_remote("tool");
    let candidate = candidate(&ctx, &repositories(&[("tool", &tool)]));

    ctx.cli()
        .arg("replace")
        .arg(&candidate)
        .assert()
        .failure()
        .stderr(predicate::str::contains("grove.toml was not found"));
    ctx.cli()
        .arg("--config")
        .arg(ctx.workspace().join("missing/grove.toml"))
        .arg("replace")
        .arg(&candidate)
        .assert()
        .failure();

    ctx.cli()
        .arg("--config")
        .arg(ctx.config_path())
        .arg("replace")
        .arg(&candidate)
        .assert()
        .success()
        .stdout(predicate::str::contains("grove.toml written"))
        .stdout(predicate::str::contains("1 repository not cloned; run gv sync"))
        .stderr(predicate::str::is_empty());

    assert_eq!(configuration(&ctx), fs::read_to_string(&candidate).unwrap());
    assert!(!ctx.workspace().join("tool").exists());
}

#[cfg(unix)]
#[test]
fn replace_with_an_identical_candidate_writes_nothing() {
    use std::os::unix::fs::MetadataExt;

    let ctx = TestContext::new();
    let tool = ctx.create_remote("tool");
    let contents = synced_grove(&ctx, &[("tool", &tool)]);
    let candidate = candidate(&ctx, &contents);
    let before = fs::metadata(ctx.config_path()).unwrap().ino();

    ctx.cli()
        .arg("replace")
        .arg(&candidate)
        .assert()
        .success()
        .stdout(predicate::str::contains("grove.toml already current"));

    assert_eq!(fs::metadata(ctx.config_path()).unwrap().ino(), before);
}

#[cfg(target_os = "linux")]
#[test]
fn replace_trashes_a_clean_leaving_repository_and_drops_its_cache_entry() {
    let ctx = TestContext::new();
    let kept = ctx.create_remote("kept");
    let old = ctx.create_remote("old");
    synced_grove(&ctx, &[("kept", &kept), ("old", &old)]);
    let candidate = candidate(&ctx, &repositories(&[("kept", &kept)]));
    let data_home = ctx.root().join("data-home");
    fs::create_dir_all(&data_home).unwrap();

    ctx.cli()
        .env("XDG_DATA_HOME", &data_home)
        .arg("replace")
        .arg(&candidate)
        .assert()
        .success()
        .stdout(predicate::str::contains("Trashed 1 repository"))
        .stdout(predicate::str::contains("- old moved to the trash"))
        .stdout(predicate::str::contains("clone cache entry removed"))
        .stdout(predicate::str::contains("grove.toml written"));

    assert!(!ctx.workspace().join("old").exists());
    assert!(data_home.join("Trash/files/old/.git").exists());
    assert!(ctx.workspace().join("kept/.git").exists());
    assert_eq!(configuration(&ctx), fs::read_to_string(&candidate).unwrap());
    let cached = cached(&ctx);
    assert!(!cached.contains("old.git") && cached.contains("kept.git"), "{cached}");
}

#[test]
fn replace_changes_nothing_while_a_leaving_repository_holds_local_work() {
    let ctx = TestContext::new();
    let kept = ctx.create_remote("kept");
    let old = ctx.create_remote("old");
    let contents = synced_grove(&ctx, &[("kept", &kept), ("old", &old)]);
    fs::write(ctx.workspace().join("old/untracked.txt"), "local\n").unwrap();
    let candidate = candidate(&ctx, &repositories(&[("kept", &kept)]));

    ctx.cli()
        .args(["replace", "--dry-run"])
        .arg(&candidate)
        .assert()
        .failure()
        .stdout(predicate::str::contains("Blocked 1 repository"));
    ctx.cli()
        .arg("replace")
        .arg(&candidate)
        .assert()
        .failure()
        .stdout(predicate::str::contains("Blocked 1 repository"))
        .stdout(predicate::str::contains("x old local-only state: uncommitted changes"))
        .stdout(predicate::str::contains("grove.toml kept until every repository converges"));

    assert_eq!(configuration(&ctx), contents);
    assert!(ctx.workspace().join("old/untracked.txt").exists());
    assert!(cached(&ctx).contains("old.git"));
}

#[test]
fn replace_moves_a_repository_to_the_path_the_candidate_declares_for_its_url() {
    let ctx = TestContext::new();
    let tool = ctx.create_remote("tool");
    synced_grove(&ctx, &[("tool", &tool)]);
    fs::write(ctx.workspace().join("tool/untracked.txt"), "local\n").unwrap();
    let candidate =
        candidate(&ctx, &format!("version = 2\n\n[groups.apps]\ntool = \"{}\"\n", tool.url()));

    ctx.cli()
        .arg("replace")
        .arg(&candidate)
        .assert()
        .success()
        .stdout(predicate::str::contains("> tool moved to apps/tool"))
        .stdout(predicate::str::contains("clone cache entry").not())
        .stdout(predicate::str::contains("not cloned").not());

    assert!(!ctx.workspace().join("tool").exists());
    assert!(ctx.workspace().join("apps/tool/untracked.txt").exists());
    assert!(cached(&ctx).contains("tool.git"));
}

#[test]
fn replace_repoints_an_origin_that_still_names_the_previous_url() {
    let ctx = TestContext::new();
    let tool = ctx.create_remote("tool");
    synced_grove(&ctx, &[("tool", &tool)]);
    let renamed = rename_remote(&ctx, &tool, "renamed");
    let candidate = candidate(&ctx, &format!("version = 2\n\n[repos]\ntool = \"{renamed}\"\n"));

    ctx.cli()
        .args(["replace", "--dry-run"])
        .arg(&candidate)
        .assert()
        .success()
        .stdout(predicate::str::contains("Would repoint 1 repository"))
        .stdout(predicate::str::contains("~ tool would repoint origin to"));
    assert_eq!(origin(&ctx.workspace().join("tool")), tool.url());

    ctx.cli()
        .arg("replace")
        .arg(&candidate)
        .assert()
        .success()
        .stdout(predicate::str::contains("~ tool origin repointed to"));

    assert_eq!(origin(&ctx.workspace().join("tool")), renamed);
    assert_eq!(configuration(&ctx), fs::read_to_string(&candidate).unwrap());
}

#[test]
fn replace_leaves_an_origin_already_on_the_candidate_url() {
    let ctx = TestContext::new();
    let tool = ctx.create_remote("tool");
    synced_grove(&ctx, &[("tool", &tool)]);
    let renamed = rename_remote(&ctx, &tool, "renamed");
    run_git(&ctx.workspace().join("tool"), &["remote", "set-url", "origin", &renamed]);
    let candidate = candidate(&ctx, &format!("version = 2\n\n[repos]\ntool = \"{renamed}\"\n"));

    ctx.cli()
        .arg("replace")
        .arg(&candidate)
        .assert()
        .success()
        .stdout(predicate::str::contains("repoint").not())
        .stdout(predicate::str::contains("grove.toml written"));

    assert_eq!(origin(&ctx.workspace().join("tool")), renamed);
    assert_eq!(configuration(&ctx), fs::read_to_string(&candidate).unwrap());
}

#[cfg(unix)]
#[test]
fn replace_blocks_a_repoint_at_a_symbolic_link() {
    let ctx = TestContext::new();
    let tool = ctx.create_remote("tool");
    let contents = synced_grove(&ctx, &[("tool", &tool)]);
    let target = ctx.workspace().join("elsewhere");
    fs::rename(ctx.workspace().join("tool"), &target).unwrap();
    std::os::unix::fs::symlink(&target, ctx.workspace().join("tool")).unwrap();
    let renamed = rename_remote(&ctx, &tool, "renamed");
    let candidate = candidate(&ctx, &format!("version = 2\n\n[repos]\ntool = \"{renamed}\"\n"));

    ctx.cli()
        .arg("replace")
        .arg(&candidate)
        .assert()
        .failure()
        .stdout(predicate::str::contains("x tool path is a symbolic link"));

    assert_eq!(origin(&target), tool.url());
    assert_eq!(configuration(&ctx), contents);
}

#[test]
fn replace_blocks_a_repoint_whose_origin_names_neither_url() {
    let ctx = TestContext::new();
    let tool = ctx.create_remote("tool");
    let contents = synced_grove(&ctx, &[("tool", &tool)]);
    let renamed = rename_remote(&ctx, &tool, "renamed");
    let elsewhere = rename_remote(&ctx, &tool, "elsewhere");
    run_git(&ctx.workspace().join("tool"), &["remote", "set-url", "origin", &elsewhere]);
    let candidate = candidate(&ctx, &format!("version = 2\n\n[repos]\ntool = \"{renamed}\"\n"));

    ctx.cli()
        .arg("replace")
        .arg(&candidate)
        .assert()
        .failure()
        .stdout(predicate::str::contains(
            "x tool remote URL matches neither the previous nor the declared URL",
        ))
        .stdout(predicate::str::contains(format!("actual:   {elsewhere}")))
        .stdout(predicate::str::contains(format!("expected: {renamed}")));

    assert_eq!(origin(&ctx.workspace().join("tool")), elsewhere);
    assert_eq!(configuration(&ctx), contents);
}

#[test]
fn replace_keeps_override_repositories_and_rejects_a_candidate_conflicting_with_the_override() {
    let ctx = TestContext::new();
    let base = ctx.create_remote("base");
    let private = ctx.create_remote("private");
    let extra = ctx.create_remote("extra");
    ctx.write_config(&repositories(&[("base", &base)]));
    ctx.write_config_at(
        "grove.override.toml",
        &format!("[repos]\nprivate = \"{}\"\n", private.url()),
    );
    ctx.cli().arg("sync").assert().success();

    let conflicting = candidate(
        &ctx,
        &format!("version = 2\n\n[repos.other]\nurl = \"{}\"\npath = \"private\"\n", extra.url()),
    );
    ctx.cli()
        .arg("replace")
        .arg(&conflicting)
        .assert()
        .failure()
        .stderr(predicate::str::contains(conflicting.display().to_string()))
        .stderr(predicate::str::contains("would leave an invalid configuration"));
    assert_eq!(configuration(&ctx), repositories(&[("base", &base)]));

    let candidate = candidate(&ctx, &repositories(&[("base", &base), ("extra", &extra)]));
    ctx.cli()
        .arg("replace")
        .arg(&candidate)
        .assert()
        .success()
        .stdout(predicate::str::contains(" private ").not())
        .stdout(predicate::str::contains("1 repository not cloned; run gv sync"));

    assert!(ctx.workspace().join("private/.git").exists());
    assert_eq!(configuration(&ctx), fs::read_to_string(&candidate).unwrap());
}

#[test]
fn replace_dry_run_reports_the_plan_and_changes_nothing() {
    let ctx = TestContext::new();
    let kept = ctx.create_remote("kept");
    let old = ctx.create_remote("old");
    let contents = synced_grove(&ctx, &[("kept", &kept), ("old", &old)]);
    let candidate = candidate(&ctx, &repositories(&[("kept", &kept)]));

    ctx.cli()
        .args(["replace", "--dry-run"])
        .arg(&candidate)
        .assert()
        .success()
        .stdout(predicate::str::contains("Would trash 1 repository"))
        .stdout(predicate::str::contains("- old would move to the trash"))
        .stdout(predicate::str::contains("clone cache entry would be removed"))
        .stdout(predicate::str::contains("grove.toml would be written"));

    assert_eq!(configuration(&ctx), contents);
    assert!(ctx.workspace().join("old/.git").exists());
    assert!(cached(&ctx).contains("old.git"));
}

#[test]
fn replace_converges_a_removed_declaration_new_to_the_candidate() {
    let ctx = TestContext::new();
    let old = ctx.create_remote("old");
    synced_grove(&ctx, &[("old", &old)]);
    let renamed = rename_remote(&ctx, &old, "renamed");
    let candidate = candidate(
        &ctx,
        &format!(
            "version = 2\n\n[repos]\nrenamed = \"{renamed}\"\n\n\
             [removed.repos]\nold = {{ url = \"{}\", moved_to = \"renamed\" }}\n",
            old.url()
        ),
    );

    ctx.cli()
        .arg("replace")
        .arg(&candidate)
        .assert()
        .success()
        .stdout(predicate::str::contains("> old moved to renamed"))
        .stdout(predicate::str::contains("clone cache entry removed"));

    assert!(!ctx.workspace().join("old").exists());
    assert_eq!(origin(&ctx.workspace().join("renamed")), renamed);
}

#[test]
fn replace_leaves_an_unchanged_removed_declaration_to_sync() {
    let ctx = TestContext::new();
    let old = ctx.create_remote("old");
    let extra = ctx.create_remote("extra");
    run_git(ctx.workspace(), &["clone", "--quiet", &old.url(), "old"]);
    let tombstone = format!("\n[removed.repos]\nold = \"{}\"\n", old.url());
    ctx.write_config(&format!("version = 2\n{tombstone}"));
    let candidate = candidate(&ctx, &format!("{}{tombstone}", repositories(&[("extra", &extra)])));

    ctx.cli()
        .arg("replace")
        .arg(&candidate)
        .assert()
        .success()
        .stdout(predicate::str::contains(" old ").not())
        .stdout(predicate::str::contains("grove.toml written"));

    assert!(ctx.workspace().join("old/.git").exists());
}

#[test]
fn replace_fails_before_any_change_for_an_unreadable_or_invalid_configuration() {
    let ctx = TestContext::new();
    let candidate = candidate(&ctx, "version = 2\n");
    ctx.write_config("version = 2\n");

    ctx.cli()
        .arg("replace")
        .arg(ctx.root().join("missing.toml"))
        .assert()
        .failure()
        .stderr(predicate::str::contains("missing.toml"));

    ctx.write_config("[repos]\n");
    ctx.cli()
        .arg("replace")
        .arg(&candidate)
        .assert()
        .failure()
        .stderr(predicate::str::contains("missing required field 'version'"));
    assert_eq!(configuration(&ctx), "[repos]\n");
}
