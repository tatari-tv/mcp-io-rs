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

#[test]
#[should_panic(expected = "Phase 2")]
fn test_run_serve_dispatches_to_phase2_stub() {
    let cmd = McpCmd { cmd: McpSub::Serve };
    cmd.run(&make_io(), build_dummy);
}

#[test]
#[should_panic(expected = "Phase 3")]
fn test_run_status_dispatches_to_phase3_stub() {
    let cmd = McpCmd { cmd: McpSub::Status };
    cmd.run(&make_io(), build_dummy);
}

#[test]
#[should_panic(expected = "Phase 5")]
fn test_run_bundle_dispatches_to_phase5_stub() {
    let cmd = McpCmd {
        cmd: McpSub::Bundle { out: None },
    };
    cmd.run(&make_io(), build_dummy);
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
