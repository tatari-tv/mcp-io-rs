use std::fmt::Display;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use log::debug;
use rmcp::ServerHandler;

use crate::McpIo;
use crate::register::Target;

/// The clap type the host embeds as one variant of its own `Command` enum
/// (`Mcp(mcp_io::McpCmd)`), mirroring `renew::UpdateCmd`.
#[derive(Args, Debug)]
pub struct McpCmd {
    #[command(subcommand)]
    cmd: McpSub,
}

#[derive(Subcommand, Debug)]
enum McpSub {
    /// Serve the host's MCP tools over stdio.
    Serve,
    /// Register this build into a Claude config target.
    Register {
        #[arg(long, value_enum, default_value = "user", ignore_case = true)]
        target: Target,
    },
    /// Remove this server's entry from a Claude config target.
    Unregister {
        #[arg(long, value_enum, default_value = "user", ignore_case = true)]
        target: Target,
    },
    /// Report where this server is registered.
    Status,
    /// Package a `.mcpb` bundle for Claude Desktop / Cowork.
    Bundle {
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

impl McpCmd {
    /// `build` constructs the host's `ServerHandler`. Called ONLY for
    /// `serve`/`bundle`; `register`/`unregister`/`status` never build it, so
    /// they need no token/login. Returns a process exit code; the host
    /// `std::process::exit`s it (renew's contract).
    pub fn run<H, F, E>(&self, io: &McpIo, build: F) -> i32
    where
        H: ServerHandler + Send + 'static,
        F: FnOnce() -> Result<H, E>,
        E: Display,
    {
        debug!(
            "McpCmd::run: bin={} server_key={} cmd={:?}",
            io.bin, io.server_key, self.cmd
        );
        match &self.cmd {
            McpSub::Serve => run_serve(io, build),
            McpSub::Register { target } => crate::register::register(io, *target),
            McpSub::Unregister { target } => crate::register::unregister(io, *target),
            McpSub::Status => crate::register::status(io),
            McpSub::Bundle { out } => run_bundle(io, out.clone(), build),
        }
    }
}

fn run_serve<H, F, E>(io: &McpIo, build: F) -> i32
where
    H: ServerHandler + Send + 'static,
    F: FnOnce() -> Result<H, E>,
    E: Display,
{
    debug!("run_serve: bin={}", io.bin);
    let _ = build;
    todo!("Phase 2: build a tokio runtime, construct the handler via `build`, call crate::serve")
}

fn run_bundle<H, F, E>(io: &McpIo, out: Option<PathBuf>, build: F) -> i32
where
    H: ServerHandler + Send + 'static,
    F: FnOnce() -> Result<H, E>,
    E: Display,
{
    debug!("run_bundle: bin={} out={out:?}", io.bin);
    let _ = build;
    crate::bundle::bundle(io, out)
}

#[cfg(test)]
mod tests;
