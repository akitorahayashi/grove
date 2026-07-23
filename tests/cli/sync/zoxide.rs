use predicates::prelude::*;

use super::{FakeZoxide, resolved_repository_path};
use crate::harness::TestContext;

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
