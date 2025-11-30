//! Linux-specific platform implementation

use anyhow::Result;

/// Get all mount points on Linux
#[cfg(unix)]
pub fn get_mount_points() -> Vec<String> {
    let mut mounts = Vec::new();

    // Read /proc/mounts to get all mounted filesystems
    if let Ok(content) = std::fs::read_to_string("/proc/mounts") {
        for line in content.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let mount_point = parts[1];

                // Skip virtual filesystems
                if mount_point.starts_with("/proc")
                    || mount_point.starts_with("/sys")
                    || mount_point.starts_with("/dev")
                    || mount_point.starts_with("/run")
                {
                    continue;
                }

                // Only include real filesystems
                let fs_type = parts.get(2).unwrap_or(&"");
                if *fs_type == "ext4"
                    || *fs_type == "ext3"
                    || *fs_type == "xfs"
                    || *fs_type == "btrfs"
                    || *fs_type == "ntfs"
                    || *fs_type == "vfat"
                    || *fs_type == "exfat"
                    || *fs_type == "fuseblk"
                {
                    mounts.push(mount_point.to_string());
                }
            }
        }
    }

    // Always include root if no mounts found
    if mounts.is_empty() {
        mounts.push("/".to_string());
    }

    mounts
}

#[cfg(not(unix))]
pub fn get_mount_points() -> Vec<String> {
    Vec::new()
}

/// Setup signal handlers for graceful shutdown
#[cfg(unix)]
pub fn setup_signal_handlers() -> Result<tokio::sync::mpsc::Receiver<()>> {
    use tokio::signal::unix::{signal, SignalKind};

    let (tx, rx) = tokio::sync::mpsc::channel(1);

    tokio::spawn(async move {
        let mut sigterm = signal(SignalKind::terminate()).expect("Failed to register SIGTERM handler");
        let mut sigint = signal(SignalKind::interrupt()).expect("Failed to register SIGINT handler");

        tokio::select! {
            _ = sigterm.recv() => {
                tracing::info!("Received SIGTERM");
            }
            _ = sigint.recv() => {
                tracing::info!("Received SIGINT");
            }
        }

        let _ = tx.send(()).await;
    });

    Ok(rx)
}

#[cfg(not(unix))]
pub fn setup_signal_handlers() -> Result<tokio::sync::mpsc::Receiver<()>> {
    let (_tx, rx) = tokio::sync::mpsc::channel(1);
    Ok(rx)
}
