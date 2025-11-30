//! MFT (Master File Table) based scanner for fast Windows indexing
//!
//! Uses direct MFT access instead of filesystem API for 10-50x faster scanning.
//! Requires administrator privileges.

#[cfg(windows)]
use anyhow::{Context, Result};
#[cfg(windows)]
use ntfs_reader::file_info::FileInfo;
#[cfg(windows)]
use ntfs_reader::mft::Mft;
#[cfg(windows)]
use ntfs_reader::volume::Volume;
#[cfg(windows)]
use std::sync::atomic::Ordering;
#[cfg(windows)]
use tracing::{error, info, warn};

#[cfg(windows)]
use super::Indexer;
#[cfg(windows)]
use crate::database::FileMetadata;

/// Scan an NTFS volume using MFT (Master File Table) for maximum speed.
/// This reads metadata directly from NTFS structures instead of calling stat() per file.
///
/// # Arguments
/// * `indexer` - The indexer instance
/// * `drive_letter` - Drive letter (e.g., 'C')
/// * `base_progress` - Starting progress value (0.0 - 1.0)
/// * `progress_range` - Progress range for this drive
///
/// # Returns
/// Number of files indexed
#[cfg(windows)]
pub async fn scan_volume_mft(
    indexer: &Indexer,
    drive_letter: char,
    base_progress: f64,
    progress_range: f64,
) -> Result<u64> {
    let volume_path = format!("\\\\.\\{}:", drive_letter);
    info!("Starting MFT scan for volume {}", volume_path);

    // Open the volume (requires admin)
    let volume = Volume::new(&volume_path)
        .with_context(|| format!("Failed to open volume {}. Are you running as administrator?", volume_path))?;

    let mft = Mft::new(volume)
        .with_context(|| format!("Failed to read MFT from {}", volume_path))?;

    let config = indexer.config();
    // Use large batch size for bulk inserts (50,000 files per transaction)
    let batch_size = 50_000;
    let include_hidden = config.watch.include_hidden;

    let mut batch: Vec<FileMetadata> = Vec::with_capacity(batch_size);
    let mut indexed_count = 0u64;
    let mut processed = 0u64;

    // Estimate total files for progress (MFT record count)
    let total_estimate = mft.max_record;
    info!("MFT contains approximately {} records", total_estimate);

    // Iterate through all MFT entries
    info!("Starting MFT iteration...");
    mft.iterate_files(|file| {
        if indexer.should_stop() {
            return; // Exit iteration early
        }

        let info = FileInfo::new(&mft, file);
        processed += 1;

        // Skip system files and special entries
        if should_skip_mft_entry(&info, include_hidden, config) {
            return;
        }

        // Build path string - ntfs-reader returns paths without drive letter
        // e.g., "\Users\misha\file.txt" - we need to prepend "C:"
        let raw_path = info.path.to_string_lossy().to_string();
        let path_str = if raw_path.starts_with('\\') || raw_path.starts_with('/') {
            format!("{}:{}", drive_letter, raw_path.replace('/', "\\"))
        } else if raw_path.is_empty() {
            // Root directory
            format!("{}:\\", drive_letter)
        } else {
            format!("{}:\\{}", drive_letter, raw_path.replace('/', "\\"))
        };

        // Check exclusion patterns
        if config.should_exclude(&path_str) {
            return;
        }

        // Create metadata from MFT info - no stat() call needed!
        let metadata = FileMetadata {
            path: path_str,
            name: info.name.clone(),
            size: if info.is_directory { 0 } else { info.size as i64 },
            is_directory: info.is_directory,
        };

        batch.push(metadata);

        // Flush batch when full
        if batch.len() >= batch_size {
            if let Err(e) = indexer.db().batch_upsert_files_with_metadata(&batch) {
                warn!("Failed to batch insert: {}", e);
            } else {
                indexed_count += batch.len() as u64;
                // Log progress every batch
                info!("Indexed {} files so far...", indexed_count);
            }
            batch.clear();

            // Update progress
            let progress = base_progress + (processed as f64 / total_estimate as f64) * progress_range;
            indexer.set_progress(progress.min(base_progress + progress_range), None);
        }
    });
    info!("MFT iteration complete, processed {} records", processed);

    // Flush remaining entries
    if !batch.is_empty() {
        if let Err(e) = indexer.db().batch_upsert_files_with_metadata(&batch) {
            warn!("Failed to batch insert remaining: {}", e);
        } else {
            indexed_count += batch.len() as u64;
        }
    }

    info!(
        "MFT scan complete for {}: indexed {} files from {} MFT records",
        volume_path, indexed_count, processed
    );

    Ok(indexed_count)
}

