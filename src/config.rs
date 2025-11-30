//! Configuration management for StellaSearch
//!
//! Handles loading, saving, and managing the service configuration.

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tracing::info;

/// Main configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub indexing: IndexingConfig,

    #[serde(default)]
    pub search: SearchConfig,

    #[serde(default)]
    pub watch: WatchConfig,

    #[serde(default)]
    pub watcher: WatcherConfig,

    #[serde(default)]
    pub service: ServiceConfig,

    #[serde(default)]
    pub performance: PerformanceConfig,

    /// Path to config file (not serialized)
    #[serde(skip)]
    pub config_path: PathBuf,

    /// Path to database file (not serialized)
    #[serde(skip)]
    pub db_path: PathBuf,
}

/// Indexing mode configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexingConfig {
    /// Mode: "everything" or "selected"
    #[serde(default = "default_mode")]
    pub mode: String,
}

/// Search backend configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchConfig {
    /// Search backend: "auto", "windows", or "sqlite"
    /// - "auto" = Windows Search if available, else SQLite (default)
    /// - "windows" = Force Windows Search (falls back if unavailable)
    /// - "sqlite" = Force custom SQLite indexer
    #[serde(default)]
    pub backend: SearchBackendType,
}

/// Search backend type
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SearchBackendType {
    /// Auto-detect: Windows Search if available, else SQLite
    #[default]
    Auto,
    /// Force Windows Search (on Windows only)
    Windows,
    /// Force SQLite-based search
    Sqlite,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            backend: SearchBackendType::default(),
        }
    }
}

/// Watch paths configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchConfig {
    /// Included paths (for "selected" mode or additional paths in "everything" mode)
    #[serde(default)]
    pub include: Vec<String>,

    /// Excluded paths (absolute paths)
    #[serde(default = "default_exclude_paths")]
    pub exclude: Vec<String>,

    /// Pattern-based exclusions (glob patterns)
    #[serde(default = "default_exclude_patterns")]
    pub exclude_patterns: Vec<String>,

    /// File extensions to exclude
    #[serde(default)]
    pub exclude_extensions: Vec<String>,

    /// Include hidden files/directories
    #[serde(default)]
    pub include_hidden: bool,
}

/// File watcher configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherConfig {
    /// Debounce delay in milliseconds
    #[serde(default = "default_debounce_ms")]
    pub debounce_ms: u64,

    /// Maximum events to batch
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,

    /// Auto-watch new drives/mount points
    #[serde(default = "default_true")]
    pub auto_watch_new_drives: bool,
}

/// Service configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceConfig {
    /// Log level
    #[serde(default = "default_log_level")]
    pub log_level: String,

    /// Custom socket path (optional)
    #[serde(default)]
    pub socket_path: Option<String>,
}

/// Performance configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceConfig {
    /// Number of indexing threads (0 = auto)
    #[serde(default)]
    pub threads: usize,

    /// Memory limit for batch operations (MB)
    #[serde(default = "default_memory_limit")]
    pub memory_limit_mb: usize,
}

// Default value functions
fn default_mode() -> String {
    "everything".to_string()
}

fn default_debounce_ms() -> u64 {
    500
}

fn default_batch_size() -> usize {
    100
}

