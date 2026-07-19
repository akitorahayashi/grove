/// Local working tree condition for a managed repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepositoryCondition {
    Missing,
    Invalid(String),
    Clean,
    Dirty,
    RemoteMismatch,
}

impl RepositoryCondition {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Missing => "missing",
            Self::Invalid(_) => "invalid",
            Self::Clean => "clean",
            Self::Dirty => "dirty",
            Self::RemoteMismatch => "remote-mismatch",
        }
    }
}

/// Ahead/behind information for a local branch compared with its remote branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchTracking {
    branch: String,
    ahead: u32,
    behind: u32,
}

impl BranchTracking {
    pub fn new(branch: String, ahead: u32, behind: u32) -> Self {
        Self { branch, ahead, behind }
    }

    pub fn branch(&self) -> &str {
        &self.branch
    }

    pub fn ahead(&self) -> u32 {
        self.ahead
    }

    pub fn behind(&self) -> u32 {
        self.behind
    }
}

/// Status data owned by the repository domain, independent of terminal formatting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryState {
    branch: Option<String>,
    condition: RepositoryCondition,
    default_branch: Option<BranchTracking>,
}

impl RepositoryState {
    pub fn new(
        branch: Option<String>,
        condition: RepositoryCondition,
        default_branch: Option<BranchTracking>,
    ) -> Self {
        Self { branch, condition, default_branch }
    }

    pub fn branch(&self) -> Option<&str> {
        self.branch.as_deref()
    }

    pub fn condition(&self) -> &RepositoryCondition {
        &self.condition
    }

    pub fn default_branch(&self) -> Option<&BranchTracking> {
        self.default_branch.as_ref()
    }
}
