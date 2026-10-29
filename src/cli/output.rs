use std::fmt::Arguments;
use std::io::{self, Write};

use anstream::AutoStream;
use anstream::ColorChoice;
use anstream::adapter::strip_str;

pub(in crate::cli) struct Output<'a> {
    stdout: &'a mut dyn Write,
    stderr: &'a mut dyn Write,
    stdout_choice: ColorChoice,
    stderr_choice: ColorChoice,
    report: ReportStream,
}

/// Where report lines go. The commands that act on repositories report on
/// stderr beside their progress; `gv replace` reports on stdout, leaving stderr
/// to errors, so a provisioning caller captures the two apart.
#[derive(Clone, Copy)]
enum ReportStream {
    Stdout,
    Stderr,
}

impl<'a> Output<'a> {
    pub(in crate::cli) fn terminal(stdout: &'a mut dyn Write, stderr: &'a mut dyn Write) -> Self {
        // anstream resolves the color decision for each real stream, honoring
        // the standard color environment variables, the global choice, and
        // terminal detection, so grove does not reimplement that stack.
        Self {
            stdout,
            stderr,
            stdout_choice: AutoStream::choice(&io::stdout()),
            stderr_choice: AutoStream::choice(&io::stderr()),
            report: ReportStream::Stderr,
        }
    }

    #[cfg(test)]
    pub(in crate::cli) fn captured(stdout: &'a mut dyn Write, stderr: &'a mut dyn Write) -> Self {
        Self {
            stdout,
            stderr,
            stdout_choice: ColorChoice::Never,
            stderr_choice: ColorChoice::Never,
            report: ReportStream::Stderr,
        }
    }

    /// The same sink with its report lines on stdout.
    pub(in crate::cli) fn reporting_on_stdout(&mut self) -> Output<'_> {
        Output {
            stdout: &mut *self.stdout,
            stderr: &mut *self.stderr,
            stdout_choice: self.stdout_choice,
            stderr_choice: self.stderr_choice,
            report: ReportStream::Stdout,
        }
    }

    pub(in crate::cli) fn stdout(&mut self, arguments: Arguments<'_>) -> io::Result<()> {
        write_adapted(self.stdout, self.stdout_choice, arguments)
    }

    pub(in crate::cli) fn stderr(&mut self, arguments: Arguments<'_>) -> io::Result<()> {
        write_adapted(self.stderr, self.stderr_choice, arguments)
    }

    pub(in crate::cli) fn report(&mut self, arguments: Arguments<'_>) -> io::Result<()> {
        match self.report {
            ReportStream::Stdout => self.stdout(arguments),
            ReportStream::Stderr => self.stderr(arguments),
        }
    }
}

fn write_adapted(
    writer: &mut dyn Write,
    choice: ColorChoice,
    arguments: Arguments<'_>,
) -> io::Result<()> {
    if matches!(choice, ColorChoice::Always | ColorChoice::AlwaysAnsi) {
        writer.write_fmt(arguments)
    } else {
        let rendered = arguments.to_string();
        writer.write_all(strip_str(&rendered).to_string().as_bytes())
    }
}

pub(in crate::cli) fn terminal_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        if character.is_control() {
            escaped.extend(character.escape_default());
        } else {
            escaped.push(character);
        }
    }
    escaped
}

pub(in crate::cli) fn terminal_multiline_text(value: &str) -> String {
    value
        .split_inclusive('\n')
        .map(|line| {
            line.strip_suffix('\n')
                .map_or_else(|| terminal_text(line), |line| format!("{}\n", terminal_text(line)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::{Output, terminal_text};

    struct FailingWriter;

    impl io::Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(io::ErrorKind::BrokenPipe, "closed output"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn propagates_writer_failure() {
        let mut stdout = FailingWriter;
        let mut stderr = Vec::new();
        let mut output = Output::captured(&mut stdout, &mut stderr);

        let error = output.stdout(format_args!("hello\n")).unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
    }

    #[test]
    fn escapes_terminal_control_characters() {
        assert_eq!(terminal_text("a\n\t\u{1b}[31m"), "a\\n\\t\\u{1b}[31m");
    }
}
