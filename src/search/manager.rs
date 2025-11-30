//! Search manager with automatic fallback
//!
//! Manages search backends and provides automatic fallback from Windows Search
//! to SQLite when the primary backend becomes unavailable.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tracing::{debug, info, warn};

use crate::config::SearchBackendType;
use crate::database::Database;
use super::{SearchBackend, SearchError, SearchQuery, SearchResult};
use super::sqlite_search::SqliteSearchBackend;

#[cfg(windows)]
use super::windows_search::WindowsSearchBackend;

/// Search manager that handles backend selection and fallback
pub struct SearchManager {
    /// Primary search backend
    primary: Box<dyn SearchBackend>,
    /// Fallback search backend (always SQLite)
    fallback: Option<Arc<SqliteSearchBackend>>,
    /// Whether we're currently using fallback
    using_fallback: AtomicBool,
    /// Configured backend type
    backend_type: SearchBackendType,
    /// Reference to database for lazy indexer startup
    db: Arc<Database>,
}

impl SearchManager {
    /// Create a new search manager based on configuration
    pub fn new(backend_type: SearchBackendType, db: Arc<Database>) -> Self {
        let (primary, fallback, using_fallback) = match backend_type {
            SearchBackendType::Auto => Self::create_auto_backends(db.clone()),
            SearchBackendType::Windows => Self::create_windows_backend(db.clone()),
            SearchBackendType::Sqlite => Self::create_sqlite_backend(db.clone()),
        };

        Self {
            primary,
            fallback,
            using_fallback: AtomicBool::new(using_fallback),
            backend_type,
            db,
        }
    }

    /// Create backends for "auto" mode (Windows Search primary, SQLite fallback)
    #[cfg(windows)]
    fn create_auto_backends(
        db: Arc<Database>,
    ) -> (Box<dyn SearchBackend>, Option<Arc<SqliteSearchBackend>>, bool) {
        let ws = WindowsSearchBackend::new();

        if ws.is_available() {
            info!("Using Windows Search as primary backend (auto mode)");
            let sqlite = Arc::new(SqliteSearchBackend::new(db));
            (Box::new(ws), Some(sqlite), false)
        } else {
            info!("Windows Search unavailable, using SQLite as primary (auto mode)");
            (Box::new(SqliteSearchBackend::new(db)), None, true)
        }
    }

    #[cfg(not(windows))]
    fn create_auto_backends(
        db: Arc<Database>,
    ) -> (Box<dyn SearchBackend>, Option<Arc<SqliteSearchBackend>>, bool) {
        info!("Using SQLite as primary backend (non-Windows platform)");
        (Box::new(SqliteSearchBackend::new(db)), None, false)
    }

    /// Create Windows Search backend (error if unavailable)
    #[cfg(windows)]
    fn create_windows_backend(
        db: Arc<Database>,
    ) -> (Box<dyn SearchBackend>, Option<Arc<SqliteSearchBackend>>, bool) {
        let ws = WindowsSearchBackend::new();

        if ws.is_available() {
            info!("Using Windows Search as primary backend (forced)");
            let sqlite = Arc::new(SqliteSearchBackend::new(db));
            (Box::new(ws), Some(sqlite), false)
        } else {
            warn!("Windows Search forced but unavailable, will fail on search");
            let sqlite = Arc::new(SqliteSearchBackend::new(db));
            (Box::new(ws), Some(sqlite), false)
        }
    }

    #[cfg(not(windows))]
    fn create_windows_backend(
        db: Arc<Database>,
    ) -> (Box<dyn SearchBackend>, Option<Arc<SqliteSearchBackend>>, bool) {
        warn!("Windows Search requested on non-Windows platform, using SQLite");
        (Box::new(SqliteSearchBackend::new(db)), None, true)
    }

    /// Create SQLite backend (no fallback needed)
    fn create_sqlite_backend(
        db: Arc<Database>,
    ) -> (Box<dyn SearchBackend>, Option<Arc<SqliteSearchBackend>>, bool) {
        info!("Using SQLite as primary backend (forced)");
        (Box::new(SqliteSearchBackend::new(db)), None, false)
    }

