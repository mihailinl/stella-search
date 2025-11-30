//! Search backend abstraction layer
//!
//! Provides a unified interface for different search backends:
//! - Windows Search (primary on Windows when available)
//! - SQLite (fallback or primary on non-Windows platforms)

pub mod sqlite_search;
#[cfg(windows)]
pub mod windows_search;
pub mod manager;

// Re-export main types
pub use manager::SearchManager;

use crate::database::IndexedFile;
use thiserror::Error;

/// Search backend errors
#[derive(Error, Debug)]
pub enum SearchError {
    #[error("Search backend is not available")]
    NotAvailable,

    #[error("Query failed: {0}")]
    QueryFailed(String),

    #[error("Database error: {0}")]
    DatabaseError(#[from] anyhow::Error),
}

/// Search query parameters
#[derive(Debug, Clone)]
pub struct SearchQuery {
    /// The search term (filename pattern)
    pub query: String,
    /// Maximum number of results to return
    pub max_results: usize,
    /// Optional extension filter (e.g., ".pdf", ".exe")
    pub extension: Option<String>,
    /// Optional directory filter
    pub directories: Option<Vec<String>>,
}

impl SearchQuery {
    pub fn new(query: impl Into<String>, max_results: usize) -> Self {
        Self {
            query: query.into(),
            max_results,
            extension: None,
            directories: None,
        }
    }

    pub fn with_extension(mut self, ext: impl Into<String>) -> Self {
        self.extension = Some(ext.into());
        self
    }

    pub fn with_directories(mut self, dirs: Vec<String>) -> Self {
        self.directories = Some(dirs);
        self
    }
}

/// Search results with timing information
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// The matching files
    pub files: Vec<IndexedFile>,
    /// Total number of matches found
    pub total_found: usize,
    /// Time taken in milliseconds
    pub query_time_ms: u64,
    /// Which backend produced these results
    pub backend_name: String,
}

/// Trait for search backends
///
/// Backends must be Send + Sync for use across async tasks.
/// The search method is synchronous - blocking is acceptable since
/// search operations are typically fast.
pub trait SearchBackend: Send + Sync {
    /// Check if this backend is currently available
    fn is_available(&self) -> bool;

    /// Perform a search query
    fn search(&self, query: &SearchQuery) -> Result<SearchResult, SearchError>;

    /// Get the name of this backend for logging/status
    fn name(&self) -> &'static str;

    /// Get a description of this backend's status
    fn status_description(&self) -> String {
        if self.is_available() {
            format!("{} (available)", self.name())
        } else {
            format!("{} (unavailable)", self.name())
        }
    }
}
