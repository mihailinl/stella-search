//! IPC client for communicating with the service

use anyhow::{Context, Result, bail};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use serde_json;

use crate::database::SearchResults;
use super::protocol::{Request, Response, ConfigResponse, StatusResponse};

/// IPC client for communicating with the StellaSearch service
pub struct IpcClient {
    // Connection will be established per-request
}

impl IpcClient {
    /// Connect to the IPC server
    pub async fn connect() -> Result<Self> {
        Ok(Self {})
    }

    /// Send a request and receive a response
    async fn send_request(&self, request: &Request) -> Result<Response> {
        #[cfg(windows)]
        {
            self.send_request_windows(request).await
        }

        #[cfg(unix)]
        {
            self.send_request_unix(request).await
        }
    }

    #[cfg(windows)]
    async fn send_request_windows(&self, request: &Request) -> Result<Response> {
        use tokio::net::windows::named_pipe::ClientOptions;

        let pipe_name = r"\\.\pipe\stella-search";

        let client = ClientOptions::new()
            .open(pipe_name)
            .context("Failed to connect to StellaSearch service. Is it running?")?;

        let request_json = serde_json::to_string(request)?;

        // Write request
        let mut writer = client;
        writer.write_all(request_json.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;

        // Read response
        let mut reader = BufReader::new(writer);
        let mut line = String::new();
        reader.read_line(&mut line).await?;

        let response: Response = serde_json::from_str(&line)?;
        Ok(response)
    }

    #[cfg(unix)]
    async fn send_request_unix(&self, request: &Request) -> Result<Response> {
        use tokio::net::UnixStream;

        // Try XDG_RUNTIME_DIR first, then /tmp
        let socket_path = if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
            std::path::PathBuf::from(runtime_dir).join("stella-search.sock")
        } else {
            std::path::PathBuf::from("/tmp/stella-search.sock")
        };

        let stream = UnixStream::connect(&socket_path)
            .await
            .context("Failed to connect to StellaSearch service. Is it running?")?;

        let request_json = serde_json::to_string(request)?;

        // Split into reader and writer
        let (reader, mut writer) = stream.into_split();

        // Write request
        writer.write_all(request_json.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;

        // Read response
        let mut reader = BufReader::new(reader);
        let mut line = String::new();
        reader.read_line(&mut line).await?;

        let response: Response = serde_json::from_str(&line)?;
        Ok(response)
    }

    /// Search for files
    pub async fn search(
        &self,
        query: &str,
        max_results: usize,
        extension: Option<&str>,
    ) -> Result<SearchResults> {
        let request = Request::Search {
            query: query.to_string(),
            max_results: Some(max_results),
            extensions: extension.map(|e| vec![e.to_string()]),
            directories: None,
        };

        match self.send_request(&request).await? {
            Response::SearchResult {
                files,
                total_found,
                query_time_ms,
            } => Ok(SearchResults {
                files,
                total_found,
                query_time_ms,
            }),
            Response::Error { message } => bail!("Search failed: {}", message),
            _ => bail!("Unexpected response type"),
        }
    }

    /// Get index status
    pub async fn get_status(&self) -> Result<StatusResponse> {
        let request = Request::Status;

        match self.send_request(&request).await? {
            Response::Status {
                indexed_files,
                indexed_dirs,
                database_size_bytes,
                is_scanning,
                scan_progress,
                current_scan_path,
            } => Ok(StatusResponse {
                indexed_files,
                indexed_dirs,
                database_size_bytes,
                is_scanning,
                scan_progress,
                current_scan_path,
            }),
            Response::Error { message } => bail!("Status failed: {}", message),
            _ => bail!("Unexpected response type"),
        }
    }

    /// Get configuration
    pub async fn get_config(&self) -> Result<ConfigResponse> {
        let request = Request::GetConfig;

        match self.send_request(&request).await? {
            Response::Config {
                mode,
                include_paths,
                exclude_paths,
                exclude_patterns,
                auto_watch_new_drives,
                include_hidden,
            } => Ok(ConfigResponse {
                mode,
                include_paths,
                exclude_paths,
                exclude_patterns,
                auto_watch_new_drives,
                include_hidden,
            }),
            Response::Error { message } => bail!("Get config failed: {}", message),
            _ => bail!("Unexpected response type"),
        }
    }

    /// Set indexing mode
    pub async fn set_mode(&self, mode: &str) -> Result<()> {
        let request = Request::SetMode {
            mode: mode.to_string(),
        };

        match self.send_request(&request).await? {
            Response::Ok { .. } => Ok(()),
            Response::Error { message } => bail!("Set mode failed: {}", message),
            _ => bail!("Unexpected response type"),
        }
    }

    /// Add include path
    pub async fn add_include(&self, path: &str) -> Result<()> {
        let request = Request::AddInclude {
            path: path.to_string(),
        };

        match self.send_request(&request).await? {
            Response::Ok { .. } => Ok(()),
            Response::Error { message } => bail!("Add include failed: {}", message),
            _ => bail!("Unexpected response type"),
        }
    }

    /// Remove include path
    pub async fn remove_include(&self, path: &str) -> Result<()> {
        let request = Request::RemoveInclude {
            path: path.to_string(),
        };

        match self.send_request(&request).await? {
            Response::Ok { .. } => Ok(()),
            Response::Error { message } => bail!("Remove include failed: {}", message),
            _ => bail!("Unexpected response type"),
        }
    }

    /// Add exclude path
    pub async fn add_exclude(&self, path: &str) -> Result<()> {
        let request = Request::AddExclude {
            path: path.to_string(),
        };

        match self.send_request(&request).await? {
            Response::Ok { .. } => Ok(()),
            Response::Error { message } => bail!("Add exclude failed: {}", message),
            _ => bail!("Unexpected response type"),
        }
    }

    /// Remove exclude path
    pub async fn remove_exclude(&self, path: &str) -> Result<()> {
        let request = Request::RemoveExclude {
            path: path.to_string(),
        };

        match self.send_request(&request).await? {
            Response::Ok { .. } => Ok(()),
            Response::Error { message } => bail!("Remove exclude failed: {}", message),
            _ => bail!("Unexpected response type"),
        }
    }

    /// Trigger reindex
    pub async fn reindex(&self, path: Option<&str>) -> Result<()> {
        let request = Request::Reindex {
            path: path.map(|s| s.to_string()),
        };

        match self.send_request(&request).await? {
            Response::Ok { .. } => Ok(()),
            Response::Error { message } => bail!("Reindex failed: {}", message),
            _ => bail!("Unexpected response type"),
        }
    }
}
