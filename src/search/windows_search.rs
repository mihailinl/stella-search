//! Windows Search backend using OLE DB
//!
//! Queries the Windows Search Index (SystemIndex) using OLE DB via PowerShell.
//! This is the primary search backend on Windows when available.

use std::sync::atomic::{AtomicBool, Ordering};

use tracing::{debug, info, warn};

use crate::database::IndexedFile;
use super::{SearchBackend, SearchError, SearchQuery, SearchResult};

/// Windows Search backend using OLE DB to query SystemIndex
pub struct WindowsSearchBackend {
    available: AtomicBool,
    last_error: std::sync::RwLock<Option<String>>,
}

impl WindowsSearchBackend {
    /// Create a new Windows Search backend
    pub fn new() -> Self {
        let backend = Self {
            available: AtomicBool::new(false),
            last_error: std::sync::RwLock::new(None),
        };

        // Check availability on creation
        backend.check_availability();
        backend
    }

    /// Check if Windows Search service is running and accessible
    /// Note: Only checks service status, not query capability (for fast startup)
    fn check_availability(&self) {
        match Self::is_wsearch_running() {
            Ok(running) => {
                if running {
                    // Service is running - assume available (will fail on first query if not)
                    info!("Windows Search service is running");
                    self.available.store(true, Ordering::SeqCst);
                    *self.last_error.write().unwrap() = None;
                } else {
                    debug!("Windows Search service is not running");
                    self.available.store(false, Ordering::SeqCst);
                    *self.last_error.write().unwrap() = Some("WSearch service not running".into());
                }
            }
            Err(e) => {
                warn!("Failed to check Windows Search status: {}", e);
                self.available.store(false, Ordering::SeqCst);
                *self.last_error.write().unwrap() = Some(e.to_string());
            }
        }
    }

