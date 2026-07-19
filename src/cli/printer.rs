use std::fmt;

use anstream::eprint;
use indicatif::ProgressDrawTarget;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Printer {
    Default,
}

impl Printer {
    pub(super) fn target(self) -> ProgressDrawTarget {
        match self {
            Self::Default => ProgressDrawTarget::stderr(),
        }
    }

    pub(super) fn stderr(self) -> Stderr {
        match self {
            Self::Default => Stderr::Enabled,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stderr {
    Enabled,
}

impl fmt::Write for Stderr {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        match self {
            Self::Enabled => eprint!("{s}"),
        }
        Ok(())
    }
}
