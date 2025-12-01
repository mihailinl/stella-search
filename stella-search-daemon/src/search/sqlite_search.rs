//! SQLite-based search backend
//!
//! Wraps the existing Database search functionality to implement SearchBackend trait.
//! This is the fallback backend when Windows Search is unavailable.

use std::sync::Arc;

use crate::database::Database;
use super::{SearchBackend, SearchError, SearchQuery, SearchResult};

/// SQLite search backend using the existing database infrastructure
pub struct SqliteSearchBackend {
    db: Arc<Database>,
}

impl SqliteSearchBackend {
    /// Create a new SQLite search backend
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// Get the underlying database reference
    pub fn database(&self) -> &Arc<Database> {
        &self.db
    }
}

impl SearchBackend for SqliteSearchBackend {
    fn is_available(&self) -> bool {
        // SQLite is always available if we have a database connection
        true
    }

    fn search(&self, query: &SearchQuery) -> Result<SearchResult, SearchError> {
        let start = std::time::Instant::now();

        // Use existing database search
        let results = self.db.search(
            &query.query,
            query.max_results,
            query.extension.as_deref(),
        )?;

        Ok(SearchResult {
            files: results.files,
            total_found: results.total_found,
            query_time_ms: start.elapsed().as_millis() as u64,
            backend_name: self.name().to_string(),
        })
    }

    fn name(&self) -> &'static str {
        "SQLite"
    }

    fn status_description(&self) -> String {
        match self.db.get_stats() {
            Ok(stats) => format!(
                "SQLite ({} files, {} dirs indexed)",
                stats.indexed_files, stats.indexed_dirs
            ),
            Err(_) => "SQLite (status unavailable)".to_string(),
        }
    }
}
