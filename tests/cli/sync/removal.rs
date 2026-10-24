use std::fs;
use std::path::{Path, PathBuf};

use predicates::prelude::*;

use crate::harness::{RemoteRepository, TestContext, commit_file, git_stdout, run_git};

/// A renamed hosting repository: a bare copy of `old` under a new URL, so the
/// moved clone fast-forwards against it as it would after a GitHub rename.
fn rename_remote(ctx: &TestContext, old: &RemoteRepository, name: &str) -> String {
    let renamed = ctx.root().join("remotes").join(format!("{name}.git"));
    run_git(ctx.root(), &["clone", "--quiet", "--bare", &old.url(), renamed.to_str().unwrap()]);
    renamed.display().to_string()
}

fn clone_into(ctx: &TestContext, url: &str, relative: &str) -> PathBuf {
    let destination = ctx.workspace().join(relative);
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    run_git(ctx.workspace(), &["clone", "--quiet", url, destination.to_str().unwrap()]);
    destination
}

type Preparation = fn(&Path);

fn rename_config(ctx: &TestContext, old_url: &str, new_url: &str) -> PathBuf {
    ctx.write_config(&format!(
        r#"
version = 2

[groups.Skills]
new = "{new_url}"

[removed.groups.Skills]
old = {{ url = "{old_url}", moved_to = "new" }}
"#
    ))
}

fn trash_config(ctx: &TestContext, old_url: &str) -> PathBuf {
    ctx.write_config(&format!("version = 2\n\n[removed.repos]\nold = \"{old_url}\"\n"))
}

#[test]
fn sync_moves_a_renamed_repository_keeping_local_files_and_updates_it() {
    let ctx = TestContext::new();
    let old = ctx.create_remote("old");
    let checkout = clone_into(&ctx, &old.url(), "Skills/old");
    fs::write(checkout.join(".git/info/exclude"), ".tmp/\n").unwrap();
    fs::create_dir(checkout.join(".tmp")).unwrap();
    fs::write(checkout.join(".tmp/notes.md"), "local\n").unwrap();
    old.add_commit("upstream.txt", "upstream\n");
    let new_url = rename_remote(&ctx, &old, "new");
    let config = rename_config(&ctx, &old.url(), &new_url);

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("Removed 1 repository"))
        .stderr(predicate::str::contains("> Skills/old moved to Skills/new"))
        .stderr(predicate::str::contains("~ Skills/new"))
        .stderr(predicate::str::contains("+ Skills/new").not());

    let moved = ctx.workspace().join("Skills/new");
    assert!(!ctx.workspace().join("Skills/old").exists());
    assert_eq!(fs::read_to_string(moved.join(".tmp/notes.md")).unwrap(), "local\n");
    assert!(moved.join("upstream.txt").exists());
    assert_eq!(git_stdout(&moved, &["config", "--get", "remote.origin.url"]), new_url);
}

#[test]
fn sync_moves_a_repository_whose_origin_already_points_to_the_destination() {
    let ctx = TestContext::new();
    let old = ctx.create_remote("old");
    let checkout = clone_into(&ctx, &old.url(), "Skills/old");
    let new_url = rename_remote(&ctx, &old, "new");
    run_git(&checkout, &["remote", "set-url", "origin", &new_url]);
    let config = rename_config(&ctx, &old.url(), &new_url);

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("> Skills/old moved to Skills/new"));

    assert!(ctx.workspace().join("Skills/new/.git").exists());
}

#[test]
fn sync_moves_a_repository_between_groups_under_the_same_name_and_keeps_its_cache() {
    let ctx = TestContext::new();
    let remote = ctx.create_remote("tool");
    ctx.write_config(&format!("version = 2\n\n[groups.apps]\ntool = \"{}\"\n", remote.url()));
    ctx.cli().arg("sync").assert().success();
    let config = ctx.write_config(&format!(
        r#"
version = 2

[groups.tools]
tool = "{url}"

[removed.groups.apps]
tool = {{ url = "{url}", moved_to = "tool" }}
"#,
        url = remote.url()
    ));

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("> apps/tool moved to tools/tool"))
        .stderr(predicate::str::contains("clone cache entry removed").not());

    assert!(ctx.workspace().join("tools/tool/.git").exists());
    ctx.cli()
        .args(["cache", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("tool.git"));
}

