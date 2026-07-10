use std::fs::File;
use std::io;
use std::path::PathBuf;

use log::debug;
use rmcp::model::Tool;
use rmcp::{ServerHandler, ServiceExt};
use serde::Serialize;
use zip::write::SimpleFileOptions;

use crate::error::Error;
use crate::{McpIo, Result};

/// `mcpb` manifest spec version this crate targets. Verified against
/// `modelcontextprotocol/mcpb`'s `MANIFEST.md` ("Current version: `0.3`") and
/// its schema `schemas/mcpb-manifest-v0.3.schema.json`, fetched 2026-07-09.
const MANIFEST_VERSION: &str = "0.3";

/// This crate only ever produces a compiled-binary bundle (never `node`/
/// `python`/`uv`): every host is a Rust CLI, so `binary` is the only server
/// type mcp-io needs to emit.
const SERVER_TYPE: &str = "binary";

/// Buffer size for the in-process duplex pipe used to enumerate the handler's
/// tools (see [`advertised_tools`]). Generous for a `tools/list` round trip;
/// not a hot path.
const DUPLEX_BUF_SIZE: usize = 8192;

/// No host-supplied author metadata exists yet (`McpIo` carries none, and the
/// design doesn't add one for Phase 5). Every consumer of this library is a
/// Tatari-owned CLI, so a fixed org-level author name is a reasonable default
/// pending a real seam. See the Phase 5 notes' Open Questions.
const AUTHOR_NAME: &str = "Tatari";

/// A `manifest.json`, shaped per the `mcpb` v0.3 schema
/// (`schemas/mcpb-manifest-v0.3.schema.json`, `additionalProperties: false`
/// throughout -- every field here is a schema-recognized field, nothing
/// invented).
#[derive(Debug, Serialize)]
struct Manifest {
    manifest_version: &'static str,
    name: String,
    version: String,
    description: String,
    author: Author,
    server: Server,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<ManifestTool>,
}

#[derive(Debug, Serialize)]
struct Author {
    name: &'static str,
}

#[derive(Debug, Serialize)]
struct Server {
    #[serde(rename = "type")]
    kind: &'static str,
    entry_point: String,
    mcp_config: McpConfig,
}

#[derive(Debug, Serialize)]
struct McpConfig {
    command: String,
    args: Vec<String>,
}

#[derive(Debug, Serialize)]
struct ManifestTool {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
}

/// Package a `.mcpb` bundle for Claude Desktop / Cowork: enumerate `handler`'s
/// advertised tools via a real, in-process MCP handshake (no live client
/// needed -- see [`advertised_tools`]), generate `manifest.json` from them plus
/// [`McpIo`] metadata, and zip it together with a copy of THIS build's binary
/// under `server/<bin>`.
///
/// The binary is bundled (not referenced by absolute path, unlike `register`'s
/// entry) because bundle's whole point is portability: it ships to colleagues'
/// machines that do NOT already have this CLI installed, so an absolute
/// `current_exe()` path (this machine's install location) would be useless to
/// them. This is the one place mcp-io deliberately does NOT mirror `register`.
pub async fn bundle<H: ServerHandler + Send + 'static>(
    io: &McpIo,
    out: Option<PathBuf>,
    handler: H,
) -> Result<PathBuf> {
    debug!("bundle: bin={} server_key={} out={out:?}", io.bin, io.server_key);

    let tools = advertised_tools(handler).await?;
    debug!("bundle: tool_count={}", tools.len());

    let manifest = build_manifest(io, &tools);
    let out_path = out.unwrap_or_else(|| PathBuf::from(format!("{}.mcpb", io.bin)));
    let bin = io.bin.clone();
    let write_path = out_path.clone();

    // Zipping + copying the binary is blocking filesystem work; run it off the
    // async runtime so a large binary copy never stalls other tasks.
    tokio::task::spawn_blocking(move || write_bundle(&write_path, &manifest, &bin)).await??;

    debug!("bundle: wrote {}", out_path.display());
    Ok(out_path)
}

