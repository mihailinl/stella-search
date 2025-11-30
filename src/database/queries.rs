//! Database query operations
//! Optimized for fast bulk inserts and small database size
//! Uses simple LIKE queries instead of FTS5 (fast enough for filename search)

use anyhow::Result;
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::path::Path;

use super::Database;

/// Pre-computed file metadata from MFT or filesystem
/// Used for efficient batch inserts without per-file stat() calls
#[derive(Debug, Clone)]
pub struct FileMetadata {
    pub path: String,
    pub name: String,
    pub size: i64,
    pub is_directory: bool,
}

/// Indexed file record (simplified - no directory, modified, indexed_at)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexedFile {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub extension: Option<String>,
    pub size: i64,
    pub is_directory: bool,
}

/// Search results
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResults {
    pub files: Vec<IndexedFile>,
    pub total_found: usize,
    pub query_time_ms: u64,
}

/// Index statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub indexed_files: u64,
    pub indexed_dirs: u64,
    pub database_size_bytes: u64,
    pub is_scanning: bool,
    pub scan_progress: f64,
    pub current_scan_path: Option<String>,
}

impl Database {
    /// Insert or update a file in the index (simplified schema)
    pub fn upsert_file(&self, path: &str, is_directory: bool, size: i64) -> Result<()> {
        let path_obj = Path::new(path);
        let name = path_obj
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let extension = if is_directory {
            None
        } else {
            path_obj.extension().map(|e| format!(".{}", e.to_string_lossy()))
        };

        let conn = self.connection();
        conn.execute(
            r#"
            INSERT INTO files (path, name, extension, size, is_directory)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(path) DO UPDATE SET
                name = excluded.name,
                extension = excluded.extension,
                size = excluded.size,
                is_directory = excluded.is_directory
            "#,
            params![path, name, extension, size, is_directory as i32],
        )?;

        Ok(())
    }

    /// Batch insert files with pre-computed metadata (for MFT scanner)
    /// This is the fastest path - no stat() calls, no extra columns
    pub fn batch_upsert_files_with_metadata(&self, files: &[FileMetadata]) -> Result<()> {
        let mut conn = self.connection();
        let tx = conn.transaction()?;

        {
            let mut stmt = tx.prepare(
                r#"
                INSERT INTO files (path, name, extension, size, is_directory)
                VALUES (?1, ?2, ?3, ?4, ?5)
                ON CONFLICT(path) DO UPDATE SET
                    name = excluded.name,
                    extension = excluded.extension,
                    size = excluded.size,
                    is_directory = excluded.is_directory
                "#,
            )?;

            for file in files {
                let extension = if file.is_directory {
                    None
                } else {
                    Path::new(&file.path)
                        .extension()
                        .map(|e| format!(".{}", e.to_string_lossy()))
                };

                stmt.execute(params![
                    file.path,
                    file.name,
                    extension,
                    file.size,
                    file.is_directory as i32,
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    /// Batch insert files for walkdir scanner (computes metadata from path)
    pub fn batch_upsert_files(&self, files: &[(String, bool)]) -> Result<()> {
        let mut conn = self.connection();
        let tx = conn.transaction()?;

        {
            let mut stmt = tx.prepare(
                r#"
                INSERT INTO files (path, name, extension, size, is_directory)
                VALUES (?1, ?2, ?3, ?4, ?5)
                ON CONFLICT(path) DO UPDATE SET
                    name = excluded.name,
                    extension = excluded.extension,
                    size = excluded.size,
                    is_directory = excluded.is_directory
                "#,
            )?;

            for (path, is_directory) in files {
                let path_obj = Path::new(path);
                let name = path_obj
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                let (extension, size) = if *is_directory {
                    (None, 0i64)
                } else {
                    let ext = path_obj.extension().map(|e| format!(".{}", e.to_string_lossy()));
                    let size = std::fs::metadata(path)
                        .map(|m| m.len() as i64)
                        .unwrap_or(0);
                    (ext, size)
                };

                stmt.execute(params![
                    path,
                    name,
                    extension,
                    size,
                    *is_directory as i32,
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    /// Delete a file from the index
    pub fn delete_file(&self, path: &str) -> Result<()> {
        let conn = self.connection();
        conn.execute("DELETE FROM files WHERE path = ?1", params![path])?;
        Ok(())
    }

    /// Delete all files under a directory
    pub fn delete_directory(&self, directory: &str) -> Result<()> {
        let conn = self.connection();
        // Delete the directory itself and all files/subdirs under it
        let like_pattern = format!("{}%", directory.replace('\\', "/"));
        conn.execute(
            "DELETE FROM files WHERE path LIKE ?1",
            params![like_pattern],
        )?;
        Ok(())
    }

    /// Search files using simple LIKE queries (fast enough for filename search)
    /// No FTS5 - Everything proves this approach works for billions of files
    pub fn search(&self, query: &str, max_results: usize, extension: Option<&str>) -> Result<SearchResults> {
        let start = std::time::Instant::now();
        let conn = self.connection();

        // Build LIKE pattern for substring matching
        let like_pattern = format!("%{}%", query);

        // Helper to extract IndexedFile from a row
        fn row_to_file(row: &rusqlite::Row) -> rusqlite::Result<IndexedFile> {
            Ok(IndexedFile {
                id: row.get(0)?,
                path: row.get(1)?,
                name: row.get(2)?,
                extension: row.get(3)?,
                size: row.get(4)?,
                is_directory: row.get::<_, i32>(5)? != 0,
            })
        }

        let files: Vec<IndexedFile> = if let Some(ext) = extension {
            // Filter by extension first (uses index), then LIKE on name
            let sql = r#"
                SELECT id, path, name, extension, size, is_directory
                FROM files
                WHERE extension = ?1 AND name LIKE ?2
                LIMIT ?3
            "#;
            let mut stmt = conn.prepare(sql)?;
            stmt.query_map(params![ext, like_pattern, max_results as i64], row_to_file)?
                .filter_map(|r| r.ok())
                .collect()
        } else {
            // General search on name
            let sql = r#"
                SELECT id, path, name, extension, size, is_directory
                FROM files
                WHERE name LIKE ?1
                LIMIT ?2
            "#;
            let mut stmt = conn.prepare(sql)?;
            stmt.query_map(params![like_pattern, max_results as i64], row_to_file)?
                .filter_map(|r| r.ok())
                .collect()
        };

        let total_found = files.len();
        let query_time_ms = start.elapsed().as_millis() as u64;

        Ok(SearchResults {
            files,
            total_found,
            query_time_ms,
        })
    }

    /// Get index statistics
    pub fn get_stats(&self) -> Result<IndexStats> {
        let conn = self.connection();

        let indexed_files: u64 = conn.query_row(
            "SELECT COUNT(*) FROM files WHERE is_directory = 0",
            [],
            |row| row.get(0),
        )?;

        let indexed_dirs: u64 = conn.query_row(
            "SELECT COUNT(*) FROM files WHERE is_directory = 1",
            [],
            |row| row.get(0),
        )?;

        drop(conn);

        let database_size_bytes = self.get_size().unwrap_or(0);

        Ok(IndexStats {
            indexed_files,
            indexed_dirs,
            database_size_bytes,
            is_scanning: false,  // Will be updated by indexer
            scan_progress: 0.0,
            current_scan_path: None,
        })
    }

    /// Clear all indexed files
    pub fn clear_all(&self) -> Result<()> {
        let conn = self.connection();
        conn.execute("DELETE FROM files", [])?;
        Ok(())
    }
}