#[test]
fn sync_dry_run_plans_removals_without_changing_anything() {
    let ctx = TestContext::new();
    let old = ctx.create_remote("old");
    clone_into(&ctx, &old.url(), "Skills/old");
    let new_url = rename_remote(&ctx, &old, "new");
    let config = rename_config(&ctx, &old.url(), &new_url);

    ctx.cli()
        .arg("--config")
        .arg(config)
        .args(["sync", "--dry-run"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Would move 1 repository"))
        .stderr(predicate::str::contains("> Skills/old would move to Skills/new"))
        .stderr(predicate::str::contains("Would clone").not());

    assert!(ctx.workspace().join("Skills/old/.git").exists());
    assert!(!ctx.workspace().join("Skills/new").exists());
}

#[test]
fn sync_leaves_paths_it_cannot_prove_it_owns() {
    let ctx = TestContext::new();
    let old = ctx.create_remote("old");
    let other = ctx.create_remote("other");
    let config = trash_config(&ctx, &old.url());

    fs::create_dir_all(ctx.workspace().join("old")).unwrap();
    fs::write(ctx.workspace().join("old/file.txt"), "plain\n").unwrap();
    ctx.cli()
        .arg("--config")
        .arg(&config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Blocked 1 repository"))
        .stderr(predicate::str::contains("x old destination exists but is not a Git repository"));
    assert!(ctx.workspace().join("old/file.txt").exists());

    fs::remove_dir_all(ctx.workspace().join("old")).unwrap();
    clone_into(&ctx, &other.url(), "old");
    ctx.cli().arg("--config").arg(&config).arg("sync").assert().failure().stderr(
        predicate::str::contains(
            "x old remote URL matches neither the removed nor the moved_to URL",
        ),
    );
    assert!(ctx.workspace().join("old/.git").exists());
}

#[cfg(unix)]
#[test]
fn sync_leaves_a_symbolic_link_and_the_repository_it_points_to() {
    let ctx = TestContext::new();
    let old = ctx.create_remote("old");
    let target = clone_into(&ctx, &old.url(), "actual");
    std::os::unix::fs::symlink(&target, ctx.workspace().join("old")).unwrap();
    let config = trash_config(&ctx, &old.url());

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("x old path is a symbolic link"));
    assert!(ctx.workspace().join("old").symlink_metadata().unwrap().file_type().is_symlink());
    assert!(target.join(".git").exists());
}

#[test]
fn sync_leaves_repositories_with_linked_worktrees() {
    let ctx = TestContext::new();
    let old = ctx.create_remote("old");
    let checkout = clone_into(&ctx, &old.url(), "old");
    run_git(&checkout, &["worktree", "add", "--quiet", "-b", "side", "../side"]);
    let config = trash_config(&ctx, &old.url());

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("x old repository has linked worktrees"));

    assert!(checkout.join(".git").exists());
}

#[test]
fn sync_leaves_repositories_holding_local_only_state() {
    let cases: [(&str, Preparation); 4] = [
        ("uncommitted changes", |checkout| {
            fs::write(checkout.join("untracked.txt"), "local\n").unwrap();
        }),
        ("stash", |checkout| {
            fs::write(checkout.join("README.md"), "changed\n").unwrap();
            run_git(
                checkout,
                &[
                    "-c",
                    "user.name=Grove Test",
                    "-c",
                    "user.email=grove@example.com",
                    "stash",
                    "--quiet",
                ],
            );
        }),
        ("unpushed commits", |checkout| {
            run_git(checkout, &["switch", "--quiet", "-c", "local"]);
            commit_file(checkout, "local.txt");
        }),
        ("unpushed commits", |checkout| {
            run_git(checkout, &["switch", "--quiet", "--detach"]);
            commit_file(checkout, "detached.txt");
        }),
    ];

    for (state, prepare) in cases {
        let ctx = TestContext::new();
        let old = ctx.create_remote("old");
        let checkout = clone_into(&ctx, &old.url(), "old");
        prepare(&checkout);
        let config = trash_config(&ctx, &old.url());

        ctx.cli()
            .arg("--config")
            .arg(config)
            .arg("sync")
            .assert()
            .failure()
            .stderr(predicate::str::contains("x old local-only state:"))
            .stderr(predicate::str::contains(state));

        assert!(checkout.join(".git").exists());
    }
}

#[test]
fn sync_does_not_move_onto_an_existing_destination() {
    let ctx = TestContext::new();
    let old = ctx.create_remote("old");
    let checkout = clone_into(&ctx, &old.url(), "Skills/old");
    fs::write(checkout.join("untracked.txt"), "local\n").unwrap();
    let new_url = rename_remote(&ctx, &old, "new");
    clone_into(&ctx, &new_url, "Skills/new");
    let config = rename_config(&ctx, &old.url(), &new_url);

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("x Skills/old local-only state: uncommitted changes"));

    assert!(checkout.join("untracked.txt").exists());
}

