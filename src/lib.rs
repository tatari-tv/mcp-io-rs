#![deny(clippy::unwrap_used)]
#![deny(clippy::print_stdout)]
#![deny(dead_code)]
#![deny(unused_variables)]

//! `mcp-io`: shared scaffolding for a local stdio MCP server, the same way `renew`
//! gives a CLI its `update` subcommand. The host supplies its own rmcp
//! `ServerHandler` (its tools); this crate owns the `mcp` subcommand surface,
//! stdio + logging discipline, self-registration into Claude config, and a
//! `.mcpb` bundle. `#![deny(clippy::print_stdout)]` because stdout IS the
//! JSON-RPC protocol channel once `serve` is running - all logging goes through
//! `log`, never `println!`.
//!
//! See `docs/design/2026-07-09-mcp-io-rs.md` in `tatari-tv/mcp-io-rs` for the
//! full design.
//!
//! `mcp_io!()` captures the CONSUMER's `CARGO_PKG_NAME`/`CARGO_PKG_VERSION` at
//! the host's call site (macro expansion happens during the host's own
//! compilation), mirroring `renew!()`:
//!
//! ```
//! let io = mcp_io::mcp_io!();
//! assert_eq!(io.bin, env!("CARGO_PKG_NAME"));
//! assert_eq!(io.version, env!("CARGO_PKG_VERSION"));
//! assert_eq!(io.server_key, io.bin);
//!
//! let io = mcp_io::mcp_io!(key = "example");
//! assert_eq!(io.server_key, "example");
//! ```

mod bundle;
mod cmd;
mod config;
mod error;
mod register;
mod serve;

pub use cmd::McpCmd;
pub use config::{McpIo, xdg_config_dir, xdg_data_dir};
pub use error::{Error, Result};
pub use serve::serve;

/// Construct an [`McpIo`] from the HOST's crate metadata, mirroring `renew!()`.
///
/// The no-arg form uses `CARGO_PKG_NAME` as both the binary name and the
/// registration `server_key`:
///
/// ```ignore
/// let io = mcp_io::mcp_io!();               // server_key = bin = CARGO_PKG_NAME
/// let io = mcp_io::mcp_io!(key = "slack");   // override the server key
/// ```
#[macro_export]
macro_rules! mcp_io {
    () => {
        $crate::McpIo::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"), None)
    };
    (key = $key:expr) => {
        $crate::McpIo::new(
            env!("CARGO_PKG_NAME"),
            env!("CARGO_PKG_VERSION"),
            Some($key.to_string()),
        )
    };
}