/// Check if an MFT entry should be skipped
#[cfg(windows)]
fn should_skip_mft_entry(
    info: &FileInfo,
    include_hidden: bool,
    _config: &crate::config::Config,
) -> bool {
    // Skip entries with empty names (deleted or system metadata)
    if info.name.is_empty() {
        return true;
    }

    // Skip common system files/directories
    let name_lower = info.name.to_lowercase();
    if name_lower == "$mft"
        || name_lower == "$mftmirr"
        || name_lower == "$logfile"
        || name_lower == "$volume"
        || name_lower == "$attrdef"
        || name_lower == "$bitmap"
        || name_lower == "$boot"
        || name_lower == "$badclus"
        || name_lower == "$secure"
        || name_lower == "$upcase"
        || name_lower == "$extend"
        || name_lower == "$quota"
        || name_lower == "$objid"
        || name_lower == "$reparse"
        || name_lower == "$usnjrnl"
    {
        return true;
    }

    // Skip hidden files if configured
    if !include_hidden && info.name.starts_with('.') {
        return true;
    }

    false
}

/// Get list of NTFS drives on the system
#[cfg(windows)]
pub fn get_ntfs_drives() -> Vec<char> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    let mut drives = Vec::new();

    // Get logical drive bitmask
    let mask = unsafe { windows_sys::Win32::Storage::FileSystem::GetLogicalDrives() };

    for i in 0..26 {
        if (mask & (1 << i)) != 0 {
            let letter = (b'A' + i as u8) as char;
            let root_path: Vec<u16> = OsStr::new(&format!("{}:\\", letter))
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();

            // Check if it's NTFS
            let mut fs_name = [0u16; 260];
            let result = unsafe {
                windows_sys::Win32::Storage::FileSystem::GetVolumeInformationW(
                    root_path.as_ptr(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    fs_name.as_mut_ptr(),
                    fs_name.len() as u32,
                )
            };

            if result != 0 {
                let fs_name_len = fs_name.iter().position(|&c| c == 0).unwrap_or(fs_name.len());
                let fs_name_str = String::from_utf16_lossy(&fs_name[..fs_name_len]);
                if fs_name_str == "NTFS" {
                    drives.push(letter);
                }
            }
        }
    }

    drives
}

/// Start MFT-based initial scan for all NTFS volumes
#[cfg(windows)]
pub async fn start_mft_scan(indexer: &Indexer) -> Result<()> {
    indexer.state.is_scanning.store(true, Ordering::Relaxed);
    indexer.state.scan_progress.store(0, Ordering::Relaxed);

    let ntfs_drives = get_ntfs_drives();
    info!("Found {} NTFS drives: {:?}", ntfs_drives.len(), ntfs_drives);

    if ntfs_drives.is_empty() {
        warn!("No NTFS drives found, falling back to walkdir scanner");
        return super::scanner::start_initial_scan(indexer).await;
    }

    // Enable bulk insert mode for maximum speed
    if let Err(e) = indexer.db().begin_bulk_insert() {
        warn!("Failed to enable bulk insert mode: {}", e);
    }

    let total_drives = ntfs_drives.len();
    let mut total_indexed = 0u64;

    for (i, drive) in ntfs_drives.iter().enumerate() {
        if indexer.should_stop() {
            info!("MFT scan stopped by request");
            break;
        }

        let base_progress = i as f64 / total_drives as f64;
        let progress_range = 1.0 / total_drives as f64;

        indexer.set_progress(base_progress, Some(&format!("{}:", drive)));

        match scan_volume_mft(indexer, *drive, base_progress, progress_range).await {
            Ok(count) => {
                total_indexed += count;
                info!("Indexed {} files from drive {}", count, drive);
            }
            Err(e) => {
                error!("Failed to scan drive {} via MFT: {}. Falling back to walkdir.", drive, e);
                // Fall back to walkdir for this drive
                let path = format!("{}:\\", drive);
                if let Err(e2) = super::scanner::scan_directory_public(
                    indexer,
                    std::path::Path::new(&path),
                    base_progress,
                    progress_range,
                ).await {
                    warn!("Walkdir fallback also failed for {}: {}", drive, e2);
                }
            }
        }
    }

    // Restore normal database settings
    if let Err(e) = indexer.db().end_bulk_insert() {
        warn!("Failed to disable bulk insert mode: {}", e);
    }

    indexer.state.is_scanning.store(false, Ordering::Relaxed);
    indexer.set_progress(1.0, None);

    info!("MFT scan complete: indexed {} total files", total_indexed);
    Ok(())
}

// Non-Windows stub implementations
#[cfg(not(windows))]
pub async fn start_mft_scan(_indexer: &super::Indexer) -> anyhow::Result<()> {
    anyhow::bail!("MFT scanning is only available on Windows")
}

#[cfg(not(windows))]
pub fn get_ntfs_drives() -> Vec<char> {
    Vec::new()
}