/// Enumerate `handler`'s advertised tools via a REAL, in-process MCP
/// handshake: serve `handler` over one end of an in-memory duplex pipe,
/// connect a bare rmcp client (`()` -- `ClientHandler` is blanket-implemented,
/// `rmcp::handler::client::ClientHandler for ()`) to the other end, and call
/// `Peer::list_all_tools()`. This is the same seam Phase 0/2 proved (a real
/// stdio-shaped handshake), just run once against an in-memory transport
/// instead of the process's real stdin/stdout, and driven by rmcp's own
/// client instead of hand-written JSON-RPC frames.
///
/// Calling `ServerHandler::list_tools` directly would be simpler, but it takes
/// a `RequestContext<RoleServer>`, which can only be constructed from a
/// `Peer<RoleServer>` -- and `Peer::new` is `pub(crate)` in rmcp 2.1/2.2 (the
/// design's own finding for why the `call` verb was cut entirely). A real
/// client peer is the only generic-over-`H` way to reach the tool list.
async fn advertised_tools<H: ServerHandler + Send + 'static>(handler: H) -> Result<Vec<Tool>> {
    debug!("advertised_tools: handler={}", std::any::type_name::<H>());
    let (server_end, client_end) = tokio::io::duplex(DUPLEX_BUF_SIZE);

    // Both sides block reading their half of the `initialize` handshake before
    // `.serve()` returns (the server waits for the request, the client waits
    // for the response), so they MUST be polled concurrently -- awaiting them
    // sequentially deadlocks (the server never sees a client that hasn't
    // started yet).
    let (server_result, client_result) = tokio::join!(handler.serve(server_end), ().serve(client_end));

    let server = server_result.map_err(|e| Error::Serve(Box::new(e)))?;
    let client: rmcp::service::RunningService<rmcp::RoleClient, ()> =
        client_result.map_err(|e| Error::BundleClientInit(Box::new(e)))?;

    let tools = client
        .peer()
        .list_all_tools()
        .await
        .map_err(|e| Error::BundleListTools(Box::new(e)))?;

    // Best-effort teardown: the tool list is already captured, so a shutdown
    // hiccup on either side is not fatal to bundle().
    let _ = client.cancel().await;
    let _ = server.cancel().await;

    debug!("advertised_tools: tool_count={}", tools.len());
    Ok(tools)
}

/// Build the `manifest.json` contents from `io` and the handler's advertised
/// `tools`. `entry_point`/`mcp_config.command` both point at `server/<bin>`,
/// the path the binary is packaged under (see [`write_bundle`]); `args`
/// mirrors the exact registration entry (`["mcp", "serve"]`).
fn build_manifest(io: &McpIo, tools: &[Tool]) -> Manifest {
    let bin = io.bin.clone();
    let packaged_path = format!("server/{bin}");
    Manifest {
        manifest_version: MANIFEST_VERSION,
        name: bin.clone(),
        version: io.version.clone(),
        description: format!("MCP server for {bin}, via mcp-io"),
        author: Author { name: AUTHOR_NAME },
        server: Server {
            kind: SERVER_TYPE,
            entry_point: packaged_path.clone(),
            mcp_config: McpConfig {
                command: packaged_path,
                args: vec!["mcp".to_string(), "serve".to_string()],
            },
        },
        tools: tools
            .iter()
            .map(|t| ManifestTool {
                name: t.name.to_string(),
                description: t.description.as_ref().map(|d| d.to_string()),
            })
            .collect(),
    }
}

/// Synchronous, blocking half of [`bundle`]: serialize `manifest`, resolve
/// THIS build's binary path, and zip both into `out_path` as `manifest.json` +
/// `server/<bin>` (executable bit preserved on unix).
fn write_bundle(out_path: &std::path::Path, manifest: &Manifest, bin: &str) -> Result<()> {
    debug!("write_bundle: out_path={}", out_path.display());
    let manifest_json = serde_json::to_vec_pretty(manifest)?;

    let exe_path = std::env::current_exe().map_err(Error::CurrentExe)?;
    let mut exe_file = File::open(&exe_path)?;

    let out_file = File::create(out_path)?;
    let mut zip = zip::ZipWriter::new(out_file);

    zip.start_file("manifest.json", SimpleFileOptions::default())?;
    io::Write::write_all(&mut zip, &manifest_json)?;

    // `Stored` (no compression) for the binary: compiled executables barely
    // compress anyway, and skipping deflate keeps `bundle` fast even for a
    // large binary (this dominated the test suite's wall-clock before the
    // change -- see the Phase 5 notes' Tradeoffs).
    let mut options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    #[cfg(unix)]
    {
        options = options.unix_permissions(0o755);
    }
    zip.start_file(format!("server/{bin}"), options)?;
    io::copy(&mut exe_file, &mut zip)?;

    zip.finish()?;
    debug!(
        "write_bundle: wrote {} bytes of manifest + {}",
        manifest_json.len(),
        exe_path.display()
    );
    Ok(())
}

#[cfg(test)]
mod tests;
