//! Domain model for repositories managed by grove.

mod branch_name;
mod definition;
mod name;
mod path;
mod selection;
mod url;

pub use branch_name::BranchName;
pub use definition::RepositoryDefinition;
pub use name::RepositoryName;
pub(crate) use path::{ResolutionError, resolve_operational_path};
pub use selection::select_repositories;
pub use url::RemoteUrl;
pub(crate) use url::redact_urls_for_display;
