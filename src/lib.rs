//! Aster Package Manager.

pub mod catalogue;
pub mod cli;
pub mod config;
pub mod error;
pub mod installer;
pub mod registry;
pub mod resolver;
pub mod schema;
pub mod util;
pub mod version;

pub const VERSION: &str = "0.2.0";
