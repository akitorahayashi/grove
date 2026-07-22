//! Phase-structured bounded-parallel execution for the repository use cases.
//!
//! A use case runs as a sequence of phases. Each phase fans its selected work
//! across bounded parallel workers, serializes work that shares a Git resource,
//! and emits phase-generic progress events. This module owns that mechanism;
//! the use cases own the policy of which phases run and what each does.

pub(crate) mod workers;

mod events;
mod run;

pub use events::Summary;
pub(crate) use events::{DiscardEvents, Event, EventProgress, EventSink};
pub(crate) use run::{Task, run_check, run_workers};
