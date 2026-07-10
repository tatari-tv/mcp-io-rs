//! Acceptance Criterion #4: `serve` writes ONLY JSON-RPC frames to stdout; all
//! logging goes to the file target.
//!
//! This is isolated in its OWN integration binary on purpose. The test redirects
//! the process's real stdout (fd 1) to a file to capture exactly what the crate
//! emits. The libtest harness also writes each test's "... ok" progress line to
//! the real stdout, so any SIBLING test completing during the capture window
//! would land its status line in our captured file and corrupt the assertion.
//! As the sole test in this binary, nothing else prints to fd 1 while the
//! redirect is in effect.
//!
//! Faithful capture is the point: a log line misrouted to stdout (instead of the
//! file target) would land in the captured file and fail the frame check below.
//! Break `init_logging` to `env_logger::Target::Stdout` and this test fails.

#![allow(clippy::unwrap_used)]

use std::fs::File;
use std::os::fd::AsRawFd;

use mcp_io::{init_logging, serve};
use rmcp::ServerHandler;

/// A do-nothing handler: rmcp defaults every `ServerHandler` method, so this is a
/// complete minimal MCP server (answers `initialize` / `tools/list`).
struct DummyHandler;
impl ServerHandler for DummyHandler {}

const INIT: &[u8] = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-06-18\",\"capabilities\":{},\"clientInfo\":{\"name\":\"test-client\",\"version\":\"0.0.0\"}}}\n";
const INITIALIZED: &[u8] = b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n";

#[test]
fn serve_writes_only_jsonrpc_frames_to_stdout() {
    let dir = tempfile::TempDir::new().unwrap();

    // Point the XDG data dir at a temp dir so init_logging writes its file there,
    // letting us prove the log lines land in the FILE (not on stdout).
    let prior_xdg = std::env::var("XDG_DATA_HOME").ok();
    unsafe { std::env::set_var("XDG_DATA_HOME", dir.path()) };
    let log_file = init_logging("mcp-io-test").unwrap();

    // A real initialize + initialized handshake in a file; EOF at end -> shutdown.
    let in_path = dir.path().join("stdin");
    let mut input = Vec::new();
    input.extend_from_slice(INIT);
    input.extend_from_slice(INITIALIZED);
    std::fs::write(&in_path, &input).unwrap();
    let out_path = dir.path().join("stdout");

    let in_file = File::open(&in_path).unwrap();
    let out_file = File::create(&out_path).unwrap();

    // Redirect the process's real stdin (fd 0) and stdout (fd 1) to the files,
    // saving the originals. `serve` writes JSON-RPC to the true fd 1, so this
    // capture is faithful: a stdout log leak would be caught by the frame check.
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
    let result = runtime.block_on(serve(DummyHandler));
    // Drop the runtime while fds are still redirected so trailing writes flush to
    // the captured file, not the restored stdout.
    drop(runtime);

    // Restore the real fds before any assert/unwrap that might print on failure.
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
    assert!(!captured.trim().is_empty(), "serve wrote nothing to stdout");

    // EVERY non-empty stdout line must be a JSON-RPC 2.0 frame. A log line would
    // not parse as JSON and would fail here.
    for line in captured.lines().filter(|l| !l.trim().is_empty()) {
        let value: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("non-JSON line on stdout (log leak?): {line:?} ({e})"));
        assert_eq!(
            value.get("jsonrpc").and_then(|v| v.as_str()),
            Some("2.0"),
            "stdout line is not a JSON-RPC 2.0 frame: {line:?}"
        );
    }

    // And the lifecycle logging DID happen -- to the file, not to stdout.
    let logged = std::fs::read_to_string(&log_file).unwrap();
    assert!(
        logged.contains("serve"),
        "expected serve lifecycle logging in the file target, got: {logged:?}"
    );
}
