mod claude;
mod desktop;

use clap::ValueEnum;
use log::debug;

use crate::McpIo;

/// Claude Code / Claude Desktop registration target. Selects which mechanism
/// `register`/`unregister`/`status` use: Claude Code `user`/`project` delegate
/// to `claude mcp add-json` (opaque-safe, idempotent); `desktop` is a direct
/// Value-preserving atomic write (no `claude` CLI in a Desktop-only install).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub enum Target {
    /// Claude Code user scope (`~/.claude.json`, honoring `$CLAUDE_CONFIG_DIR`).
    User,
    /// Claude Code project scope (`./.mcp.json`).
    Project,
    /// Claude Desktop config (macOS; Linux community build).
    Desktop,
}

/// Register this build's `server_key` -> `current_exe()` entry into `target`.
/// Returns a process exit code (renew's contract).
pub fn register(io: &McpIo, target: Target) -> i32 {
    debug!(
        "register: bin={} server_key={} target={target:?}",
        io.bin, io.server_key
    );
    match target {
        Target::User | Target::Project => claude::register(io, target),
        Target::Desktop => desktop::register(io),
    }
}

/// Remove this build's entry from `target`.
pub fn unregister(io: &McpIo, target: Target) -> i32 {
    debug!(
        "unregister: bin={} server_key={} target={target:?}",
        io.bin, io.server_key
    );
    match target {
        Target::User | Target::Project => claude::unregister(io, target),
        Target::Desktop => desktop::unregister(io),
    }
}

/// Report where this server is registered across all targets, and whether the
/// host's handshake name matches `bin` (the rmcp-reports-itself-as-"rmcp" gotcha).
pub fn status(io: &McpIo) -> i32 {
    debug!("status: bin={} server_key={}", io.bin, io.server_key);
    todo!("Phase 3: status orchestration across all targets")
}

#[cfg(test)]
mod tests;
