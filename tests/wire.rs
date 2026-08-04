//! Wire regression: pin the EXACT JSON-RPC response frames `serve` emits for
//! `initialize`, `tools/list` and `tools/call`, so a future rmcp bump that changes
//! the protocol output fails CI instead of shipping.
//!
//! Before this test that guarantee was a `md5sum` run by hand against
//! `examples/spike.rs` (recorded in the rmcp 3.1 cutover design doc as
//! `40e80ec22e610409bd519fd3df9b0089`). A hand-run checksum rots, and the example
//! is labelled throwaway in its own header, so the permanent guarantee lives here
//! in `tests/` instead and hangs off a handler this file owns.
//!
//! **The `protocolVersion` below is pinned BELOW 2026-07-28 on purpose.** rmcp 3.1
//! carries two lifecycles and picks one by negotiated version
//! (`uses_legacy_lifecycle`, `rmcp-3.1.0/src/service.rs`). On the legacy path it
//! calls `strip_result_type_for_legacy_peer()` and the frames look exactly as
//! asserted here; at 2026-07-28 and above it deliberately emits an extra
//! `resultType: "complete"`. Without the pin, a harness change that crossed the
//! threshold would fail the byte assertion for a reason having nothing to do with a
//! regression, and would read as "rmcp broke our wire" when it did not.
//!
//! Like `tests/stdout.rs`, this is the SOLE test in its own integration binary: it
//! redirects the process's real stdout (fd 1) to capture what the crate emits, and
//! the libtest harness writes each test's "... ok" progress line to that same real
//! stdout. A sibling test finishing during the capture window would corrupt the
//! assertion.

#![allow(clippy::unwrap_used)]

use std::fs::File;
use std::os::fd::AsRawFd;

use mcp_io::{init_logging, serve};
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerInfo};
use rmcp::{ErrorData as McpError, ServerHandler, tool, tool_handler, tool_router};

/// Server identity is pinned to LITERALS, deliberately not `env!("CARGO_PKG_VERSION")`.
/// This test asserts the shape of the wire, not the crate's version, so a routine
/// version bump must not have to re-baseline the expected frames.
const SERVER_NAME: &str = "wire-test";
const SERVER_VERSION: &str = "0.0.0";
const INSTRUCTIONS: &str = "Wire regression server. One tool: ping.";
const TOOL_DESCRIPTION: &str = "Trivial liveness check. Takes no parameters; returns the string \"pong\".";

/// The negotiated protocol version. MUST stay below 2026-07-28; see the module docs.
const PROTOCOL_VERSION: &str = "2025-06-18";

/// A one-tool server, so `tools/list` has something to advertise and `tools/call`
/// has something to invoke. Mirrors the shape a real host (slack-cli, clyde,
/// persona-cli, marquee/cli) implements against this crate.
#[derive(Clone)]
struct WireServer;

#[tool_router]
impl WireServer {
    #[tool(description = "Trivial liveness check. Takes no parameters; returns the string \"pong\".")]
    async fn ping(&self) -> Result<CallToolResult, McpError> {
        Ok(CallToolResult::success(vec![ContentBlock::text("pong")]))
    }
}

#[tool_handler]
impl ServerHandler for WireServer {
    fn get_info(&self) -> ServerInfo {
        // `.with_server_info` is mandatory: rmcp's `Implementation::from_build_env()`
        // expands `env!("CARGO_CRATE_NAME")` inside rmcp itself, so a handler that
        // skips this reports its name to the client as "rmcp". See the README.
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(SERVER_NAME, SERVER_VERSION))
            .with_instructions(INSTRUCTIONS.to_string())
    }
}

fn request(id: u32, method: &str) -> String {
    format!("{{\"jsonrpc\":\"2.0\",\"id\":{id},\"method\":\"{method}\",\"params\":{{}}}}\n")
}

