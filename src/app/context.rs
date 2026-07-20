use crate::git::{CommandGitClient, GitClient};
use crate::zoxide::{CommandZoxideClient, ZoxideClient};

/// Application context holding external command boundaries.
pub struct AppContext<G: GitClient, Z: ZoxideClient = CommandZoxideClient> {
    git: G,
    zoxide: Z,
}

impl<G: GitClient> AppContext<G, CommandZoxideClient> {
    pub fn new(git: G) -> Self {
        Self { git, zoxide: CommandZoxideClient }
    }
}

impl<G: GitClient, Z: ZoxideClient> AppContext<G, Z> {
    pub fn git(&self) -> &G {
        &self.git
    }

    pub fn zoxide(&self) -> &Z {
        &self.zoxide
    }
}

impl Default for AppContext<CommandGitClient, CommandZoxideClient> {
    fn default() -> Self {
        Self::new(CommandGitClient::default())
    }
}
