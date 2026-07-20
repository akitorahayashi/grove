use predicates::prelude::*;
use std::process::{Command, Stdio};

use crate::harness::TestContext;

#[test]
fn help_lists_mvp_commands() {
    let ctx = TestContext::new();

    ctx.cli()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("init"))
        .stdout(predicate::str::contains("i"))
        .stdout(predicate::str::contains("sync"))
        .stdout(predicate::str::contains("s"))
        .stdout(predicate::str::contains("status"))
        .stdout(predicate::str::contains("st"))
        .stdout(predicate::str::contains("validate"))
        .stdout(predicate::str::is_match(r"(?m)^\s+validate\s+.*\[aliases: vl\]").unwrap())
        .stdout(predicate::str::contains("list").not())
        .stdout(predicate::str::contains("ls").not());
}

#[test]
fn version_uses_grove_package() {
    let ctx = TestContext::new();

    ctx.cli().arg("--version").assert().success().stdout(predicate::str::contains("gv 0.1.0"));
}

#[test]
fn early_closed_stdout_exits_without_panic() {
    let ctx = TestContext::new();
    let mut config = String::from("version = 1\n");
    for index in 0..2_000 {
        config.push_str(&format!(
            "[[repo]]\nname = \"repo-{index}\"\npath = \"repo-{index}\"\nurl = \"git@example.com:repo-{index}.git\"\n"
        ));
    }
    let config = ctx.write_config(&config);
    let mut child = Command::new(assert_cmd::cargo::cargo_bin!("gv"))
        .current_dir(ctx.workspace())
        .arg("--config")
        .arg(config)
        .arg("status")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());

    let output = child.wait_with_output().unwrap();

    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.contains("panicked"));
    assert!(!stderr.contains("Broken pipe"));
}
