//! Domain model for repositories managed by grove.

mod definition;
mod name;
mod selection;
mod state;

pub use definition::RepositoryDefinition;
pub use name::RepositoryName;
pub use selection::select_repositories;
pub use state::BranchTracking;
