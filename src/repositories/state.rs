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
