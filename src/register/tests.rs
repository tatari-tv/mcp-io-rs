#![allow(clippy::unwrap_used)]

use std::convert::Infallible;

use rmcp::ServerHandler;
use rmcp::model::{Implementation, ServerInfo};
use tempfile::TempDir;

use super::*;

fn make_io() -> McpIo {
    McpIo::new("mcp-io-test", "0.0.0", None)
}

/// Default `get_info` -> rmcp reports itself as "rmcp" (the gotcha the status
/// warning exists to catch).
struct DummyHandler;
impl ServerHandler for DummyHandler {}

/// Overrides `get_info` to report a chosen name, mimicking a host that correctly
/// called `.with_server_info(Implementation::new(bin, ..))`.
struct NamedHandler(String);
impl ServerHandler for NamedHandler {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::default().with_server_info(Implementation::new(self.0.clone(), "0.0.0"))
    }
}

fn claude_available() -> bool {
    std::process::Command::new("claude")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn test_target_label() {
    assert_eq!(Target::User.label(), "user");
    assert_eq!(Target::Project.label(), "project");
    assert_eq!(Target::Desktop.label(), "desktop");
}

#[test]
fn test_current_exe_is_absolute() {
    let exe = current_exe().unwrap();
    assert!(
        std::path::Path::new(&exe).is_absolute(),
        "current_exe must be absolute: {exe}"
    );
}

/// status always exits 0; the DummyHandler reports "rmcp" != bin, exercising the
/// mismatch-warning branch.
#[test]
fn test_status_exits_ok_on_name_mismatch() {
    let guard = ENV_LOCK.lock().unwrap();
    let prior = std::env::var("CLAUDE_CONFIG_DIR").ok();
    let dir = TempDir::new().unwrap();
    unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", dir.path()) };

    let io = make_io();
    // Name is "rmcp" (default) != "mcp-io-test": warn path, still exit 0.
    assert_eq!(status(&io, || Ok::<_, Infallible>(DummyHandler)), EXIT_SUCCESS);

    match prior {
        Some(v) => unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", v) },
        None => unsafe { std::env::remove_var("CLAUDE_CONFIG_DIR") },
    }
    drop(guard);
}

#[test]
fn test_status_exits_ok_on_matching_name_and_build_error() {
    let guard = ENV_LOCK.lock().unwrap();
    let prior = std::env::var("CLAUDE_CONFIG_DIR").ok();
    let dir = TempDir::new().unwrap();
    unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", dir.path()) };

    let io = make_io();
    // Matching name: no warning, exit 0.
    let named = NamedHandler(io.bin.clone());
    assert_eq!(status(&io, || Ok::<_, Infallible>(named)), EXIT_SUCCESS);
    // Build failure: name check skipped, still exit 0.
    assert_eq!(
        status(&io, || Err::<DummyHandler, _>("token missing".to_string())),
        EXIT_SUCCESS
    );

    match prior {
        Some(v) => unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", v) },
        None => unsafe { std::env::remove_var("CLAUDE_CONFIG_DIR") },
    }
    drop(guard);
}

/// Live round-trip through the real `claude` CLI (user scope) in an isolated
/// `$CLAUDE_CONFIG_DIR`: register -> present -> unregister -> absent. Skipped when
/// `claude` is not on PATH (the command-construction + entry shape are covered by
/// unit tests in `claude/tests.rs` regardless).
#[test]
fn test_claude_user_roundtrip() {
    if !claude_available() {
        eprintln!("SKIP test_claude_user_roundtrip: `claude` not on PATH");
        return;
    }
    let guard = ENV_LOCK.lock().unwrap();
    let prior = std::env::var("CLAUDE_CONFIG_DIR").ok();
    let dir = TempDir::new().unwrap();
    unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", dir.path()) };

    let io = make_io();
    assert!(!is_registered(&io, Target::User), "should start unregistered");
    assert_eq!(register(&io, Target::User), EXIT_SUCCESS);
    assert!(is_registered(&io, Target::User), "register should make it present");
    assert_eq!(unregister(&io, Target::User), EXIT_SUCCESS);
    assert!(!is_registered(&io, Target::User), "unregister should remove it");

    match prior {
        Some(v) => unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", v) },
        None => unsafe { std::env::remove_var("CLAUDE_CONFIG_DIR") },
    }
    drop(guard);
}
