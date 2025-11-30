//! StellaSearch - Lightweight file indexing service for Stella AI
//!
//! A cross-platform file indexing service that provides fast file search
//! using SQLite FTS5 full-text search or Windows Search (when available).

mod config;
mod database;
mod indexer;
mod ipc;
mod platform;
mod search;

use std::sync::Arc;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use crate::config::Config;
use crate::database::Database;
use crate::indexer::Indexer;
use crate::ipc::IpcServer;
use crate::search::SearchManager;

/// StellaSearch - Lightweight file indexing service
#[derive(Parser)]
#[command(name = "stella-search")]
#[command(author = "Misha")]
#[command(version = "0.1.0")]
#[command(about = "Lightweight file indexing service for Stella AI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the indexing service daemon
    Daemon,

    /// Start as Windows service (Windows only)
    #[cfg(windows)]
    Service,

    /// Search for files
    Search {
        /// Search query
        query: String,

        /// Maximum number of results
        #[arg(short, long, default_value = "20")]
        max_results: usize,

        /// Filter by file extension (e.g., ".pdf")
        #[arg(short, long)]
        extension: Option<String>,
    },

    /// Show index status
    Status,

    /// Add a path to the exclude list
    Exclude {
        /// Path to exclude
        path: String,
    },

    /// Remove a path from the exclude list
    Unexclude {
        /// Path to remove from exclusions
        path: String,
    },

    /// Add a path to the include list (for "selected" mode)
    Include {
        /// Path to include
        path: String,
    },

    /// Remove a path from the include list
    Uninclude {
        /// Path to remove from inclusions
        path: String,
    },

    /// Set indexing mode
    SetMode {
        /// Mode: "everything" or "selected"
        mode: String,
    },

    /// Trigger a full reindex
    Reindex {
        /// Optional path to reindex (defaults to all)
        path: Option<String>,
    },

    /// Show current configuration
    Config,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .with_target(false)
        .compact()
        .init();

    match cli.command {
        Commands::Daemon => {
            info!("Starting StellaSearch daemon...");
            run_daemon().await?;
        }

        #[cfg(windows)]
        Commands::Service => {
            info!("Starting as Windows service...");
            platform::windows::run_service()?;
        }

        Commands::Search {
            query,
            max_results,
            extension,
        } => {
            search_files(&query, max_results, extension.as_deref()).await?;
        }

        Commands::Status => {
            show_status().await?;
        }

        Commands::Exclude { path } => {
            add_exclusion(&path).await?;
        }

        Commands::Unexclude { path } => {
            remove_exclusion(&path).await?;
        }

        Commands::Include { path } => {
            add_inclusion(&path).await?;
        }

        Commands::Uninclude { path } => {
            remove_inclusion(&path).await?;
        }

        Commands::SetMode { mode } => {
            set_mode(&mode).await?;
        }

        Commands::Reindex { path } => {
            trigger_reindex(path.as_deref()).await?;
        }

        Commands::Config => {
            show_config().await?;
        }
    }

    Ok(())
}

/// Run the main daemon process
async fn run_daemon() -> Result<()> {
    // Load configuration
    let config = Config::load()?;
    info!("Configuration loaded: mode={}, search_backend={:?}",
          config.indexing.mode, config.search.backend);

    // Initialize database
    let db = Arc::new(Database::new(&config)?);
    db.init_schema()?;
    info!("Database initialized");

    // Create search manager
    let search_manager = Arc::new(SearchManager::new(config.search.backend.clone(), db.clone()));
    info!("Search backend: {}", search_manager.active_backend_name());

    // Create indexer
    let indexer = Indexer::new((*db).clone(), config.clone());

    // Only start indexing if needed (not using Windows Search as primary)
    if search_manager.needs_indexing() {
        info!("Starting local indexing...");

        // Start initial indexing in background
        let indexer_clone = indexer.clone();
        tokio::spawn(async move {
            if let Err(e) = indexer_clone.start_initial_scan().await {
                tracing::error!("Initial scan failed: {}", e);
            }
        });

        // Start file watcher
        let watcher_indexer = indexer.clone();
        tokio::spawn(async move {
            if let Err(e) = watcher_indexer.start_watcher().await {
                tracing::error!("File watcher failed: {}", e);
            }
        });
    } else {
        info!("Using Windows Search - skipping local indexing");
    }

    // Start IPC server (blocks until shutdown)
    let ipc_server = IpcServer::new(db, indexer, config, search_manager);
    ipc_server.run().await?;

    Ok(())
}

