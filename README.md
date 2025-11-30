# StellaSearch

A lightweight, fast, and privacy-focused file indexing service for desktop systems. Built with Rust for maximum performance and minimal resource usage.

## Features

- **Fast Full-Text Search**: Uses SQLite FTS5 for blazing-fast file searches (typically <1ms)
- **Real-Time Indexing**: Watches for file changes and updates the index automatically
- **Cross-Platform**: Works on Windows and Linux
- **Privacy-Focused**: 100% offline - your files never leave your computer
- **Low Resource Usage**: ~5-10MB RAM when idle, ~2MB binary size
- **Flexible Configuration**: Watch everything with exclusions, or only specific folders
- **Smart Exclusions**: Automatically excludes system folders, node_modules, .git, etc.
- **Windows Service Support**: Can run as a Windows service for background operation

## Installation

### Build from Source

1. Install Rust: https://rustup.rs/
2. Clone and build:

```bash
git clone https://github.com/YourUsername/stella-search.git
cd stella-search
cargo build --release
```

The binary will be at `target/release/stella-search.exe` (Windows) or `target/release/stella-search` (Linux).

## Usage

### Start the Daemon

```bash
# Run in foreground
stella-search daemon

# Or install as Windows service (admin required)
stella-search service install
net start StellaSearch
```

### Search Files

```bash
# Basic search
stella-search search "document"

# Search with extension filter
stella-search search "main" --extension ".rs"

# Limit results
stella-search search "config" --max-results 10
```

### Check Status

```bash
stella-search status
```

Output:
```
StellaSearch Status
==================
Indexed files:    150000
Indexed dirs:     12000
Database size:    45 MB
Is scanning:      false
Scan progress:    100%
```

### View Configuration

```bash
stella-search config
```

### Manage Exclusions

```bash
# Add exclusion
stella-search exclude "C:\MyLargeBackups"

# Remove exclusion
stella-search unexclude "C:\MyLargeBackups"
```

### Change Indexing Mode

```bash
# Watch everything (with exclusions)
stella-search set-mode everything

# Watch only selected folders
stella-search set-mode selected

# Add folders to watch in selected mode
stella-search include "C:\Projects"
stella-search include "D:\Documents"
```

### Trigger Reindex

```bash
stella-search reindex
```

## Configuration

Configuration is stored at:
- Windows: `%APPDATA%\stella\stella-search\config\config.toml`
- Linux: `~/.config/stella/stella-search/config/config.toml`

Example `config.toml`:

```toml
[indexing]
mode = "everything"  # or "selected"
auto_watch_new_drives = true
include_hidden = false

include_paths = []  # Used when mode = "selected"

exclude_paths = [
    "C:/Windows",
    "C:/$Recycle.Bin",
]

exclude_patterns = [
    "**/node_modules",
    "**/.git",
    "**/target",
]
```

## IPC Protocol

StellaSearch exposes a JSON-based IPC interface for integration with other applications:

- Windows: Named pipe `\\.\pipe\stella-search`
- Linux: Unix socket `/tmp/stella-search.sock`

### Request/Response Format

```json
// Search request
{"Search": {"query": "document", "max_results": 20, "extension": null}}

// Response
{
  "SearchResults": {
    "files": [
      {
        "path": "C:/Users/user/document.txt",
        "name": "document.txt",
        "size": 1234,
        "modified": 1699000000
      }
    ],
    "total_found": 1,
    "query_time_ms": 0
  }
}
```

## Integration with Stella

StellaSearch is designed to integrate with the Stella AI assistant. The Stella UI can:
- Start/stop the indexing service
- Configure include/exclude paths
- Perform file searches through the AI

## Performance

- **Initial Index**: ~1 minute per 100,000 files
- **Search Speed**: <1ms for most queries
- **Memory Usage**: 5-10MB idle, 50-100MB during scan
- **Database Size**: ~500 bytes per file on average

## License

MIT License - See [LICENSE](LICENSE) for details.

## Contributing

Contributions are welcome! Please open an issue or submit a pull request.