    /// Perform a search, with automatic fallback if primary fails
    pub fn search(&self, query: &SearchQuery) -> SearchResult {
        // If already using fallback, go straight to it
        if self.using_fallback.load(Ordering::Relaxed) {
            return self.search_with_fallback(query);
        }

        // Try primary backend
        match self.primary.search(query) {
            Ok(result) => {
                debug!(
                    "Primary backend ({}) returned {} results in {}ms",
                    result.backend_name, result.total_found, result.query_time_ms
                );
                result
            }
            Err(SearchError::NotAvailable) => {
                warn!(
                    "Primary backend ({}) not available, switching to fallback",
                    self.primary.name()
                );
                self.using_fallback.store(true, Ordering::Relaxed);
                self.search_with_fallback(query)
            }
            Err(e) => {
                warn!(
                    "Primary backend ({}) query failed: {}, trying fallback",
                    self.primary.name(),
                    e
                );
                self.search_with_fallback(query)
            }
        }
    }

    /// Search using fallback backend
    fn search_with_fallback(&self, query: &SearchQuery) -> SearchResult {
        if let Some(ref fallback) = self.fallback {
            match fallback.search(query) {
                Ok(result) => {
                    debug!(
                        "Fallback backend ({}) returned {} results in {}ms",
                        result.backend_name, result.total_found, result.query_time_ms
                    );
                    result
                }
                Err(e) => {
                    warn!("Fallback backend query failed: {}", e);
                    // Return empty results on complete failure
                    SearchResult {
                        files: Vec::new(),
                        total_found: 0,
                        query_time_ms: 0,
                        backend_name: "none".to_string(),
                    }
                }
            }
        } else {
            // No fallback available, try primary again (for SQLite-only mode)
            match self.primary.search(query) {
                Ok(result) => result,
                Err(e) => {
                    warn!("Search failed with no fallback: {}", e);
                    SearchResult {
                        files: Vec::new(),
                        total_found: 0,
                        query_time_ms: 0,
                        backend_name: "none".to_string(),
                    }
                }
            }
        }
    }

    /// Check if we're currently using the fallback backend
    pub fn is_using_fallback(&self) -> bool {
        self.using_fallback.load(Ordering::Relaxed)
    }

    /// Get the name of the currently active backend
    pub fn active_backend_name(&self) -> &'static str {
        if self.is_using_fallback() {
            if let Some(ref fallback) = self.fallback {
                fallback.name()
            } else {
                self.primary.name()
            }
        } else {
            self.primary.name()
        }
    }

    /// Get the configured backend type
    pub fn backend_type(&self) -> SearchBackendType {
        self.backend_type.clone()
    }

    /// Get status description of all backends
    pub fn status_description(&self) -> String {
        let primary_status = self.primary.status_description();

        if let Some(ref fallback) = self.fallback {
            let fallback_status = fallback.status_description();
            let active = if self.is_using_fallback() {
                "fallback"
            } else {
                "primary"
            };
            format!(
                "Primary: {}, Fallback: {}, Active: {}",
                primary_status, fallback_status, active
            )
        } else {
            format!("Backend: {}", primary_status)
        }
    }

    /// Check if the primary backend is Windows Search
    pub fn is_windows_search_primary(&self) -> bool {
        self.primary.name() == "WindowsSearch"
    }

    /// Check if indexing is needed (false when using Windows Search)
    pub fn needs_indexing(&self) -> bool {
        // If we're using Windows Search successfully, no indexing needed
        if self.is_windows_search_primary() && !self.is_using_fallback() {
            return false;
        }
        true
    }

    /// Force refresh of backend availability
    #[cfg(windows)]
    pub fn refresh_availability(&self) {
        // Try to switch back to primary if it becomes available
        if self.is_using_fallback() && self.primary.is_available() {
            info!("Primary backend became available, switching back");
            self.using_fallback.store(false, Ordering::Relaxed);
        }
    }

    #[cfg(not(windows))]
    pub fn refresh_availability(&self) {
        // No-op on non-Windows
    }

    /// Get reference to the database
    pub fn database(&self) -> &Arc<Database> {
        &self.db
    }
}
