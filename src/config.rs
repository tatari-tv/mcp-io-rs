use std::path::PathBuf;

use log::debug;

/// Host identity, captured at the host's call site by the [`crate::mcp_io!`] macro.
/// Mirrors renew's `Renew` config object: built once from the host's `CARGO_PKG_*`
/// metadata, then passed by reference into [`crate::McpCmd::run`].
#[derive(Debug, Clone)]
pub struct McpIo {
    /// Registration key under `mcpServers` in Claude config. Defaults to `bin`;
    /// override via `mcp_io!(key = "...")`.
    pub server_key: String,
    /// Host binary name (used for messages, log file naming, and the default
    /// `server_key`).
    pub bin: String,
    /// Host version (`CARGO_PKG_VERSION` at the host's call site).
    pub version: String,
}

impl McpIo {
    /// Construct directly. Prefer [`crate::mcp_io!`] at the host's call site so
    /// `bin`/`version` are captured from the HOST's Cargo metadata, not this
    /// crate's own.
    pub fn new(bin: impl Into<String>, version: impl Into<String>, server_key: Option<String>) -> Self {
        let bin = bin.into();
        let version = version.into();
        let server_key = server_key.unwrap_or_else(|| bin.clone());
        debug!("McpIo::new: bin={bin} version={version} server_key={server_key}");
        Self {
            server_key,
            bin,
            version,
        }
    }
}

/// XDG config dir, honoring `$XDG_CONFIG_HOME` and falling back to `$HOME/.config`.
/// Deliberately NOT `dirs::config_dir()`: that only honors the XDG env var on
/// Linux, and silently resolves to `~/Library/Application Support` on macOS.
pub fn xdg_config_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        let path = PathBuf::from(dir);
        if path.is_absolute() {
            return Some(path);
        }
    }
    dirs::home_dir().map(|h| h.join(".config"))
}

/// XDG data dir, honoring `$XDG_DATA_HOME` and falling back to `$HOME/.local/share`.
pub fn xdg_data_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("XDG_DATA_HOME") {
        let path = PathBuf::from(dir);
        if path.is_absolute() {
            return Some(path);
        }
    }
    dirs::home_dir().map(|h| h.join(".local").join("share"))
}

#[cfg(test)]
mod tests;
