/// Result of a fast-forward update for a local default branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitUpdate {
    before: String,
    after: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitUpdateBlock {
    DetachedHead,
    DirtyWorkingTree,
    MissingLocalBranch,
    MissingRemoteBranch,
    Diverged,
    AheadOfOrigin,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Restoration {
    NotNeeded,
    Restored,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitUpdateOutcome {
    Completed { update: GitUpdate, restoration: Restoration },
    Blocked(GitUpdateBlock),
    Failed { primary: String, restoration: Restoration },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitRefreshOutcome {
    Completed { update: GitUpdate, previous_branch: Option<String> },
    Blocked(GitUpdateBlock),
    Failed { message: String, previous_branch: Option<String> },
}

impl GitUpdate {
    pub fn new(before: String, after: String) -> Self {
        Self { before, after }
    }

    pub fn before(&self) -> &str {
        &self.before
    }

    pub fn after(&self) -> &str {
        &self.after
    }

    pub fn changed(&self) -> bool {
        self.before != self.after
    }
}
