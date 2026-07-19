//! Library entry point for the grove CLI.

pub mod app;
mod cli;
pub mod config;
mod error;
pub mod git;
pub mod repositories;

pub use app::api::{status, sync};
pub use cli::run as cli;
pub use error::AppError;
