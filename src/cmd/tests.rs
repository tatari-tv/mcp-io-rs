#![allow(clippy::unwrap_used)]

use clap::Parser;

use super::*;

/// A do-nothing handler. `rmcp::ServerHandler` defaults every method, so this
/// is sufficient to exercise `McpCmd::run`'s dispatch without pulling in the
/// `#[tool_router]`/`#[tool_handler]` machinery (that lands with a real handler
/// in Phase 2/4).
struct DummyHandler;
impl ServerHandler for DummyHandler {}

fn build_dummy() -> Result<DummyHandler, String> {
    Ok(DummyHandler)
}

fn make_io() -> McpIo {
    McpIo::new("test", "0.0.0", None)
}

// The `serve` arm is no longer a stub as of Phase 2: it routes logging to a
// file, builds a tokio runtime, and runs `crate::serve::serve`. Its behavior is
// covered by the serve lifecycle + stdout-discipline tests in `src/serve/tests.rs`
// (driving a fake handler over a transport is the right seam, not `McpCmd::run`,
// which would seize the process's real stdin/stdout).

// Inverted from the Phase-1 `should_panic(expected = "Phase 3")` stub: the status
// arm is real as of Phase 3. It surveys every target read-only and returns
// EXIT_SUCCESS regardless of registration state (the presence report + the
// handshake-name warning go to stderr; the round-trip / preservation behavior is
// covered by `src/register/tests.rs` and `src/register/desktop/tests.rs`).
#[test]
fn test_run_status_returns_success() {
    let cmd = McpCmd { cmd: McpSub::Status };
    assert_eq!(cmd.run(&make_io(), build_dummy), 0);
}

// Inverted from the Phase-1 `should_panic(expected = "Phase 5")` stub: the
// bundle arm is real as of Phase 5. `McpCmd::run` is a sync entry point that
// spins its own tokio runtime (mirroring `run_serve`), so driving it here
// (rather than only in `src/bundle/tests.rs`) proves the dispatch AND runtime
// wiring, not just `bundle()`'s internals.
#[test]
fn test_run_bundle_dispatches_and_writes_file() {
    let dir = tempfile::TempDir::new().unwrap();
    let out = dir.path().join("test.mcpb");
    let cmd = McpCmd {
        cmd: McpSub::Bundle { out: Some(out.clone()) },
    };
    assert_eq!(cmd.run(&make_io(), build_dummy), 0);
    assert!(out.is_file(), "expected a .mcpb file at {}", out.display());
}

#[derive(Parser)]
struct TestCli {
    #[command(subcommand)]
    cmd: McpSub,
}

#[test]
fn test_register_target_defaults_to_user() {
    let cli = TestCli::parse_from(["test", "register"]);
    assert!(matches!(cli.cmd, McpSub::Register { target: Target::User }));
}

#[test]
fn test_register_target_accepts_kebab_case() {
    let cli = TestCli::parse_from(["test", "register", "--target", "project"]);
    assert!(matches!(
        cli.cmd,
        McpSub::Register {
            target: Target::Project
        }
    ));
}

#[test]
fn test_register_target_is_case_insensitive() {
    let cli = TestCli::parse_from(["test", "register", "--target", "DESKTOP"]);
    assert!(matches!(
        cli.cmd,
        McpSub::Register {
            target: Target::Desktop
        }
    ));
}
