//! IPC protocol definitions

use serde::{Deserialize, Serialize};
use crate::database::{IndexedFile, SearchResults, IndexStats};

/// Request message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Request {
    /// Search for files
    Search {
        query: String,
        max_results: Option<usize>,
        extensions: Option<Vec<String>>,
        directories: Option<Vec<String>>,
    },

    /// Set indexing mode
    SetMode {
        mode: String,
    },

    /// Get current mode
    GetMode,

    /// Add path to include list
    AddInclude {
        path: String,
    },

    /// Remove path from include list
    RemoveInclude {
        path: String,
    },

    /// Add path to exclude list
    AddExclude {
        path: String,
    },

    /// Remove path from exclude list
    RemoveExclude {
        path: String,
    },

    /// Get current configuration
    GetConfig,

    /// Get index status
    Status,

    /// Trigger reindex
    Reindex {
        path: Option<String>,
    },

    /// Reload configuration
    ReloadConfig,
}

/// Response message types
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    /// Search results
    SearchResult {
        files: Vec<IndexedFile>,
        total_found: usize,
        query_time_ms: u64,
    },

    /// Status response
    Status {
        search_backend: String,
        indexed_files: u64,
        indexed_dirs: u64,
        database_size_bytes: u64,
        is_scanning: bool,
        scan_progress: f64,
        current_scan_path: Option<String>,
    },

    /// Config response
    Config {
        mode: String,
        include_paths: Vec<String>,
        exclude_paths: Vec<String>,
        exclude_patterns: Vec<String>,
        auto_watch_new_drives: bool,
        include_hidden: bool,
    },

    /// Mode response
    Mode {
        mode: String,
    },

    /// Success response
    Ok {
        message: String,
    },

    /// Error response
    Error {
        message: String,
    },
}

impl Response {
    /// Create an OK response
    pub fn ok(message: impl Into<String>) -> Self {
        Response::Ok {
            message: message.into(),
        }
    }

    /// Create an error response
    pub fn error(message: impl Into<String>) -> Self {
        Response::Error {
            message: message.into(),
        }
    }

    /// Create a search result response
    pub fn search_result(results: SearchResults) -> Self {
        Response::SearchResult {
            files: results.files,
            total_found: results.total_found,
            query_time_ms: results.query_time_ms,
        }
    }

    /// Create a status response
    pub fn status(stats: IndexStats, search_backend: String) -> Self {
        Response::Status {
            search_backend,
            indexed_files: stats.indexed_files,
            indexed_dirs: stats.indexed_dirs,
            database_size_bytes: stats.database_size_bytes,
            is_scanning: stats.is_scanning,
            scan_progress: stats.scan_progress,
            current_scan_path: stats.current_scan_path,
        }
    }

    /// Create a config response
    pub fn config(config: &crate::config::Config) -> Self {
        Response::Config {
            mode: config.indexing.mode.clone(),
            include_paths: config.watch.include.clone(),
            exclude_paths: config.watch.exclude.clone(),
            exclude_patterns: config.watch.exclude_patterns.clone(),
            auto_watch_new_drives: config.watcher.auto_watch_new_drives,
            include_hidden: config.watch.include_hidden,
        }
    }
}

/// Config response for IPC client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigResponse {
    pub mode: String,
    pub include_paths: Vec<String>,
    pub exclude_paths: Vec<String>,
    pub exclude_patterns: Vec<String>,
    pub auto_watch_new_drives: bool,
    pub include_hidden: bool,
}

/// Status response for IPC client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    pub search_backend: String,
    pub indexed_files: u64,
    pub indexed_dirs: u64,
    pub database_size_bytes: u64,
    pub is_scanning: bool,
    pub scan_progress: f64,
    pub current_scan_path: Option<String>,
}
