use assert_cmd::Command;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use tempfile::TempDir;

pub struct TestContext {
    root: TempDir,
    workspace: PathBuf,
}

impl TestContext {
    pub fn new() -> Self {
        let root = TempDir::new().expect("failed to create temp directory");
        let workspace = root.path().join("workspace");
        fs::create_dir_all(&workspace).expect("failed to create workspace");
        Self { root, workspace }
    }

    pub fn workspace(&self) -> &Path {
        &self.workspace
    }

    pub fn root(&self) -> &Path {
        self.root.path()
    }

    pub fn config_path(&self) -> PathBuf {
        self.workspace.join("grove.toml")
    }

    pub fn write_config(&self, contents: &str) -> PathBuf {
        let path = self.config_path();
        fs::write(&path, contents).expect("failed to write grove.toml");
        path
    }

    pub fn write_config_at(&self, relative_path: &str, contents: &str) -> PathBuf {
        let path = self.workspace.join(relative_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("failed to create config directory");
        }
        fs::write(&path, contents).expect("failed to write config");
        path
    }

    pub fn cli(&self) -> Command {
        let mut command = Command::cargo_bin("gv").expect("failed to locate gv binary");
        command.current_dir(&self.workspace);
        command.env("XDG_CACHE_HOME", self.cache_home());
        command
    }

    pub fn cache_home(&self) -> PathBuf {
        self.root().join("cache-home")
    }

    pub fn cache_root(&self) -> PathBuf {
        self.cache_home().join("grove")
    }

    pub fn create_remote(&self, name: &str) -> RemoteRepository {
        let seed = self.root().join("seeds").join(name);
        let remote = self.root().join("remotes").join(format!("{name}.git"));
        fs::create_dir_all(seed.parent().unwrap()).expect("failed to create seed parent");
        fs::create_dir_all(remote.parent().unwrap()).expect("failed to create remote parent");

        run_git(
            self.root(),
            &["init", "--bare", "--initial-branch=main", remote.to_str().unwrap()],
        );
        fs::create_dir_all(&seed).expect("failed to create seed repository");
        run_git(&seed, &["init", "-b", "main"]);
        fs::write(seed.join("README.md"), "initial\n").expect("failed to write initial file");
        run_git(&seed, &["add", "README.md"]);
        run_git(
            &seed,
            &[
                "-c",
                "user.name=Grove Test",
                "-c",
                "user.email=grove@example.com",
                "commit",
                "-m",
                "initial",
            ],
        );
        run_git(&seed, &["remote", "add", "origin", remote.to_str().unwrap()]);
        run_git(&seed, &["push", "-u", "origin", "main"]);

        RemoteRepository { seed, remote }
    }
}

pub struct RemoteRepository {
    seed: PathBuf,
    remote: PathBuf,
}

impl RemoteRepository {
    pub fn url(&self) -> String {
        self.remote.display().to_string()
    }

    pub fn add_commit(&self, file_name: &str, contents: &str) {
        fs::write(self.seed.join(file_name), contents).expect("failed to write seed file");
        run_git(&self.seed, &["add", file_name]);
        run_git(
            &self.seed,
            &[
                "-c",
                "user.name=Grove Test",
                "-c",
                "user.email=grove@example.com",
                "commit",
                "-m",
                file_name,
            ],
        );
        run_git(&self.seed, &["push", "origin", "main"]);
    }
}

pub fn run_git(directory: &Path, args: &[&str]) {
    let output = ProcessCommand::new("git")
        .current_dir(directory)
        .args(args)
        .output()
        .expect("failed to run git");
    assert!(
        output.status.success(),
        "git {} failed\nstdout:\n{}\nstderr:\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
