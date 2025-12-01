//! File system watcher for real-time index updates

use anyhow::Result;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;
use tracing::{info, warn, debug, error};

use super::Indexer;

/// Start the file system watcher
pub async fn start_watcher(indexer: &Indexer) -> Result<()> {
    let config = indexer.config();
    let watch_paths = config.get_watch_paths();
    let debounce_ms = config.watcher.debounce_ms;

    info!("Starting file watcher for {} paths", watch_paths.len());

    // Create channel for events
    let (tx, rx) = mpsc::channel();

    // Create watcher with debouncing
    let watcher_config = Config::default()
        .with_poll_interval(Duration::from_millis(debounce_ms));

    let mut watcher: RecommendedWatcher = Watcher::new(
        move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        },
        watcher_config,
    )?;

    // Add watch paths
    for path in &watch_paths {
        match watcher.watch(path, RecursiveMode::Recursive) {
            Ok(_) => info!("Watching: {:?}", path),
            Err(e) => warn!("Failed to watch {:?}: {}", path, e),
        }
    }

    // Process events
    info!("File watcher started, processing events...");

    loop {
        if indexer.should_stop() {
            info!("File watcher stopping by request");
            break;
        }

        // Use recv_timeout to allow checking should_stop periodically
        match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(event) => {
                if let Err(e) = process_event(indexer, &event).await {
                    debug!("Error processing event: {}", e);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // No event, continue loop
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                error!("Watcher channel disconnected");
                break;
            }
        }
    }

    Ok(())
}

/// Process a file system event
async fn process_event(indexer: &Indexer, event: &Event) -> Result<()> {
    let config = indexer.config();

    for path in &event.paths {
        let path_str = path.to_string_lossy().to_string();

        // Check if path should be excluded
        if config.should_exclude(&path_str) {
            continue;
        }

        match &event.kind {
            EventKind::Create(_) => {
                info!("File created: {}", path_str);
                let is_dir = path.is_dir();
                let size = if is_dir { 0 } else {
                    std::fs::metadata(&path).map(|m| m.len() as i64).unwrap_or(0)
                };
                indexer.db().upsert_file(&path_str, is_dir, size)?;
            }

            EventKind::Modify(_) => {
                info!("File modified: {}", path_str);
                // Only update if it exists (might be a temporary file)
                if path.exists() {
                    let is_dir = path.is_dir();
                    let size = if is_dir { 0 } else {
                        std::fs::metadata(&path).map(|m| m.len() as i64).unwrap_or(0)
                    };
                    indexer.db().upsert_file(&path_str, is_dir, size)?;
                }
            }

            EventKind::Remove(_) => {
                info!("File removed: {}", path_str);
                indexer.db().delete_file(&path_str)?;
            }

            EventKind::Access(_) => {
                // Ignore access events
            }

            EventKind::Other => {
                // Ignore other events
            }

            _ => {
                debug!("Other event: {:?}", event.kind);
            }
        }
    }

    Ok(())
}
