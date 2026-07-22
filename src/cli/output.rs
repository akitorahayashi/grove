use std::fmt::Arguments;
use std::io::{self, IsTerminal, Write};

use anstream::ColorChoice;

pub(in crate::cli) struct Output<'a> {
    stdout: &'a mut dyn Write,
    stderr: &'a mut dyn Write,
    stdout_is_terminal: bool,
    stderr_is_terminal: bool,
}

impl<'a> Output<'a> {
    pub(in crate::cli) fn terminal(stdout: &'a mut dyn Write, stderr: &'a mut dyn Write) -> Self {
        Self {
            stdout,
            stderr,
            stdout_is_terminal: io::stdout().is_terminal(),
            stderr_is_terminal: io::stderr().is_terminal(),
        }
    }

    #[cfg(test)]
    pub(in crate::cli) fn captured(stdout: &'a mut dyn Write, stderr: &'a mut dyn Write) -> Self {
        Self { stdout, stderr, stdout_is_terminal: false, stderr_is_terminal: false }
    }

    pub(in crate::cli) fn stdout_is_terminal(&self) -> bool {
        self.stdout_is_terminal
    }

    pub(in crate::cli) fn stdout(&mut self, arguments: Arguments<'_>) -> io::Result<()> {
        write_adapted(self.stdout, self.stdout_is_terminal, arguments)
    }

    pub(in crate::cli) fn stderr(&mut self, arguments: Arguments<'_>) -> io::Result<()> {
        write_adapted(self.stderr, self.stderr_is_terminal, arguments)
    }
}

fn write_adapted(
    writer: &mut dyn Write,
    is_terminal: bool,
    arguments: Arguments<'_>,
) -> io::Result<()> {
    let color = if std::env::var_os("NO_COLOR").is_some() {
        false
    } else if std::env::var_os("CLICOLOR_FORCE").is_some_and(|value| value != "0") {
        true
    } else if std::env::var_os("CLICOLOR").is_some_and(|value| value == "0") {
        false
    } else {
        match ColorChoice::global() {
            ColorChoice::Always | ColorChoice::AlwaysAnsi => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => is_terminal,
        }
    };
    if color {
        writer.write_fmt(arguments)
    } else {
        let rendered = arguments.to_string();
        writer.write_all(strip_ansi(&rendered).as_bytes())
    }
}

fn strip_ansi(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == 0x1b && bytes.get(index + 1) == Some(&b'[') {
            index += 2;
            while index < bytes.len() {
                let byte = bytes[index];
                index += 1;
                if (0x40..=0x7e).contains(&byte) {
                    break;
                }
            }
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(output).expect("removing ASCII escape sequences preserves UTF-8")
}

pub(in crate::cli) fn terminal_text(value: &str) -> String {
    value
        .chars()
        .flat_map(|character| {
            if character.is_control() {
                character.escape_default().collect::<Vec<_>>()
            } else {
                vec![character]
            }
        })
        .collect()
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