#[test]
fn serve_emits_the_pinned_jsonrpc_frames() {
    let dir = tempfile::TempDir::new().unwrap();

    // Route logging to a file under a temp XDG dir. A log line leaking to stdout
    // would land in the captured frames and fail the assertions below.
    let prior_xdg = std::env::var("XDG_DATA_HOME").ok();
    unsafe { std::env::set_var("XDG_DATA_HOME", dir.path()) };
    let _log_file = init_logging("mcp-io-wire-test").unwrap();

    // The full four-frame handshake from the design doc: initialize (at the pinned
    // protocol version), initialized, tools/list, tools/call. EOF then shuts down.
    let mut input = String::new();
    input.push_str(&format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{{\"protocolVersion\":\"{PROTOCOL_VERSION}\",\"capabilities\":{{}},\"clientInfo\":{{\"name\":\"wire-client\",\"version\":\"0.0.0\"}}}}}}\n"
    ));
    input.push_str("{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n");
    input.push_str(&request(2, "tools/list"));
    input.push_str(
        "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"ping\",\"arguments\":{}}}\n",
    );

    let in_path = dir.path().join("stdin");
    let out_path = dir.path().join("stdout");
    std::fs::write(&in_path, input.as_bytes()).unwrap();

    let in_file = File::open(&in_path).unwrap();
    let out_file = File::create(&out_path).unwrap();

    // Redirect the process's real fd 0 / fd 1 to the files, saving the originals.
    // `serve` writes JSON-RPC to the true fd 1, so this capture is faithful.
    let (saved_in, saved_out) = unsafe {
        let si = libc::dup(0);
        let so = libc::dup(1);
        libc::dup2(in_file.as_raw_fd(), 0);
        libc::dup2(out_file.as_raw_fd(), 1);
        (si, so)
    };

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = runtime.block_on(serve(WireServer));
    // Drop the runtime while the fds are still redirected so trailing writes flush
    // to the captured file rather than to the restored stdout.
    drop(runtime);

    // Restore the real fds BEFORE any assert that might print on failure.
    unsafe {
        libc::dup2(saved_in, 0);
        libc::dup2(saved_out, 1);
        libc::close(saved_in);
        libc::close(saved_out);
    }
    match prior_xdg {
        Some(v) => unsafe { std::env::set_var("XDG_DATA_HOME", v) },
        None => unsafe { std::env::remove_var("XDG_DATA_HOME") },
    }

    result.expect("serve should exit Ok on a clean client disconnect");

    let captured = String::from_utf8(std::fs::read(&out_path).unwrap()).expect("stdout must be utf-8");
    let frames: Vec<serde_json::Value> = captured
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap_or_else(|e| panic!("non-JSON line on stdout: {l:?} ({e})")))
        .collect();

    assert_eq!(
        frames.len(),
        3,
        "expected exactly 3 response frames (initialize, tools/list, tools/call), got {}: {captured}",
        frames.len()
    );

    // Look each response up by its JSON-RPC `id`, NEVER by arrival position. rmcp
    // dispatches each request as its own task, so `tools/list` (id 2) and
    // `tools/call` (id 3) genuinely race and either can land on stdout first --
    // an earlier draft of this test indexed `frames[1]`/`frames[2]` and failed
    // intermittently for exactly that reason. Out-of-order responses are legal
    // JSON-RPC, so matching on `id` is the correct semantics, not a workaround.
    let by_id = |id: u64| -> &serde_json::Value {
        frames
            .iter()
            .find(|f| f.get("id").and_then(serde_json::Value::as_u64) == Some(id))
            .unwrap_or_else(|| panic!("no response frame with id={id}; captured: {captured}"))
    };

    // `initialize`. Asserted whole: a new or renamed field anywhere in the result
    // (rmcp's `resultType` at protocol >= 2026-07-28, a changed capabilities shape,
    // a dropped `instructions`) fails here and names itself in the diff.
    assert_eq!(
        *by_id(1),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": SERVER_NAME, "version": SERVER_VERSION},
                "instructions": INSTRUCTIONS,
            }
        }),
        "initialize frame changed"
    );

    // `tools/list`. Pins the advertised tool name, description and input schema, so
    // a schemars or macro change that reshapes the schema fails CI.
    assert_eq!(
        *by_id(2),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": {
                "tools": [{
                    "name": "ping",
                    "description": TOOL_DESCRIPTION,
                    "inputSchema": {"type": "object", "properties": {}},
                }]
            }
        }),
        "tools/list frame changed"
    );

    // `tools/call`. This is the frame rmcp 3.x most plausibly changes: 3.1 widened
    // the return to `CallToolResponse`, whose `Complete` variant must still
    // serialize as the old `isError` + `content` pair on the legacy path.
    assert_eq!(
        *by_id(3),
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 3,
            "result": {
                "content": [{"type": "text", "text": "pong"}],
                "isError": false,
            }
        }),
        "tools/call frame changed"
    );
}
