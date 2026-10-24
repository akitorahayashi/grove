use std::cell::OnceCell;

use crate::AppError;
use crate::cache::Store;
use crate::trash::SystemTrash;
use crate::zoxide::CommandZoxideClient;

/// Application context holding grove's external boundaries: the Git and zoxide
/// command clients, the system trash, and the local clone cache.
///
/// The cache is resolved from the environment lazily, on first access, so that
/// operations which never place or seed clones (`status`, `refresh`) do not
/// require `XDG_CACHE_HOME` or `HOME` to be set.
pub struct AppContext<G, Z = CommandZoxideClient, T = SystemTrash> {
    git: G,
    zoxide: Z,
    trash: T,
    cache: OnceCell<Store>,
}

impl<G> AppContext<G, CommandZoxideClient, SystemTrash> {
    pub fn new(git: G) -> Self {
        Self { git, zoxide: CommandZoxideClient, trash: SystemTrash, cache: OnceCell::new() }
    }
}

impl<G, Z, T> AppContext<G, Z, T> {
    #[cfg(test)]
    pub(crate) fn with_trash<U>(self, trash: U) -> AppContext<G, Z, U> {
        AppContext { git: self.git, zoxide: self.zoxide, trash, cache: self.cache }
    }

    #[cfg(test)]
    pub(crate) fn with_cache(self, store: Store) -> Self {
        let cache = OnceCell::new();
        let _ = cache.set(store);
        Self { cache, ..self }
    }

    pub fn git(&self) -> &G {
        &self.git
    }

    pub fn zoxide(&self) -> &Z {
        &self.zoxide
    }

    pub fn trash(&self) -> &T {
        &self.trash
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
