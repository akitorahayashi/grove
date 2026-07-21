//! Report scaffolding shared by the sync and refresh use cases. The outcome
//! vocabularies differ per use case, so the entry is generic over them.

use crate::repositories::RepositoryDefinition;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BlockedReasonDetails {
    RemoteUrlMismatch { actual: String, expected: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry<O> {
    repository: String,
    outcome: O,
    blocked_details: Option<BlockedReasonDetails>,
}

impl<O> Entry<O> {
    pub(crate) fn new(repository: &RepositoryDefinition, outcome: O) -> Self {
        Self { repository: repository.display_path().to_string(), outcome, blocked_details: None }
    }

    pub(crate) fn blocked_with_details(
        repository: &RepositoryDefinition,
        outcome: O,
        blocked_details: BlockedReasonDetails,
    ) -> Self {
        Self {
            repository: repository.display_path().to_string(),
            outcome,
            blocked_details: Some(blocked_details),
        }
    }

    pub fn repository(&self) -> &str {
        &self.repository
    }

    pub fn outcome(&self) -> &O {
        &self.outcome
    }

    pub(crate) fn blocked_details(&self) -> Option<&BlockedReasonDetails> {
        self.blocked_details.as_ref()
    }
}
