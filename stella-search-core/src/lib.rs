//! Shared types for stella-search
//!
//! This crate contains types shared between the native library (DLL/SO)
//! and the daemon executable.

use serde::{Deserialize, Serialize};

/// Indexed file record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedFile {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub extension: Option<String>,
    pub size: i64,
    pub is_directory: bool,
}

/// Search results returned by both native library and daemon
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub files: Vec<IndexedFile>,
    pub total_found: usize,
    pub query_time_ms: u64,
}

/// Index statistics (used by daemon only, but shared for IPC)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub indexed_files: u64,
    pub indexed_dirs: u64,
    pub database_size_bytes: u64,
    pub is_scanning: bool,
    pub scan_progress: f64,
    pub current_scan_path: Option<String>,
}

/// Search error types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SearchError {
    /// System search not available
    NotAvailable,
    /// Query failed
    QueryFailed(String),
    /// Internal error
    Internal(String),
}

impl std::fmt::Display for SearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchError::NotAvailable => write!(f, "System search not available"),
            SearchError::QueryFailed(msg) => write!(f, "Query failed: {}", msg),
            SearchError::Internal(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for SearchError {}

/// Search backend identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchBackend {
    /// Windows Search via OLE DB
    WindowsSearch,
    /// Linux Tracker3 via D-Bus
    Tracker,
    /// Local SQLite FTS database
    SQLite,
}

impl std::fmt::Display for SearchBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SearchBackend::WindowsSearch => write!(f, "WindowsSearch"),
            SearchBackend::Tracker => write!(f, "Tracker"),
            SearchBackend::SQLite => write!(f, "SQLite"),
        }
    }
}
