//! Phase 0 spike (THROWAWAY): prove the one foundational seam the whole `mcp-io`
//! library rests on -- a generic `serve<H: rmcp::ServerHandler>(handler)` that
//! compiles AND handshakes over stdio on rmcp 2.1.0.
//!
//! This is NOT product code. It exists only to de-risk the seam before Phase 1
//! scaffolds the real crate. Phase 2 reproduces the exact `serve<H>` shape proven
//! here as production code.
//!
//! Headless proof (a real stdio MCP handshake, no GUI Inspector):
//!
//! ```text
//! printf '%s\n' \
//!   '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"spike-client","version":"0.0.0"}}}' \
//!   '{"jsonrpc":"2.0","method":"notifications/initialized"}' \
//!   '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' \
//!   | cargo run --example spike
//! ```
//!
//! stdout is the JSON-RPC protocol channel: nothing writes to it but rmcp's own
//! framing. The spike logs its lifecycle to STDERR so the handshake stays clean.

use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerInfo};
use rmcp::{ErrorData as McpError, ServerHandler, ServiceExt, tool, tool_handler, tool_router};

/// A dummy 1-tool server. The whole point of the spike is that the library never
/// needs to know this concrete type: `serve<H>` below is generic over it.
#[derive(Clone)]
struct SpikeServer;

#[tool_router]
impl SpikeServer {
    /// The one trivial tool. No parameters; returns a constant string so the
    /// `tools/list` handshake has exactly one tool to advertise.
    #[tool(description = "Trivial liveness check. Takes no parameters; returns the string \"pong\".")]
    async fn ping(&self) -> Result<CallToolResult, McpError> {
        eprintln!("SpikeServer::ping: no params");
        Ok(CallToolResult::success(vec![ContentBlock::text("pong")]))
    }
}

#[tool_handler]
impl ServerHandler for SpikeServer {
    fn get_info(&self) -> ServerInfo {
        eprintln!("SpikeServer::get_info: MCP client requested server info");
        // The `rmcp`-reports-itself gotcha: `Implementation::from_build_env()` (what
        // `ServerInfo::new` defaults to) expands `env!("CARGO_CRATE_NAME")` INSIDE
        // rmcp's own crate, so the server would report itself as "rmcp" unless the
        // host sets `.with_server_info(...)` explicitly. The whole library docs will
        // require the host to do this; the spike proves the seam.
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("spike", env!("CARGO_PKG_VERSION")))
            .with_instructions("Phase 0 spike server. One tool: ping.".to_string())
    }
}

/// The generic seam the library will own. It is generic over ANY host
/// `ServerHandler`, knows nothing about the concrete tools, and runs the full
/// stdio lifecycle: bring the service up on (stdin, stdout), block until the
/// client disconnects, and surface the quit reason.
///
/// All logging goes to STDERR because stdout is the JSON-RPC protocol channel.
async fn serve<H: ServerHandler + Send + 'static>(handler: H) -> eyre::Result<()> {
    eprintln!("serve: bringing MCP server up on stdio transport");
    let service = handler.serve((tokio::io::stdin(), tokio::io::stdout())).await?;
    eprintln!("serve: MCP server started, waiting for client requests");

    let quit_reason = service.waiting().await?;
    eprintln!("serve: client disconnected, shutting down (reason={quit_reason:?})");
    Ok(())
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    serve(SpikeServer).await
}
