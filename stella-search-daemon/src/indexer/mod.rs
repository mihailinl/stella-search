//! File indexing module
//!
//! Handles directory scanning and file watching.

mod scanner;
mod watcher;
#[cfg(windows)]
mod mft_scanner;

#[allow(unused_imports)]
pub use scanner::scan_directory_public;

use std::sync::{Arc, atomic::{AtomicBool, AtomicU64, Ordering}};
use std::sync::RwLock;
use anyhow::Result;

use crate::config::Config;
use crate::database::Database;

/// Shared indexer state
#[derive(Clone)]
pub struct Indexer {
    db: Database,
    config: Config,
    state: Arc<IndexerState>,
}

/// Indexer runtime state
pub struct IndexerState {
    pub is_scanning: AtomicBool,
    pub scan_progress: AtomicU64,  // Stored as progress * 10000 for precision
    pub current_scan_path: RwLock<Option<String>>,
    pub should_stop: AtomicBool,
}

impl Indexer {
    /// Create a new indexer
    pub fn new(db: Database, config: Config) -> Self {
        Self {
            db,
            config,
            state: Arc::new(IndexerState {
                is_scanning: AtomicBool::new(false),
                scan_progress: AtomicU64::new(0),
                current_scan_path: RwLock::new(None),
                should_stop: AtomicBool::new(false),
            }),
        }
    }

    /// Check if currently scanning
    pub fn is_scanning(&self) -> bool {
        self.state.is_scanning.load(Ordering::Relaxed)
    }

    /// Get scan progress (0.0 - 1.0)
    pub fn get_scan_progress(&self) -> f64 {
        self.state.scan_progress.load(Ordering::Relaxed) as f64 / 10000.0
    }

    /// Get current scan path
    pub fn get_current_scan_path(&self) -> Option<String> {
        self.state.current_scan_path.read().unwrap().clone()
    }

    /// Set scan progress
    fn set_progress(&self, progress: f64, path: Option<&str>) {
        self.state.scan_progress.store((progress * 10000.0) as u64, Ordering::Relaxed);
        if let Some(p) = path {
            *self.state.current_scan_path.write().unwrap() = Some(p.to_string());
        }
    }

    /// Request stop
    pub fn request_stop(&self) {
        self.state.should_stop.store(true, Ordering::Relaxed);
    }

    /// Check if should stop
    fn should_stop(&self) -> bool {
        self.state.should_stop.load(Ordering::Relaxed)
    }

    /// Start initial scan
    /// Uses MFT scanner on Windows NTFS volumes for maximum speed,
    /// falls back to walkdir for non-NTFS volumes or other platforms.
    /// Skips scan if database already has indexed files.
    pub async fn start_initial_scan(&self) -> Result<()> {
        // Check if already indexed - skip scan if we have files
        let stats = self.db().get_stats()?;
        if stats.indexed_files > 0 {
            tracing::info!(
                "Database already has {} files indexed, skipping initial scan. Use 'reindex' command to force re-scan.",
                stats.indexed_files
            );
            return Ok(());
        }

        #[cfg(windows)]
        {
            // Try MFT scanner first on Windows (requires admin)
            match mft_scanner::start_mft_scan(self).await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    tracing::warn!("MFT scan failed, falling back to walkdir: {}", e);
                    // Fall through to walkdir
                }
            }
        }

        scanner::start_initial_scan(self).await
    }

    /// Start file watcher
    pub async fn start_watcher(&self) -> Result<()> {
        watcher::start_watcher(self).await
    }

    /// Reindex a specific path
    pub async fn reindex_path(&self, path: Option<&str>) -> Result<()> {
        scanner::reindex_path(self, path).await
    }

    /// Get database reference
    pub fn db(&self) -> &Database {
        &self.db
    }

    /// Get config reference
    pub fn config(&self) -> &Config {
        &self.config
    }
}
