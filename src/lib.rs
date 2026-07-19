//! Library entry point exposing the core command handlers.

pub(crate) mod app;
mod cli;
mod error;
pub mod items;
pub mod labels;

pub use app::api::{
    item_add, item_delete, item_list, label_add, label_delete, label_list, labeling_attach,
    labeling_detach, labeling_find, labeling_list,
};
pub use cli::run as cli;
pub use error::AppError;
