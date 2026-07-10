#![allow(clippy::unwrap_used)]

use rmcp::ServerHandler;
use rmcp::service::QuitReason;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;

/// A do-nothing handler. rmcp defaults every `ServerHandler` method, so a bare
/// impl is a complete (if minimal) MCP server: it answers `initialize` and
/// `tools/list` (with zero tools). Enough to exercise the full serve lifecycle.
struct DummyHandler;
impl ServerHandler for DummyHandler {}

const INIT: &[u8] = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-06-18\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test-client\",\"version\":\"0.0.0\"}}}\n";
const INITIALIZED: &[u8] = b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n";

/// Success criterion #1: drive the fake handler through the serve lifecycle over
/// an in-memory transport and assert a CLEAN shutdown when the client disconnects
/// (EOF) -- `waiting()` returns and the quit reason is `Closed`.
///
/// (The stdout-discipline test -- Acceptance Criterion #4 -- lives in its own
/// integration binary, `tests/stdout.rs`, because it redirects the process's real
/// fd 1 and cannot share a harness with other tests that also print there.)
#[tokio::test]
async fn test_serve_with_clean_shutdown_on_client_disconnect() {
    // client_out -> server_in : the client writes requests, the server reads them.
    let (mut client_out, server_in) = tokio::io::duplex(8192);
    // server_out -> client_in : the server writes responses, the client reads them.
    let (server_out, mut client_in) = tokio::io::duplex(8192);

    let handle = tokio::spawn(async move { serve_with(DummyHandler, (server_in, server_out)).await });

    // Drain server responses so a full write buffer can never stall the server.
    let drain = tokio::spawn(async move {
        let mut buf = Vec::new();
        let _ = client_in.read_to_end(&mut buf).await;
        buf
    });

    client_out.write_all(INIT).await.unwrap();
    client_out.write_all(INITIALIZED).await.unwrap();
    // Close the client's write half -> EOF on server_in -> clean shutdown.
    drop(client_out);

    let quit = handle.await.unwrap().unwrap();
    assert!(
        matches!(quit, QuitReason::Closed),
        "expected a clean Closed shutdown on client disconnect, got {quit:?}"
    );

    let responses = drain.await.unwrap();
    assert!(
        !responses.is_empty(),
        "server should have written at least the initialize response frame"
    );
}