fn default_true() -> bool {
    true
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_memory_limit() -> usize {
    50
}

fn default_exclude_paths() -> Vec<String> {
    let mut paths = Vec::new();

    #[cfg(windows)]
    {
        paths.extend(vec![
            "C:/Windows".to_string(),
            "C:/Windows.old".to_string(),
            "C:/$Recycle.Bin".to_string(),
            "C:/System Volume Information".to_string(),
            "C:/Recovery".to_string(),
            "C:/PerfLogs".to_string(),
            "C:/ProgramData/Microsoft".to_string(),
            "C:/ProgramData/Package Cache".to_string(),
        ]);
    }

    #[cfg(unix)]
    {
        paths.extend(vec![
            "/proc".to_string(),
            "/sys".to_string(),
            "/dev".to_string(),
            "/run".to_string(),
            "/tmp".to_string(),
            "/var/cache".to_string(),
            "/var/log".to_string(),
            "/lost+found".to_string(),
        ]);
    }

    paths
}

fn default_exclude_patterns() -> Vec<String> {
    vec![
        // Development folders
        "**/node_modules".to_string(),
        "**/.git".to_string(),
        "**/target".to_string(),
        "**/bin".to_string(),
        "**/obj".to_string(),
        "**/__pycache__".to_string(),
        "**/venv".to_string(),
        "**/.venv".to_string(),
        "**/.gradle".to_string(),
        "**/.maven".to_string(),
        "**/build".to_string(),
        // IDE/Editor folders
        "**/.vs".to_string(),
        "**/.idea".to_string(),
        "**/.vscode".to_string(),
        // Package caches
        "**/.npm".to_string(),
        "**/.nuget".to_string(),
        "**/.cargo/registry".to_string(),
        "**/.rustup".to_string(),
        // Temporary files
        "**/*.tmp".to_string(),
        "**/*.temp".to_string(),
        "**/*.bak".to_string(),
        "**/*.swp".to_string(),
        "**/*.lock".to_string(),
        "**/Thumbs.db".to_string(),
        "**/.DS_Store".to_string(),
    ]
}

impl Default for IndexingConfig {
    fn default() -> Self {
        Self {
            mode: default_mode(),
        }
    }
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            include: Vec::new(),
            exclude: default_exclude_paths(),
            exclude_patterns: default_exclude_patterns(),
            exclude_extensions: Vec::new(),
            include_hidden: false,
        }
    }
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            debounce_ms: default_debounce_ms(),
            batch_size: default_batch_size(),
            auto_watch_new_drives: true,
        }
    }
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            log_level: default_log_level(),
            socket_path: None,
        }
    }
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            threads: 0,
            memory_limit_mb: default_memory_limit(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        let (config_path, db_path) = Self::get_default_paths();
        Self {
            indexing: IndexingConfig::default(),
            search: SearchConfig::default(),
            watch: WatchConfig::default(),
            watcher: WatcherConfig::default(),
            service: ServiceConfig::default(),
            performance: PerformanceConfig::default(),
            config_path,
            db_path,
        }
    }
}

impl Config {
    /// Get default paths for config and database
    fn get_default_paths() -> (PathBuf, PathBuf) {
        if let Some(proj_dirs) = ProjectDirs::from("com", "stella", "stella-search") {
            let config_dir = proj_dirs.config_dir();
            let data_dir = proj_dirs.data_dir();

            // Create directories if they don't exist
            let _ = fs::create_dir_all(config_dir);
            let _ = fs::create_dir_all(data_dir);

            (
                config_dir.join("config.toml"),
                data_dir.join("stella-search.db"),
            )
        } else {
            // Fallback paths
            #[cfg(windows)]
            {
                let appdata = std::env::var("APPDATA").unwrap_or_else(|_| ".".to_string());
                let base = PathBuf::from(appdata).join("StellaSearch");
                let _ = fs::create_dir_all(&base);
                (base.join("config.toml"), base.join("stella-search.db"))
            }

            #[cfg(unix)]
            {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                let config_dir = PathBuf::from(&home).join(".config/stella-search");
                let data_dir = PathBuf::from(&home).join(".local/share/stella-search");
                let _ = fs::create_dir_all(&config_dir);
                let _ = fs::create_dir_all(&data_dir);
                (
                    config_dir.join("config.toml"),
                    data_dir.join("stella-search.db"),
                )
            }
        }
    }

    /// Load configuration from file, or create default if not exists
    pub fn load() -> Result<Self> {
        let (config_path, db_path) = Self::get_default_paths();

        let mut config = if config_path.exists() {
            let content = fs::read_to_string(&config_path)
                .with_context(|| format!("Failed to read config file: {:?}", config_path))?;
            let mut config: Config = toml::from_str(&content)
                .with_context(|| format!("Failed to parse config file: {:?}", config_path))?;
            config.config_path = config_path;
            config.db_path = db_path;
            config
        } else {
            info!("Config file not found, creating default at {:?}", config_path);
            let config = Config::default();
            config.save()?;
            config
        };

        Ok(config)
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<()> {
        let content = toml::to_string_pretty(self)
            .context("Failed to serialize config")?;

        // Ensure parent directory exists
        if let Some(parent) = self.config_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory: {:?}", parent))?;
        }

        fs::write(&self.config_path, content)
            .with_context(|| format!("Failed to write config file: {:?}", self.config_path))?;

        info!("Configuration saved to {:?}", self.config_path);
        Ok(())
    }

