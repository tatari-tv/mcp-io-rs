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

/// The checked contract artifact (`tests/fixtures/contract.json`, Phase 6): pins the
/// entry shape and per-target path-resolution rule so a future consumer (a
/// `mcp-io-py` port, or any other rewrite) cannot drift the way okta-auth did. Every
/// assertion here calls the REAL writer/resolver, never a hand-copied literal, so
/// changing the entry shape or a path rule without updating the fixture fails these
/// tests.
mod contract {
    use super::*;

    const CONTRACT: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/contract.json"));

    fn fixture() -> serde_json::Value {
        serde_json::from_str(CONTRACT).unwrap()
    }

    #[test]
    fn test_contract_entry_matches_claude_entry_json() {
        let contract = fixture();
        let example = &contract["entry"]["example"];
        let command = example["command"].as_str().unwrap();
        assert_eq!(
            &crate::register::claude::entry_json(command),
            example,
            "claude::entry_json() diverged from the checked contract fixture"
        );
    }

    #[test]
    fn test_contract_project_path_matches_config_path() {
        let contract = fixture();
        let expected = contract["targets"]["project"]["config-file"].as_str().unwrap();
        assert_eq!(
            crate::register::claude::config_path(Target::Project).unwrap(),
            std::path::PathBuf::from(expected)
        );
    }

    #[test]
    fn test_contract_user_path_honors_env_and_filename() {
        let contract = fixture();
        let filename = contract["targets"]["user"]["config-file"].as_str().unwrap();

        let guard = ENV_LOCK.lock().unwrap();
        let prior = std::env::var("CLAUDE_CONFIG_DIR").ok();
        let dir = TempDir::new().unwrap();
        unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", dir.path()) };
        assert_eq!(
            crate::register::claude::config_path(Target::User).unwrap(),
            dir.path().join(filename)
        );
        unsafe { std::env::remove_var("CLAUDE_CONFIG_DIR") };
        assert!(
            crate::register::claude::config_path(Target::User)
                .unwrap()
                .ends_with(filename)
        );

        match prior {
            Some(v) => unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", v) },
            None => unsafe { std::env::remove_var("CLAUDE_CONFIG_DIR") },
        }
        drop(guard);
    }

    /// Desktop's path is asserted by SUFFIX only (`Claude/<config-file>`), never a
    /// full platform-specific literal -- the env-honoring behavior itself is already
    /// covered by `crate::config`'s own tests, and the suffix holds regardless of
    /// whatever `$XDG_CONFIG_HOME` happens to be set to in this process.
    #[test]
    fn test_contract_desktop_path_matches_suffix() {
        let contract = fixture();
        let filename = contract["targets"]["desktop"]["config-file"].as_str().unwrap();
        let path = crate::register::desktop::config_path().unwrap();
        assert!(
            path.ends_with(format!("Claude/{filename}")),
            "desktop config_path() {} must end with Claude/{filename}",
            path.display()
        );
    }
}
