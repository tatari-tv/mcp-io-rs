use std::path::PathBuf;

use log::{LevelFilter, debug};
use rmcp::service::QuitReason;
use rmcp::transport::IntoTransport;
use rmcp::{RoleServer, ServerHandler, ServiceExt};

use crate::{Error, Result};

/// Default file-log verbosity. DEBUG so the function-level entry/exit story
/// (the logging rule) is actually captured: a local stdio MCP is low-volume, and
/// the `mcp` subcommand carries no `--log-level` flag to turn it up when needed,
/// so DEBUG-to-a-file is the right default.
const DEFAULT_LOG_LEVEL: LevelFilter = LevelFilter::Debug;

/// Route the `log` facade to a FILE under the XDG data dir, keyed by bin name
/// (`<xdg-data>/<bin>/logs/<bin>.log`), and return the resolved path.
///
/// This is the whole logging discipline of the crate: once [`serve`] is running,
/// stdout IS the JSON-RPC protocol channel, so NOTHING may write log output
/// there (nor to stderr, which a supervising client may also capture). Routing
/// `log` to a file is what keeps the crate honest under
/// `#![deny(clippy::print_stdout)]`. The host calls this (via `McpCmd::run`)
/// BEFORE serving.
///
/// Idempotent: a second call (e.g. across tests sharing one process) is a no-op
/// that still returns the resolved path, since the global logger can only be
/// installed once. The level is fixed to [`DEFAULT_LOG_LEVEL`]; `RUST_LOG` is
/// deliberately never consulted (house rule).
pub fn init_logging(bin: &str) -> Result<PathBuf> {
    let log_dir = crate::config::xdg_data_dir()
        .ok_or_else(|| Error::LogPath(bin.to_string()))?
        .join(bin)
        .join("logs");
    std::fs::create_dir_all(&log_dir)?;
    let log_file = log_dir.join(format!("{bin}.log"));

    let target = Box::new(std::fs::OpenOptions::new().create(true).append(true).open(&log_file)?);

    // `try_init` (not `init`) so a repeated call never panics; the first install
    // wins and later calls are a no-op.
    let already_initialized = env_logger::Builder::new()
        .filter_level(DEFAULT_LOG_LEVEL)
        .target(env_logger::Target::Pipe(target))
        .try_init()
        .is_err();

    debug!(
        "init_logging: bin={bin} log_file={} already_initialized={already_initialized}",
        log_file.display()
    );
    Ok(log_file)
}

/// The generic stdio serve seam the library owns. Brings the host's
/// `ServerHandler` up on the stdio transport, blocks until the client
/// disconnects, and logs the quit reason.
///
/// stdout is the JSON-RPC protocol channel here: the caller MUST have routed
/// logging to a file (via [`init_logging`]) first, or protocol frames and log
/// lines collide on stdout and corrupt the stream.
pub async fn serve<H: ServerHandler + Send + 'static>(handler: H) -> Result<()> {
    debug!("serve: handler={}", std::any::type_name::<H>());
    let quit_reason = serve_with(handler, (tokio::io::stdin(), tokio::io::stdout())).await?;
    debug!("serve: clean shutdown, quit_reason={quit_reason:?}");
    Ok(())
}

/// The transport-generic core of [`serve`]. Split out so tests can drive a fake
/// handler over an in-memory transport (an in-process duplex pipe) instead of the
/// process's real stdin/stdout, and assert the [`QuitReason`] directly. Production
/// [`serve`] delegates here over the real `(stdin, stdout)` pair, so the public
/// seam stays exactly as the design specifies.
async fn serve_with<H, T, E, A>(handler: H, transport: T) -> Result<QuitReason>
where
    H: ServerHandler + Send + 'static,
    T: IntoTransport<RoleServer, E, A>,
    E: std::error::Error + Send + Sync + 'static,
{
    debug!("serve_with: handler={}", std::any::type_name::<H>());
    let service = handler.serve(transport).await.map_err(|e| Error::Serve(Box::new(e)))?;
    debug!("serve_with: server up, waiting for client requests");
    let quit_reason = service.waiting().await?;
    debug!("serve_with: client disconnected, quit_reason={quit_reason:?}");
    Ok(quit_reason)
}

#[cfg(test)]
mod tests;
