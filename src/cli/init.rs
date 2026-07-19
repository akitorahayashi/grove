use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use clap::Args;

use crate::AppError;

const CONFIG_TEMPLATE: &str = include_str!("../assets/grove.toml.tpl");

#[derive(Args)]
pub(super) struct InitCommand;

pub(super) fn run(config: Option<PathBuf>, _command: InitCommand) -> Result<(), AppError> {
    if config.is_some() {
        return Err(AppError::config_error("--config cannot be used with init"));
    }

    let path = std::env::current_dir()?.join("grove.toml");
    let file = OpenOptions::new().write(true).create_new(true).open(&path)?;
    write_template(file, &path, CONFIG_TEMPLATE.as_bytes())?;

    println!("created {}", path.display());

    Ok(())
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
        Err(remove_error) => Err(AppError::config_error(format!(
            "failed to write {}: {}; also failed to remove the partial file: {}",
            path.display(),
            write_error,
            remove_error
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::io;

    use tempfile::TempDir;

    use super::write_template;

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
    fn write_template_removes_partial_file_when_write_fails() {
        let directory = TempDir::new().expect("failed to create temp directory");
        let path = directory.path().join("grove.toml");
        fs::write(&path, "").expect("failed to create partial file");

        let error =
            write_template(FailingWriter, &path, b"version = 1\n").expect_err("write should fail");

        assert_eq!(error.kind(), io::ErrorKind::WriteZero);
        assert!(!path.exists());
    }
}