    /// Get socket path for IPC
    pub fn get_socket_path(&self) -> PathBuf {
        if let Some(custom_path) = &self.service.socket_path {
            PathBuf::from(custom_path)
        } else {
            #[cfg(windows)]
            {
                PathBuf::from(r"\\.\pipe\stella-search")
            }

            #[cfg(unix)]
            {
                // Use XDG_RUNTIME_DIR if available, otherwise /tmp
                if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
                    PathBuf::from(runtime_dir).join("stella-search.sock")
                } else {
                    PathBuf::from("/tmp/stella-search.sock")
                }
            }
        }
    }

    /// Check if a path should be excluded
    pub fn should_exclude(&self, path: &str) -> bool {
        // Check absolute exclusions
        for excluded in &self.watch.exclude {
            let excluded_normalized = excluded.replace('\\', "/");
            let path_normalized = path.replace('\\', "/");

            if path_normalized.starts_with(&excluded_normalized) {
                return true;
            }
        }

        // Check pattern exclusions
        for pattern in &self.watch.exclude_patterns {
            if let Ok(glob) = glob::Pattern::new(pattern) {
                let path_normalized = path.replace('\\', "/");
                if glob.matches(&path_normalized) {
                    return true;
                }
            }
        }

        // Check extension exclusions
        if !self.watch.exclude_extensions.is_empty() {
            if let Some(ext) = std::path::Path::new(path).extension() {
                let ext_str = format!(".{}", ext.to_string_lossy());
                if self.watch.exclude_extensions.contains(&ext_str) {
                    return true;
                }
            }
        }

        // Check hidden files
        if !self.watch.include_hidden {
            let path_obj = std::path::Path::new(path);
            if let Some(name) = path_obj.file_name() {
                if name.to_string_lossy().starts_with('.') {
                    return true;
                }
            }
        }

        false
    }

    /// Get paths to watch based on mode
    pub fn get_watch_paths(&self) -> Vec<PathBuf> {
        match self.indexing.mode.as_str() {
            "selected" => {
                // Only watch included paths
                self.watch.include.iter().map(PathBuf::from).collect()
            }
            _ => {
                // "everything" mode - watch all drives
                let mut paths = Vec::new();

                #[cfg(windows)]
                {
                    // Get all drive letters
                    for letter in b'A'..=b'Z' {
                        let drive = format!("{}:\\", letter as char);
                        let path = PathBuf::from(&drive);
                        if path.exists() {
                            paths.push(path);
                        }
                    }
                }

                #[cfg(unix)]
                {
                    // Watch root and common mount points
                    paths.push(PathBuf::from("/"));
                }

                // Also add any extra include paths
                for include_path in &self.watch.include {
                    let path = PathBuf::from(include_path);
                    if !paths.contains(&path) {
                        paths.push(path);
                    }
                }

                paths
            }
        }
    }
}

/// Thread-safe configuration wrapper
#[derive(Clone)]
pub struct SharedConfig {
    inner: Arc<RwLock<Config>>,
}

impl SharedConfig {
    pub fn new(config: Config) -> Self {
        Self {
            inner: Arc::new(RwLock::new(config)),
        }
    }

    pub fn read(&self) -> std::sync::RwLockReadGuard<Config> {
        self.inner.read().unwrap()
    }

    pub fn write(&self) -> std::sync::RwLockWriteGuard<Config> {
        self.inner.write().unwrap()
    }

    pub fn reload(&self) -> Result<()> {
        let new_config = Config::load()?;
        let mut config = self.inner.write().unwrap();
        *config = new_config;
        Ok(())
    }
}
