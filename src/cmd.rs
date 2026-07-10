use std::fmt::Display;
use std::path::PathBuf;

use clap::{Args, Subcommand};
use log::debug;
use rmcp::ServerHandler;

use crate::McpIo;
use crate::register::Target;

/// Process exit codes the host `std::process::exit`s (renew's contract).
const EXIT_SUCCESS: i32 = 0;
const EXIT_FAILURE: i32 = 1;

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
            McpSub::Status => crate::register::status(io, build),
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

    // Route logging to a file BEFORE anything touches stdout: once serve runs,
    // stdout is the JSON-RPC protocol channel. Errors here go to stderr, never
    // stdout (which the crate-level `deny(clippy::print_stdout)` also forbids).
    let log_file = match crate::serve::init_logging(&io.bin) {
        Ok(path) => path,
        Err(e) => {
            eprintln!("{}: failed to initialize logging: {e}", io.bin);
            return EXIT_FAILURE;
        }
    };
    debug!("run_serve: logging routed to {}", log_file.display());

    // Construct the host's handler only now (serve is a build-requiring verb).
    let handler = match build() {
        Ok(handler) => handler,
        Err(e) => {
            log::error!("run_serve: handler construction failed: {e}");
            eprintln!("{}: failed to build MCP server: {e}", io.bin);
            return EXIT_FAILURE;
        }
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(e) => {
            log::error!("run_serve: failed to build tokio runtime: {e}");
            eprintln!("{}: failed to start async runtime: {e}", io.bin);
            return EXIT_FAILURE;
        }
    };

    match runtime.block_on(crate::serve::serve(handler)) {
        Ok(()) => {
            debug!("run_serve: serve exited cleanly");
            EXIT_SUCCESS
        }
        Err(e) => {
            log::error!("run_serve: serve failed: {e}");
            eprintln!("{}: mcp serve failed: {e}", io.bin);
            EXIT_FAILURE
        }
    }
}

fn run_bundle<H, F, E>(io: &McpIo, out: Option<PathBuf>, build: F) -> i32
where
    H: ServerHandler + Send + 'static,
    F: FnOnce() -> Result<H, E>,
    E: Display,
{
    debug!("run_bundle: bin={} out={out:?}", io.bin);

    // Bundle is a build-requiring verb (like serve): the manifest's tool list
    // comes from the REAL built handler, not a guess.
    let handler = match build() {
        Ok(handler) => handler,
        Err(e) => {
            log::error!("run_bundle: handler construction failed: {e}");
            eprintln!("{}: failed to build MCP server: {e}", io.bin);
            return EXIT_FAILURE;
        }
    };

    let runtime = match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(e) => {
            log::error!("run_bundle: failed to build tokio runtime: {e}");
            eprintln!("{}: failed to start async runtime: {e}", io.bin);
            return EXIT_FAILURE;
        }
    };

    match runtime.block_on(crate::bundle::bundle(io, out, handler)) {
        Ok(path) => {
            eprintln!("{}: wrote bundle to {}", io.bin, path.display());
            debug!("run_bundle: wrote bundle to {}", path.display());
            EXIT_SUCCESS
        }
        Err(e) => {
            log::error!("run_bundle: bundle failed: {e}");
            eprintln!("{}: mcp bundle failed: {e}", io.bin);
            EXIT_FAILURE
        }
    }
}

#[cfg(test)]
mod tests;
