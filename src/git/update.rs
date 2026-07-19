/// Result of a fast-forward update for a local default branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitUpdate {
    before: String,
    after: String,
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
