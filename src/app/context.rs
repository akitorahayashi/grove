use crate::cache::Store;
use crate::git::GitClient;
use crate::zoxide::{CommandZoxideClient, ZoxideClient};

/// Application context holding grove's external boundaries: the Git and zoxide
/// command clients and the local clone cache.
pub struct AppContext<G: GitClient, Z: ZoxideClient = CommandZoxideClient> {
    git: G,
    zoxide: Z,
    cache: Store,
}

impl<G: GitClient> AppContext<G, CommandZoxideClient> {
    pub fn new(git: G, cache: Store) -> Self {
        Self { git, zoxide: CommandZoxideClient, cache }
    }
}

impl<G: GitClient, Z: ZoxideClient> AppContext<G, Z> {
    pub fn git(&self) -> &G {
        &self.git
    }

    pub fn zoxide(&self) -> &Z {
        &self.zoxide
    }

    pub(crate) fn cache(&self) -> &Store {
        &self.cache
    }
}