/// Search files via IPC client
async fn search_files(query: &str, max_results: usize, extension: Option<&str>) -> Result<()> {
    let client = ipc::IpcClient::connect().await?;
    let results = client.search(query, max_results, extension).await?;

    println!("Found {} files (showing up to {}):", results.total_found, max_results);
    println!();

    for file in &results.files {
        println!("  {} ({} bytes)", file.path, file.size);
    }

    println!();
    println!("Query time: {}ms", results.query_time_ms);

    Ok(())
}

/// Show index status via IPC client
async fn show_status() -> Result<()> {
    let client = ipc::IpcClient::connect().await?;
    let status = client.get_status().await?;

    println!("StellaSearch Status");
    println!("==================");
    println!("Indexed files:    {}", status.indexed_files);
    println!("Indexed dirs:     {}", status.indexed_dirs);
    println!("Database size:    {} MB", status.database_size_bytes / 1_000_000);
    println!("Is scanning:      {}", status.is_scanning);
    if status.is_scanning {
        println!("Scan progress:    {:.1}%", status.scan_progress * 100.0);
        if let Some(path) = &status.current_scan_path {
            println!("Current path:     {}", path);
        }
    }

    Ok(())
}

/// Add an exclusion path via IPC client
async fn add_exclusion(path: &str) -> Result<()> {
    let client = ipc::IpcClient::connect().await?;
    client.add_exclude(path).await?;
    println!("Added exclusion: {}", path);
    Ok(())
}

/// Remove an exclusion path via IPC client
async fn remove_exclusion(path: &str) -> Result<()> {
    let client = ipc::IpcClient::connect().await?;
    client.remove_exclude(path).await?;
    println!("Removed exclusion: {}", path);
    Ok(())
}

/// Add an inclusion path via IPC client
async fn add_inclusion(path: &str) -> Result<()> {
    let client = ipc::IpcClient::connect().await?;
    client.add_include(path).await?;
    println!("Added inclusion: {}", path);
    Ok(())
}

/// Remove an inclusion path via IPC client
async fn remove_inclusion(path: &str) -> Result<()> {
    let client = ipc::IpcClient::connect().await?;
    client.remove_include(path).await?;
    println!("Removed inclusion: {}", path);
    Ok(())
}

/// Set indexing mode via IPC client
async fn set_mode(mode: &str) -> Result<()> {
    let client = ipc::IpcClient::connect().await?;
    client.set_mode(mode).await?;
    println!("Indexing mode set to: {}", mode);
    Ok(())
}

/// Trigger reindex via IPC client
async fn trigger_reindex(path: Option<&str>) -> Result<()> {
    let client = ipc::IpcClient::connect().await?;
    client.reindex(path).await?;
    match path {
        Some(p) => println!("Reindex triggered for: {}", p),
        None => println!("Full reindex triggered"),
    }
    Ok(())
}

/// Show current configuration via IPC client
async fn show_config() -> Result<()> {
    let client = ipc::IpcClient::connect().await?;
    let config = client.get_config().await?;

    println!("StellaSearch Configuration");
    println!("==========================");
    println!("Mode:                {}", config.mode);
    println!("Auto-watch drives:   {}", config.auto_watch_new_drives);
    println!("Include hidden:      {}", config.include_hidden);
    println!();
    println!("Include paths ({}):", config.include_paths.len());
    for path in &config.include_paths {
        println!("  + {}", path);
    }
    println!();
    println!("Exclude paths ({}):", config.exclude_paths.len());
    for path in &config.exclude_paths {
        println!("  - {}", path);
    }
    println!();
    println!("Exclude patterns ({}):", config.exclude_patterns.len());
    for pattern in &config.exclude_patterns {
        println!("  - {}", pattern);
    }

    Ok(())
}
