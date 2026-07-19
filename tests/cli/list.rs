use predicates::prelude::*;

use crate::harness::TestContext;

#[test]
fn list_outputs_repositories_from_root_and_include() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1
include = ["work/grove.toml"]

[[repo]]
name = "dotfiles"
path = "personal/dotfiles"
url = "git@example.com:dotfiles.git"
"#,
    );
    ctx.write_config_at(
        "work/grove.toml",
        r#"
version = 1

[[repo]]
name = "frontend"
path = "frontend"
url = "git@example.com:frontend.git"
"#,
    );

    ctx.cli()
        .arg("--config")
        .arg(config)
        .arg("ls")
        .assert()
        .success()
        .stdout(predicate::str::contains("dotfiles"))
        .stdout(predicate::str::contains("personal/dotfiles"))
        .stdout(predicate::str::contains("frontend"))
        .stdout(predicate::str::contains("work/frontend"));
}

#[test]
fn list_json_outputs_machine_readable_repository_definitions() {
    let ctx = TestContext::new();
    let config = ctx.write_config(
        r#"
version = 1

[[repo]]
name = "frontend"
path = "frontend"
url = "git@example.com:frontend.git"
"#,
    );

    let output = ctx
        .cli()
        .arg("--config")
        .arg(config)
        .arg("list")
        .arg("--json")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let json: serde_json::Value = serde_json::from_slice(&output).expect("valid JSON output");

    assert_eq!(json["repositories"][0]["name"], "frontend");
    assert!(json["repositories"][0]["path"].as_str().unwrap().ends_with("frontend"));
    assert_eq!(json["repositories"][0]["url"], "git@example.com:frontend.git");
    assert!(json["repositories"][0]["source_config"].as_str().unwrap().ends_with("grove.toml"));
}
