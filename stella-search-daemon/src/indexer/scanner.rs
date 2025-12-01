//! Directory scanner for initial indexing

use anyhow::Result;
use std::path::Path;
use std::sync::atomic::Ordering;
use tracing::{info, warn, debug};
use walkdir::WalkDir;

use super::Indexer;

/// Start the initial directory scan
pub async fn start_initial_scan(indexer: &Indexer) -> Result<()> {
    indexer.state.is_scanning.store(true, Ordering::Relaxed);
    indexer.state.scan_progress.store(0, Ordering::Relaxed);

    let watch_paths = indexer.config().get_watch_paths();
    info!("Starting initial scan of {} paths", watch_paths.len());

    // Enable bulk insert mode for faster indexing
    if let Err(e) = indexer.db().begin_bulk_insert() {
        warn!("Failed to enable bulk insert mode: {}", e);
    }

    let total_paths = watch_paths.len();
    for (i, path) in watch_paths.iter().enumerate() {
        if indexer.should_stop() {
            info!("Scan stopped by request");
            break;
        }

        let base_progress = i as f64 / total_paths as f64;
        indexer.set_progress(base_progress, Some(&path.to_string_lossy()));

        info!("Scanning: {:?}", path);
        if let Err(e) = scan_directory(indexer, path, base_progress, 1.0 / total_paths as f64).await {
            warn!("Error scanning {:?}: {}", path, e);
        }
    }

    // Restore normal database settings
    if let Err(e) = indexer.db().end_bulk_insert() {
        warn!("Failed to disable bulk insert mode: {}", e);
    }

    indexer.state.is_scanning.store(false, Ordering::Relaxed);
    indexer.set_progress(1.0, None);

    info!("Initial scan complete");
    Ok(())
}

/// Reindex a specific path or all paths
pub async fn reindex_path(indexer: &Indexer, path: Option<&str>) -> Result<()> {
    indexer.state.is_scanning.store(true, Ordering::Relaxed);

    match path {
        Some(p) => {
            info!("Reindexing path: {}", p);
            indexer.set_progress(0.0, Some(p));

            // Clear existing entries under this path
            indexer.db().delete_directory(p)?;

            // Rescan
            scan_directory(indexer, Path::new(p), 0.0, 1.0).await?;
        }
        None => {
            info!("Full reindex requested");

            // Clear all entries
            indexer.db().clear_all()?;

            // Rescan everything
            start_initial_scan(indexer).await?;
            return Ok(());
        }
    }

    indexer.state.is_scanning.store(false, Ordering::Relaxed);
    indexer.set_progress(1.0, None);

    Ok(())
}

/// Scan a single directory recursively
async fn scan_directory(
    indexer: &Indexer,
    path: &Path,
    base_progress: f64,
    progress_range: f64,
) -> Result<()> {
    let config = indexer.config();
    // Use large batch size for bulk inserts (50,000 files per transaction)
    let batch_size = 50_000;

    let mut batch: Vec<(String, bool)> = Vec::with_capacity(batch_size);
    let mut processed = 0u64;
    let mut total_estimate = 1000u64; // Initial estimate, will be updated

    // First pass: count entries for progress estimation (quick)
    if let Ok(count) = quick_count_entries(path) {
        total_estimate = count.max(1);
    }

    for entry in WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !should_skip_entry(e, config))
    {
        if indexer.should_stop() {
            // Flush remaining batch before stopping
            if !batch.is_empty() {
                let _ = indexer.db().batch_upsert_files(&batch);
            }
            return Ok(());
        }

        match entry {
            Ok(entry) => {
                let path_str = entry.path().to_string_lossy().to_string();
                let is_dir = entry.file_type().is_dir();

                // Skip the root path itself
                if entry.depth() == 0 {
                    continue;
                }

                batch.push((path_str, is_dir));

                if batch.len() >= batch_size {
                    if let Err(e) = indexer.db().batch_upsert_files(&batch) {
                        warn!("Failed to batch insert: {}", e);
                    }
                    batch.clear();

                    processed += batch_size as u64;
                    let progress = base_progress + (processed as f64 / total_estimate as f64) * progress_range;
                    indexer.set_progress(progress.min(base_progress + progress_range), Some(&entry.path().to_string_lossy()));
                }
            }
            Err(e) => {
                debug!("Error walking directory: {}", e);
            }
        }
    }

    // Flush remaining entries
    if !batch.is_empty() {
        if let Err(e) = indexer.db().batch_upsert_files(&batch) {
            warn!("Failed to batch insert remaining: {}", e);
        }
    }

    Ok(())
}

/// Quick count of entries in a directory (for progress estimation)
fn quick_count_entries(path: &Path) -> Result<u64> {
    let mut count = 0u64;

    // Only count top-level for speed, multiply by estimate
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            count += 1;
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                count += 100; // Estimate subdirectory contents
            }
        }
    }

    Ok(count)
}

/// Check if a directory entry should be skipped
fn should_skip_entry(entry: &walkdir::DirEntry, config: &crate::config::Config) -> bool {
    let path = entry.path();
    let path_str = path.to_string_lossy();

    // Check if path should be excluded
    if config.should_exclude(&path_str) {
        return true;
    }

    // Skip hidden files if configured
    if !config.watch.include_hidden {
        if let Some(name) = path.file_name() {
            if name.to_string_lossy().starts_with('.') {
                return true;
            }
        }
    }

    false
}

/// Public wrapper for scan_directory (used by MFT scanner fallback)
pub async fn scan_directory_public(
    indexer: &Indexer,
    path: &Path,
    base_progress: f64,
    progress_range: f64,
) -> Result<()> {
    scan_directory(indexer, path, base_progress, progress_range).await
}
