//! Database module for StellaSearch
//!
//! Handles SQLite database operations including FTS5 full-text search.

mod schema;
mod queries;

pub use schema::Database;
pub use queries::*;
