# mcp-io-rs implementation notes

Append-only. One section per phase. Companion to
`docs/design/2026-07-09-mcp-io-rs.md`.

## Phase 0: Prove the generic serve seam (spike, throwaway)

### Design decisions
- Proved the seam with a real headless stdio handshake, not the GUI MCP Inspector -- `examples/spike.rs` -- piping an `initialize` + `notifications/initialized` + `tools/list` JSON-RPC sequence into `cargo run --example spike` over stdin. Reproducible in CI/headless, no manual client. The doc offered "MCP Inspector or `claude mcp`"; a piped JSON-RPC sequence is a real stdio MCP client and is deterministic.
- Generic seam shape (this is the exact pattern Phase 2 reproduces as production code):
  - Imports:
    ```rust
    use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerInfo};
    use rmcp::{ErrorData as McpError, ServerHandler, ServiceExt, tool, tool_handler, tool_router};
    ```
  - Generic function signature (`serve<H>` in `examples/spike.rs`):
    ```rust
    async fn serve<H: ServerHandler + Send + 'static>(handler: H) -> eyre::Result<()> {
        let service = handler.serve((tokio::io::stdin(), tokio::io::stdout())).await?;
        let quit_reason = service.waiting().await?;
        // quit_reason is a QuitReason; on stdin EOF it is `Closed`.
        Ok(())
    }
    ```
    `.serve(...)` comes from the `ServiceExt` trait (must be in scope). Lifecycle is `handler.serve((stdin, stdout)).await` -> `service.waiting().await` -> `Ok(quit_reason)`; `waiting()` returns the quit reason by value.
  - `.with_server_info` call (defeats the rmcp-reports-itself-as-"rmcp" gotcha), inside `get_info` on the `#[tool_handler] impl ServerHandler`:
    ```rust
    ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
        .with_server_info(Implementation::new("spike", env!("CARGO_PKG_VERSION")))
        .with_instructions("...".to_string())
    ```
  - Tool router wiring: `#[tool_router]` on the tools `impl` generates `Self::tool_router()`; `#[tool_handler]` on the `impl ServerHandler` picks it up automatically -- no `tool_router` struct field required (confirmed against `persona-cli/src/mcp.rs`, which has no such field).
- Success `CallToolResult` uses `ContentBlock::text(...)` (rmcp 2.1.0 has no `Content` re-export in `rmcp::model`; the type is `ContentBlock`, matching `persona-cli`).
- All spike logging goes to STDERR (`eprintln!`) because stdout is the JSON-RPC protocol channel. Verified: stdout carried only two JSON-RPC frames; the four lifecycle lines landed on stderr.

### Deviations
- `serve<H>` returns `eyre::Result<()>` in the spike; the production library will return the crate's `thiserror` `Result`. Correct for a throwaway example (eyre is a dev-dependency here, per the doc); Phase 2 swaps the error type. Same effect, correct seam.
- Verification method is a piped JSON-RPC handshake rather than the MCP Inspector named in the doc's Phase 0 bullet. Same protocol, headless and deterministic; lists exactly the dummy tool as the criterion requires.
- Package `edition = "2021"` and `version = "0.0.0"` for the spike crate (per the Phase 0 task brief). Phase 1 scaffolds the real crate; the fleet standard is edition 2024, so Phase 1 should set edition 2024 and a real starting version.

### Tradeoffs
- Piped-stdin handshake vs a spawned MCP client library: the pipe is zero extra deps and CI-friendly; a client lib would exercise more of the protocol but is unnecessary to de-risk the one seam (generic serve compiles + handshakes + lists the tool). Chose the pipe.

### Open questions
- None.