#[test]
fn sync_removes_the_cache_entry_of_a_removed_url_even_without_a_checkout() {
    let ctx = TestContext::new();
    let old = ctx.create_remote("old");
    ctx.write_single_repository_config("old", &old.url(), None);
    ctx.cli().arg("sync").assert().success();
    fs::remove_dir_all(ctx.workspace().join("old")).unwrap();
    let config = trash_config(&ctx, &old.url());

    ctx.cli()
        .arg("--config")
        .arg(&config)
        .args(["sync", "--dry-run"])
        .assert()
        .success()
        .stderr(predicate::str::contains("= old"))
        .stderr(predicate::str::contains("clone cache entry would be removed"));
    ctx.cli()
        .arg("--config")
        .arg(&config)
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("clone cache entry removed"));

    ctx.cli()
        .args(["cache", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("old.git").not());
}

#[test]
fn sync_ignore_overrides_skips_removed_declarations_from_the_override() {
    let ctx = TestContext::new();
    let old = ctx.create_remote("old");
    clone_into(&ctx, &old.url(), "old");
    let config = ctx.write_config("version = 2\n");
    ctx.write_config_at(
        "grove.override.toml",
        &format!("[removed.repos]\nold = \"{}\"\n", old.url()),
    );

    ctx.cli()
        .arg("--config")
        .arg(&config)
        .args(["sync", "--dry-run", "-i"])
        .assert()
        .success()
        .stderr(predicate::str::contains("old").not());
    ctx.cli()
        .arg("--config")
        .arg(&config)
        .args(["sync", "--dry-run"])
        .assert()
        .success()
        .stderr(predicate::str::contains("- old would move to the trash"));
}

#[test]
fn sync_targets_only_removals_moving_to_a_selected_repository() {
    let ctx = TestContext::new();
    let old = ctx.create_remote("old");
    let unrelated = ctx.create_remote("unrelated");
    clone_into(&ctx, &old.url(), "Skills/old");
    let new_url = rename_remote(&ctx, &old, "new");
    let config = ctx.write_config(&format!(
        r#"
version = 2

[groups.Skills]
new = "{new_url}"
other = "{other}"

[removed.groups.Skills]
old = {{ url = "{old_url}", moved_to = "new" }}
"#,
        other = unrelated.url(),
        old_url = old.url(),
    ));

    ctx.cli()
        .arg("--config")
        .arg(&config)
        .args(["sync", "other"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Skills/old").not());
    assert!(ctx.workspace().join("Skills/old").exists());

    ctx.cli()
        .arg("--config")
        .arg(&config)
        .args(["sync", "new"])
        .assert()
        .success()
        .stderr(predicate::str::contains("> Skills/old moved to Skills/new"));
}

#[cfg(target_os = "linux")]
#[test]
fn sync_moves_a_clean_removed_repository_to_the_trash() {
    let ctx = TestContext::new();
    let old = ctx.create_remote("old");
    clone_into(&ctx, &old.url(), "old");
    let config = trash_config(&ctx, &old.url());
    let data_home = ctx.root().join("data-home");
    fs::create_dir_all(&data_home).unwrap();

    ctx.cli()
        .env("XDG_DATA_HOME", &data_home)
        .arg("--config")
        .arg(config)
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("- old moved to the trash"));

    assert!(!ctx.workspace().join("old").exists());
    assert!(data_home.join("Trash/files/old/.git").exists());
}
