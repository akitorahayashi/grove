use std::cell::OnceCell;

use crate::AppError;
use crate::cache::Store;
use crate::git::GitClient;
use crate::zoxide::{CommandZoxideClient, ZoxideClient};

/// Application context holding grove's external boundaries: the Git and zoxide
/// command clients and the local clone cache.
///
/// The cache is resolved from the environment lazily, on first access, so that
/// operations which never place or seed clones (`status`, `refresh`) do not
/// require `XDG_CACHE_HOME` or `HOME` to be set.
pub struct AppContext<G: GitClient, Z: ZoxideClient = CommandZoxideClient> {
    git: G,
    zoxide: Z,
    cache: OnceCell<Store>,
}

impl<G: GitClient> AppContext<G, CommandZoxideClient> {
    pub fn new(git: G) -> Self {
        Self { git, zoxide: CommandZoxideClient, cache: OnceCell::new() }
    }
}

impl<G: GitClient, Z: ZoxideClient> AppContext<G, Z> {
    pub fn git(&self) -> &G {
        &self.git
    }

    pub fn zoxide(&self) -> &Z {
        &self.zoxide
    }

    /// The local clone cache, resolved from the environment on first use. Only
    /// operations that place or seed clones reach for it, so the cache
    /// environment is required by those operations alone.
    pub(crate) fn cache(&self) -> Result<&Store, AppError> {
        if let Some(store) = self.cache.get() {
            return Ok(store);
        }
        let store = Store::from_env()?;
        Ok(self.cache.get_or_init(|| store))
    }
}
