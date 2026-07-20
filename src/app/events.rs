//! Phase-generic progress events shared by the parallel repository use cases.

use std::sync::mpsc::Sender;
use std::time::Duration;

use crate::AppError;
use crate::git::{GitProgress, GitProgressSink};
use crate::repositories::RepositoryDefinition;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PhaseSummary {
    count: usize,
    elapsed: Duration,
}

impl PhaseSummary {
    pub(crate) fn new(count: usize, elapsed: Duration) -> Self {
        Self { count, elapsed }
    }

    pub fn count(self) -> usize {
        self.count
    }

    pub fn elapsed(self) -> Duration {
        self.elapsed
    }
}

#[derive(Debug)]
pub(crate) enum Event<P> {
    PhaseStarted { phase: P, total: usize },
    RepositoryStarted { repository: String, phase: P },
    GitProgress { repository: String, progress: GitProgress },
    RepositoryFinished { repository: String, phase: P },
    PhaseCompleted { phase: P, summary: PhaseSummary },
    PhaseFailed { phase: P },
}

pub(crate) trait EventSink<P>: Sync {
    fn emit(&self, event: Event<P>) -> Result<(), AppError>;
}

impl<P: Send> EventSink<P> for Sender<Event<P>> {
    fn emit(&self, event: Event<P>) -> Result<(), AppError> {
        self.send(event).map_err(|_| AppError::internal("event receiver disconnected"))
    }
}

#[derive(Debug, Default)]
pub(crate) struct DiscardEvents;

impl<P> EventSink<P> for DiscardEvents {
    fn emit(&self, _event: Event<P>) -> Result<(), AppError> {
        Ok(())
    }
}

pub(crate) struct EventProgress<'a, P> {
    repository: &'a RepositoryDefinition,
    events: &'a dyn EventSink<P>,
}

impl<'a, P> EventProgress<'a, P> {
    pub(crate) fn new(repository: &'a RepositoryDefinition, events: &'a dyn EventSink<P>) -> Self {
        Self { repository, events }
    }
}

impl<P> GitProgressSink for EventProgress<'_, P> {
    fn progress(&mut self, progress: GitProgress) -> Result<(), AppError> {
        self.events.emit(Event::GitProgress {
            repository: self.repository.display_path().to_string(),
            progress,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::{Event, EventSink};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum TestPhase {
        Checking,
    }

    #[test]
    fn disconnected_event_receiver_is_an_application_error() {
        let (sender, receiver) = mpsc::channel::<Event<TestPhase>>();
        drop(receiver);

        let result = sender.emit(Event::PhaseStarted { phase: TestPhase::Checking, total: 1 });

        assert!(result.is_err_and(|error| error.to_string().contains("receiver disconnected")));
    }
}
