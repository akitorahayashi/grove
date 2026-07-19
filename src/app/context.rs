use crate::git::{CommandGitClient, GitClient};

/// Application context holding external command boundaries.
pub struct AppContext<G: GitClient> {
    git: G,
}

impl<G: GitClient> AppContext<G> {
    pub fn new(git: G) -> Self {
        Self { git }
    }

    pub fn git(&self) -> &G {
        &self.git
    }
}

impl Default for AppContext<CommandGitClient> {
    fn default() -> Self {
        Self::new(CommandGitClient)
    }
}
