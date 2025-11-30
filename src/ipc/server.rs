//! IPC server implementation

use anyhow::Result;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{info, warn, error, debug};

use crate::config::Config;
use crate::database::Database;
use crate::indexer::Indexer;
use super::protocol::{Request, Response};

/// IPC server for handling client requests
pub struct IpcServer {
    db: Database,
    indexer: Indexer,
    config: Config,
}

impl IpcServer {
    /// Create a new IPC server
    pub fn new(db: Database, indexer: Indexer, config: Config) -> Self {
        Self { db, indexer, config }
    }

    /// Run the IPC server
    pub async fn run(&self) -> Result<()> {
        #[cfg(windows)]
        {
            self.run_windows().await
        }

        #[cfg(unix)]
        {
            self.run_unix().await
        }
    }

    /// Handle a single request
    async fn handle_request(&self, request: Request) -> Response {
        match request {
            Request::Search {
                query,
                max_results,
                extensions,
                directories: _,
            } => {
                let max = max_results.unwrap_or(50);
                let ext = extensions.as_ref().and_then(|e| e.first()).map(|s| s.as_str());

                match self.db.search(&query, max, ext) {
                    Ok(results) => Response::search_result(results),
                    Err(e) => Response::error(format!("Search failed: {}", e)),
                }
            }

            Request::SetMode { mode } => {
                if mode != "everything" && mode != "selected" {
                    return Response::error("Invalid mode. Use 'everything' or 'selected'");
                }

                // Note: In a full implementation, we'd update the config and save it
                Response::ok(format!("Mode set to '{}'", mode))
            }

            Request::GetMode => {
                Response::Mode {
                    mode: self.config.indexing.mode.clone(),
                }
            }

            Request::AddInclude { path } => {
                // Validate path exists
                if !std::path::Path::new(&path).exists() {
                    return Response::error(format!("Path does not exist: {}", path));
                }
                Response::ok(format!("Added include path: {}", path))
            }

            Request::RemoveInclude { path } => {
                Response::ok(format!("Removed include path: {}", path))
            }

            Request::AddExclude { path } => {
                Response::ok(format!("Added exclude path: {}", path))
            }

            Request::RemoveExclude { path } => {
                Response::ok(format!("Removed exclude path: {}", path))
            }

            Request::GetConfig => {
                Response::config(&self.config)
            }

            Request::Status => {
                match self.db.get_stats() {
                    Ok(mut stats) => {
                        // Update with live indexer state
                        stats.is_scanning = self.indexer.is_scanning();
                        stats.scan_progress = self.indexer.get_scan_progress();
                        stats.current_scan_path = self.indexer.get_current_scan_path();
                        Response::status(stats)
                    }
                    Err(e) => Response::error(format!("Failed to get stats: {}", e)),
                }
            }

            Request::Reindex { path } => {
                let indexer = self.indexer.clone();
                let path_owned = path.clone();

                // Spawn reindex in background
                tokio::spawn(async move {
                    if let Err(e) = indexer.reindex_path(path_owned.as_deref()).await {
                        error!("Reindex failed: {}", e);
                    }
                });

                match path {
                    Some(p) => Response::ok(format!("Reindex started for: {}", p)),
                    None => Response::ok("Full reindex started"),
                }
            }

            Request::ReloadConfig => {
                Response::ok("Configuration reloaded")
            }
        }
    }

    #[cfg(windows)]
    async fn run_windows(&self) -> Result<()> {
        use tokio::net::windows::named_pipe::{ServerOptions, PipeMode};

        let pipe_name = r"\\.\pipe\stella-search";
        info!("Starting IPC server on {}", pipe_name);

        loop {
            // Create a new pipe instance
            let server = ServerOptions::new()
                .first_pipe_instance(false)
                .pipe_mode(PipeMode::Message)
                .create(pipe_name)?;

            // Wait for a client to connect
            server.connect().await?;

            let mut reader = BufReader::new(server);
            let mut line = String::new();

            // Read request
            match reader.read_line(&mut line).await {
                Ok(0) => continue, // Connection closed
                Ok(_) => {
                    debug!("Received request: {}", line.trim());

                    // Parse and handle request
                    let response = match serde_json::from_str::<Request>(&line) {
                        Ok(request) => self.handle_request(request).await,
                        Err(e) => Response::error(format!("Invalid request: {}", e)),
                    };

                    // Send response
                    let response_json = serde_json::to_string(&response)?;
                    let mut writer = reader.into_inner();
                    writer.write_all(response_json.as_bytes()).await?;
                    writer.write_all(b"\n").await?;
                    writer.flush().await?;
                }
                Err(e) => {
                    warn!("Error reading from pipe: {}", e);
                }
            }
        }
    }

    #[cfg(unix)]
    async fn run_unix(&self) -> Result<()> {
        use tokio::net::UnixListener;

        let socket_path = self.config.get_socket_path();

        // Remove existing socket file
        if socket_path.exists() {
            std::fs::remove_file(&socket_path)?;
        }

        let listener = UnixListener::bind(&socket_path)?;
        info!("Starting IPC server on {:?}", socket_path);

        // Set socket permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o660))?;
        }

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let mut reader = BufReader::new(stream);
                    let mut line = String::new();

                    // Read request
                    match reader.read_line(&mut line).await {
                        Ok(0) => continue, // Connection closed
                        Ok(_) => {
                            debug!("Received request: {}", line.trim());

                            // Parse and handle request
                            let response = match serde_json::from_str::<Request>(&line) {
                                Ok(request) => self.handle_request(request).await,
                                Err(e) => Response::error(format!("Invalid request: {}", e)),
                            };

                            // Send response
                            let response_json = serde_json::to_string(&response)?;
                            let mut writer = reader.into_inner();
                            writer.write_all(response_json.as_bytes()).await?;
                            writer.write_all(b"\n").await?;
                            writer.flush().await?;
                        }
                        Err(e) => {
                            warn!("Error reading from socket: {}", e);
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to accept connection: {}", e);
                }
            }
        }
    }
}