    /// Check if WSearch service is running using sc query
    fn is_wsearch_running() -> Result<bool, String> {
        use std::process::Command;

        let output = Command::new("sc")
            .args(["query", "WSearch"])
            .output()
            .map_err(|e| format!("Failed to query WSearch service: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Check if service is running
        Ok(stdout.contains("RUNNING"))
    }

    /// Perform a test query to verify Windows Search is working
    fn test_query(&self) -> Result<(), SearchError> {
        // Execute a minimal test query
        self.execute_search("test_query_check_12345", 1, None, None)?;
        Ok(())
    }

    /// Execute a Windows Search query using OLE DB
    fn execute_search(
        &self,
        query: &str,
        max_results: usize,
        extension: Option<&str>,
        directories: Option<&[String]>,
    ) -> Result<Vec<IndexedFile>, SearchError> {
        // Build the SQL query for Windows Search
        let sql = self.build_search_sql(query, max_results, extension, directories);

        debug!("Executing Windows Search query: {}", sql);

        // Execute via ADO/OLE DB
        self.execute_oledb_query(&sql)
    }

    /// Build SQL query for Windows Search
    fn build_search_sql(
        &self,
        query: &str,
        max_results: usize,
        extension: Option<&str>,
        directories: Option<&[String]>,
    ) -> String {
        let mut conditions = Vec::new();

        // Filename search - using LIKE for substring matching
        // Escape single quotes in query
        let escaped_query = query.replace('\'', "''");
        conditions.push(format!("System.FileName LIKE '%{}%'", escaped_query));

        // Extension filter
        if let Some(ext) = extension {
            let escaped_ext = ext.replace('\'', "''");
            // Windows Search uses System.ItemType for extension (includes the dot)
            conditions.push(format!("System.ItemType = '{}'", escaped_ext));
        }

        // Directory filter (scope)
        if let Some(dirs) = directories {
            if !dirs.is_empty() {
                let scope_conditions: Vec<String> = dirs
                    .iter()
                    .map(|d| {
                        let escaped = d.replace('\'', "''").replace('\\', "/");
                        format!("SCOPE = 'file:{}'", escaped)
                    })
                    .collect();
                conditions.push(format!("({})", scope_conditions.join(" OR ")));
            }
        }

        let where_clause = conditions.join(" AND ");

        format!(
            r#"SELECT TOP {} System.ItemPathDisplay, System.FileName, System.ItemType, System.Size, System.ItemFolderPathDisplay
               FROM SystemIndex
               WHERE {}
               ORDER BY System.Search.Rank DESC"#,
            max_results, where_clause
        )
    }

    /// Execute OLE DB query using ADO via COM
    fn execute_oledb_query(&self, sql: &str) -> Result<Vec<IndexedFile>, SearchError> {
        // Use PowerShell to execute the ADO query (simplest approach for OLE DB)
        // This avoids complex COM interop in Rust while still being fast
        use std::process::Command;

        // Build PowerShell script to query Windows Search
        let ps_script = format!(
            r#"
$conn = New-Object -ComObject ADODB.Connection
$conn.Open("Provider=Search.CollatorDSO;Extended Properties='Application=Windows'")
$rs = $conn.Execute(@"
{}
"@)

$results = @()
while (-not $rs.EOF) {{
    $path = $rs.Fields.Item("System.ItemPathDisplay").Value
    $name = $rs.Fields.Item("System.FileName").Value
    $itemType = $rs.Fields.Item("System.ItemType").Value
    $size = $rs.Fields.Item("System.Size").Value
    $folder = $rs.Fields.Item("System.ItemFolderPathDisplay").Value

    if ($null -eq $size) {{ $size = 0 }}
    $isDir = ($itemType -eq "Directory") -or ($itemType -eq "Folder") -or [string]::IsNullOrEmpty($itemType)

    $results += [PSCustomObject]@{{
        path = $path
        name = $name
        extension = if ($isDir) {{ $null }} else {{ $itemType }}
        size = [long]$size
        is_directory = $isDir
    }}
    $rs.MoveNext()
}}
$rs.Close()
$conn.Close()

$results | ConvertTo-Json -Compress
"#,
            sql.replace('"', "`\"")
        );

        let output = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps_script])
            .output()
            .map_err(|e| SearchError::QueryFailed(format!("PowerShell execution failed: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Check if this is "no results" vs actual error
            if stderr.contains("EOF") || stderr.contains("BOF") {
                return Ok(Vec::new());
            }
            return Err(SearchError::QueryFailed(format!(
                "PowerShell query failed: {}",
                stderr
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stdout = stdout.trim();

        // Handle empty results
        if stdout.is_empty() || stdout == "null" {
            return Ok(Vec::new());
        }

        // Parse JSON results
        self.parse_search_results(stdout)
    }

    /// Parse JSON results from PowerShell
    fn parse_search_results(&self, json: &str) -> Result<Vec<IndexedFile>, SearchError> {
        // Handle single result (PowerShell doesn't wrap single item in array)
        let json = if json.starts_with('[') {
            json.to_string()
        } else {
            format!("[{}]", json)
        };

        #[derive(serde::Deserialize)]
        struct WinSearchResult {
            path: Option<String>,
            name: Option<String>,
            extension: Option<String>,
            size: Option<i64>,
            is_directory: Option<bool>,
        }

        let results: Vec<WinSearchResult> = serde_json::from_str(&json)
            .map_err(|e| SearchError::QueryFailed(format!("Failed to parse results: {}", e)))?;

        Ok(results
            .into_iter()
            .filter_map(|r| {
                let path = r.path?;
                let name = r.name.unwrap_or_else(|| {
                    std::path::Path::new(&path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default()
                });

                Some(IndexedFile {
                    id: 0, // Windows Search doesn't have IDs
                    path,
                    name,
                    extension: r.extension,
                    size: r.size.unwrap_or(0),
                    is_directory: r.is_directory.unwrap_or(false),
                })
            })
            .collect())
    }

    /// Refresh availability status
    pub fn refresh_availability(&self) {
        self.check_availability();
    }
}

impl Default for WindowsSearchBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchBackend for WindowsSearchBackend {
    fn is_available(&self) -> bool {
        self.available.load(Ordering::SeqCst)
    }

    fn search(&self, query: &SearchQuery) -> Result<SearchResult, SearchError> {
        if !self.is_available() {
            return Err(SearchError::NotAvailable);
        }

        let start = std::time::Instant::now();

        let files = self.execute_search(
            &query.query,
            query.max_results,
            query.extension.as_deref(),
            query.directories.as_deref(),
        )?;

        let total_found = files.len();

        Ok(SearchResult {
            files,
            total_found,
            query_time_ms: start.elapsed().as_millis() as u64,
            backend_name: self.name().to_string(),
        })
    }

    fn name(&self) -> &'static str {
        "WindowsSearch"
    }

    fn status_description(&self) -> String {
        if self.is_available() {
            "Windows Search (using system index)".to_string()
        } else {
            let error = self.last_error.read().unwrap();
            match error.as_ref() {
                Some(e) => format!("Windows Search (unavailable: {})", e),
                None => "Windows Search (unavailable)".to_string(),
            }
        }
    }
}
