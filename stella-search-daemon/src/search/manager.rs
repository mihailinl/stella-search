//! Search manager for SQLite backend
//!
//! The daemon only provides SQLite search.
//! Windows Search is handled by the native DLL (stella-search-native).

use std::sync::Arc;

use tracing::{debug, info, warn};

use crate::config::SearchBackendType;
use crate::database::Database;
use super::{SearchBackend, SearchError, SearchQuery, SearchResult};
use super::sqlite_search::SqliteSearchBackend;

/// Search manager that handles SQLite search backend
pub struct SearchManager {
    /// SQLite search backend
    backend: SqliteSearchBackend,
    /// Reference to database
    db: Arc<Database>,
}

impl SearchManager {
    /// Create a new search manager
    pub fn new(_backend_type: SearchBackendType, db: Arc<Database>) -> Self {
        info!("Using SQLite as search backend (daemon mode)");
        let backend = SqliteSearchBackend::new(db.clone());

        Self { backend, db }
    }

    /// Perform a search
    pub fn search(&self, query: &SearchQuery) -> SearchResult {
        match self.backend.search(query) {
            Ok(result) => {
                debug!(
                    "SQLite search returned {} results in {}ms",
                    result.total_found, result.query_time_ms
                );
                result
            }
            Err(e) => {
                warn!("SQLite search failed: {}", e);
                SearchResult {
                    files: Vec::new(),
                    total_found: 0,
                    query_time_ms: 0,
                    backend_name: "SQLite".to_string(),
                }
            }
        }
    }

    /// Get the name of the active backend
    pub fn active_backend_name(&self) -> &'static str {
        "SQLite"
    }

    /// Check if indexing is needed (always true for SQLite daemon)
    pub fn needs_indexing(&self) -> bool {
        true
    }

    /// Get reference to the database
    pub fn database(&self) -> &Arc<Database> {
        &self.db
    }

    /// Get status description
    pub fn status_description(&self) -> String {
        self.backend.status_description()
    }

    /// No-op for daemon (no fallback switching)
    pub fn refresh_availability(&self) {
        // No-op - daemon always uses SQLite
    }
}
