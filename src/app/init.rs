use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::AppError;

const CONFIG_TEMPLATE: &str = include_str!("../assets/grove.toml.tpl");

#[derive(Debug, Clone)]
pub struct Report {
    created_path: PathBuf,
}

impl Report {
    fn new(created_path: PathBuf) -> Self {
        Self { created_path }
    }

    pub fn created_path(&self) -> &Path {
        &self.created_path
    }
}

pub fn execute(directory: &Path) -> Result<Report, AppError> {
    let path = directory.join("grove.toml");
    let file = OpenOptions::new().write(true).create_new(true).open(&path)?;
    write_template(file, &path, CONFIG_TEMPLATE.as_bytes())?;

    Ok(Report::new(path))
}

fn write_template(mut writer: impl Write, path: &Path, template: &[u8]) -> Result<(), AppError> {
    if let Err(write_error) = writer.write_all(template) {
        drop(writer);
        remove_partial_file(path, &write_error)?;
        return Err(write_error.into());
    }

    Ok(())
}

fn remove_partial_file(path: &Path, write_error: &std::io::Error) -> Result<(), AppError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(remove_error) if remove_error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(remove_error) => Err(AppError::from(std::io::Error::other(format!(
            "failed to write {}: {}; also failed to remove the partial file: {}",
            path.display(),
            write_error,
            remove_error
        )))),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;

    use tempfile::TempDir;

    use super::{execute, write_template};

    struct FailingWriter;

    impl io::Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::WriteZero, "simulated write failure"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn init_creates_new_config() {
        let directory = TempDir::new().expect("failed to create temp directory");

        let report = execute(directory.path()).expect("init should create grove.toml");

        assert_eq!(report.created_path(), directory.path().join("grove.toml"));
        let contents =
            fs::read_to_string(report.created_path()).expect("failed to read created config");
        assert!(contents.contains("version = 1"));
    }

    #[test]
    fn init_refuses_existing_config() {
        let directory = TempDir::new().expect("failed to create temp directory");
        let path = directory.path().join("grove.toml");
        fs::write(&path, "existing\n").expect("failed to create existing config");

        let error = execute(directory.path()).expect_err("init should refuse existing config");

        assert_eq!(error.io_error().map(io::Error::kind), Some(io::ErrorKind::AlreadyExists));
        assert_eq!(fs::read_to_string(path).expect("failed to read config"), "existing\n");
    }

    #[test]
    fn write_template_removes_partial_file_when_write_fails() {
        let directory = TempDir::new().expect("failed to create temp directory");
        let path = directory.path().join("grove.toml");
        fs::write(&path, "").expect("failed to create partial file");

        let error =
            write_template(FailingWriter, &path, b"version = 1\n").expect_err("write should fail");

        assert_eq!(error.io_error().map(io::Error::kind), Some(io::ErrorKind::WriteZero));
        assert!(!path.exists());
    }

    #[test]
    fn write_template_reports_cleanup_failure_context() {
        let directory = TempDir::new().expect("failed to create temp directory");
        let path = directory.path().join("grove.toml");
        fs::create_dir(&path).expect("failed to create cleanup-blocking directory");

        let error =
            write_template(FailingWriter, &path, b"version = 1\n").expect_err("write should fail");

        assert!(error.to_string().contains("also failed to remove the partial file"));
        assert!(path.is_dir());
    }
}
