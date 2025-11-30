//! Database schema and initialization

use anyhow::{Context, Result};
use rusqlite::Connection;
use std::sync::{Arc, Mutex};
use tracing::info;

use crate::config::Config;

/// Database wrapper with connection pooling
#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
    db_path: String,
}

impl Database {
    /// Create a new database connection
    pub fn new(config: &Config) -> Result<Self> {
        let db_path = config.db_path.to_string_lossy().to_string();

        // Ensure parent directory exists
        if let Some(parent) = config.db_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create database directory: {:?}", parent))?;
        }

        let conn = Connection::open(&config.db_path)
            .with_context(|| format!("Failed to open database: {:?}", config.db_path))?;

        // Enable WAL mode for better concurrent access
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path,
        })
    }

    /// Enable bulk insert mode for fast indexing
    /// Call this before starting a large batch insert operation
    pub fn begin_bulk_insert(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        // Use journal_mode=OFF for fast writes without journal overhead
        // Reduced cache_size to 50MB to limit RAM usage
        conn.execute_batch(
            "PRAGMA synchronous = OFF;
             PRAGMA journal_mode = OFF;
             PRAGMA cache_size = -50000;
             PRAGMA temp_store = MEMORY;"
        )?;
        info!("Bulk insert mode enabled");
        Ok(())
    }

    /// End bulk insert mode and restore normal settings
    /// Call this after completing a large batch insert operation
    pub fn end_bulk_insert(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        // Restore normal settings
        conn.execute_batch(
            "PRAGMA synchronous = NORMAL;
             PRAGMA journal_mode = WAL;"
        )?;
        info!("Bulk insert mode disabled, normal settings restored");

        // Note: VACUUM removed - it takes too long on large databases
        // Database size will be slightly larger but indexing completes faster

        Ok(())
    }

    /// Initialize the database schema
    pub fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();

        // Check if we need to migrate from old schema
        // Old schema had 'directory' column, new schema doesn't
        let needs_migration: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('files') WHERE name = 'directory'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|count| count > 0)
            .unwrap_or(false);

        if needs_migration {
            info!("Detected old schema, dropping files table for migration");
            conn.execute_batch("DROP TABLE IF EXISTS files; DROP TABLE IF EXISTS files_fts;")?;
        }

        conn.execute_batch(SCHEMA_SQL)?;

        info!("Database schema initialized");
        Ok(())
    }

    /// Get a connection handle
    pub fn connection(&self) -> std::sync::MutexGuard<Connection> {
        self.conn.lock().unwrap()
    }

    /// Get database file size in bytes
    pub fn get_size(&self) -> Result<u64> {
        let metadata = std::fs::metadata(&self.db_path)?;
        Ok(metadata.len())
    }
}

/// SQL schema for the database
/// Optimized for fast bulk inserts and small database size
/// No FTS5 - uses simple LIKE queries which are fast enough for filename search
const SCHEMA_SQL: &str = r#"
-- Main files table (simplified - removed directory, modified, indexed_at)
CREATE TABLE IF NOT EXISTS files (
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    extension TEXT,
    size INTEGER NOT NULL DEFAULT 0,
    is_directory INTEGER NOT NULL DEFAULT 0
);

-- Only 2 indexes needed for search
CREATE INDEX IF NOT EXISTS idx_files_name ON files(name);
CREATE INDEX IF NOT EXISTS idx_files_extension ON files(extension);

-- Index statistics table
CREATE TABLE IF NOT EXISTS stats (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Initialize stats if not present
INSERT OR IGNORE INTO stats (key, value) VALUES ('last_full_scan', '0');
INSERT OR IGNORE INTO stats (key, value) VALUES ('total_files', '0');
INSERT OR IGNORE INTO stats (key, value) VALUES ('total_dirs', '0');
"#;
