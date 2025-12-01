//! Linux search backend using Tracker3 (GNOME) via D-Bus
//!
//! TODO: Implement Tracker3 SPARQL queries over D-Bus.
//! For now, returns not available so daemon (SQLite) is used.

/// Check if Tracker3 is available
pub fn is_available() -> bool {
    // TODO: Check if Tracker3 is running via D-Bus
    // For now, return false to fall back to SQLite daemon
    false
}

/// Search using Tracker3 (placeholder)
pub fn search(
    _query: &str,
    _max_results: u32,
    _extension: Option<&str>,
) -> Result<String, Box<dyn std::error::Error>> {
    Err("Tracker3 search not implemented yet".into())
}
