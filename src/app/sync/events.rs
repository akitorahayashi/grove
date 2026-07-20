use std::sync::mpsc::Sender;

use crate::AppError;
use crate::git::GitProgress;

use super::PhaseSummary;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Checking,
    Preparing,
    Updating,
}

impl Phase {
    pub fn message(self) -> &'static str {
        match self {
            Self::Checking => "Checking repositories...",
            Self::Preparing => "Preparing repositories...",
            Self::Updating => "Updating repositories...",
        }
    }
}

#[derive(Debug)]
pub(crate) enum Event {
    PhaseStarted { phase: Phase, total: usize },
    RepositoryStarted { repository: String, phase: Phase },
    GitProgress { repository: String, progress: GitProgress },
    RepositoryFinished { repository: String, phase: Phase },
    PhaseCompleted { phase: Phase, summary: PhaseSummary },
    PhaseFailed { phase: Phase },
}

pub(crate) trait EventSink: Sync {
    fn emit(&self, event: Event) -> Result<(), AppError>;
}

impl EventSink for Sender<Event> {
    fn emit(&self, event: Event) -> Result<(), AppError> {
        self.send(event).map_err(|_| AppError::internal("sync event receiver disconnected"))
    }
}

#[derive(Debug, Default)]
pub(super) struct DiscardEvents;

impl EventSink for DiscardEvents {
    fn emit(&self, _event: Event) -> Result<(), AppError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::{Event, EventSink, Phase};

    #[test]
    fn disconnected_event_receiver_is_an_application_error() {
        let (sender, receiver) = mpsc::channel();
        drop(receiver);

        let result = sender.emit(Event::PhaseStarted { phase: Phase::Checking, total: 1 });

        assert!(result.is_err_and(|error| error.to_string().contains("receiver disconnected")));
    }
}
