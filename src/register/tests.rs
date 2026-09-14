#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::convert::Infallible;
use std::ffi::OsString;
use std::fs;

use rmcp::ServerHandler;
use rmcp::model::{Implementation, ServerInfo};
use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;
use crate::register::claude::entry_json;

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
    assert_eq!(register(&io, Target::User, false), EXIT_SUCCESS);
    assert!(is_registered(&io, Target::User), "register should make it present");
    // No --force anywhere in this round trip: the entry the CLI just wrote for us
    // must read as OURS, or the guard would reject what the writer wrote.
    assert_eq!(unregister(&io, Target::User, false), EXIT_SUCCESS);
    assert!(!is_registered(&io, Target::User), "unregister should remove it");

    match prior {
        Some(v) => unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", v) },
        None => unsafe { std::env::remove_var("CLAUDE_CONFIG_DIR") },
    }
    drop(guard);
}

/// Redirects Claude Desktop's config path into a temp dir for one test: macOS
/// resolves it under `$HOME`, Linux under `$XDG_CONFIG_HOME`, so both are set.
/// Restoring on DROP (rather than at the end of the test body) means a failed
/// assertion cannot leak the override into the rest of the process. Env is
/// process-global, so every user of this also holds `ENV_LOCK`.
struct DesktopHome {
    dir: TempDir,
    home: Option<OsString>,
    xdg: Option<OsString>,
}

impl DesktopHome {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let home = std::env::var_os("HOME");
        let xdg = std::env::var_os("XDG_CONFIG_HOME");
        unsafe {
            std::env::set_var("HOME", dir.path());
            std::env::set_var("XDG_CONFIG_HOME", dir.path());
        }
        Self { dir, home, xdg }
    }

    /// The REAL resolver's answer, asserted to be inside the temp dir so a test can
    /// never write to the developer's own Claude Desktop config.
    fn config(&self) -> std::path::PathBuf {
        let path = desktop::config_path().unwrap();
        assert!(
            path.starts_with(self.dir.path()),
            "desktop config path {} escaped the temp home {}",
            path.display(),
            self.dir.path().display()
        );
        path
    }
}

impl Drop for DesktopHome {
    fn drop(&mut self) {
        restore("HOME", self.home.take());
        restore("XDG_CONFIG_HOME", self.xdg.take());
    }
}

fn restore(key: &str, prior: Option<OsString>) {
    match prior {
        Some(value) => unsafe { std::env::set_var(key, value) },
        None => unsafe { std::env::remove_var(key) },
    }
}

/// The reported scenario: the community `xoxc` slack MCP server sitting under OUR
/// key, next to an unrelated server. This is the entry `register` used to delete.
fn foreign_config(key: &str) -> Value {
    let mut config = json!({
        "theme": "dark",
        "mcpServers": {
            "other-a": { "type": "stdio", "command": "/bin/a", "args": ["x"] }
        }
    });
    config["mcpServers"][key] = json!({
        "command": "npx",
        "args": ["-y", "slack-mcp-server@latest"],
        "env": { "SLACK_MCP_XOXC_TOKEN": "xoxc-fake" }
    });
    config
}

fn seed(path: &std::path::Path, value: &Value) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
}

fn read(path: &std::path::Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

/// Success criterion (1): a foreign entry under our key survives a `register`
/// byte-for-byte, and the call FAILS. Before the ownership check, this test's
/// config came back with the `xoxc` entry replaced by ours and an exit code of 0.
#[test]
fn test_register_refuses_a_foreign_entry_and_leaves_the_file_byte_identical() {
    let guard = ENV_LOCK.lock().unwrap();
    let home = DesktopHome::new();
    let io = make_io();
    let path = home.config();
    seed(&path, &foreign_config(&io.server_key));
    let before = fs::read(&path).unwrap();

    assert_eq!(
        register(&io, Target::Desktop, false),
        EXIT_FAILURE,
        "register must refuse a key registered by somebody else"
    );
    assert_eq!(
        fs::read(&path).unwrap(),
        before,
        "the refused config must be byte-for-byte untouched"
    );

    drop(home);
    drop(guard);
}

/// Success criterion (2): the same scenario with `--force` replaces the entry.
#[test]
fn test_register_force_replaces_a_foreign_entry() {
    let guard = ENV_LOCK.lock().unwrap();
    let home = DesktopHome::new();
    let io = make_io();
    let path = home.config();
    seed(&path, &foreign_config(&io.server_key));

    assert_eq!(register(&io, Target::Desktop, true), EXIT_SUCCESS);

    let config = read(&path);
    assert_eq!(
        config["mcpServers"][&io.server_key],
        entry_json(&current_exe().unwrap(), &BTreeMap::new()),
        "--force must replace the foreign entry with ours"
    );
    // The override is scoped to OUR key: everything else still survives.
    assert_eq!(config["mcpServers"]["other-a"]["command"], json!("/bin/a"));
    assert_eq!(config["theme"], json!("dark"));

    drop(home);
    drop(guard);
}

/// `unregister` is destructive on both paths, so it carries the same guard and the
/// same override.
#[test]
fn test_unregister_refuses_a_foreign_entry_until_forced() {
    let guard = ENV_LOCK.lock().unwrap();
    let home = DesktopHome::new();
    let io = make_io();
    let path = home.config();
    seed(&path, &foreign_config(&io.server_key));
    let before = fs::read(&path).unwrap();

    assert_eq!(unregister(&io, Target::Desktop, false), EXIT_FAILURE);
    assert_eq!(fs::read(&path).unwrap(), before, "refusal must not write");

    assert_eq!(unregister(&io, Target::Desktop, true), EXIT_SUCCESS);
    assert!(
        read(&path)["mcpServers"].get(&io.server_key).is_none(),
        "--force must remove the entry"
    );

    drop(home);
    drop(guard);
}

/// The guard must accept what the writer wrote: a plain re-register of OUR OWN
/// entry needs no `--force`. A predicate that failed here would be a guard firing
/// on the happy path, which is how safety checks get disabled.
#[test]
fn test_register_over_our_own_entry_needs_no_force() {
    let guard = ENV_LOCK.lock().unwrap();
    let home = DesktopHome::new();
    let io = make_io();
    let path = home.config();

    assert_eq!(register(&io, Target::Desktop, false), EXIT_SUCCESS, "absent -> proceed");
    assert_eq!(register(&io, Target::Desktop, false), EXIT_SUCCESS, "ours -> proceed");
    assert_eq!(unregister(&io, Target::Desktop, false), EXIT_SUCCESS, "ours -> proceed");
    assert!(read(&path)["mcpServers"].get(&io.server_key).is_none());

    drop(home);
    drop(guard);
}

/// `status` reports the ownership word for all three states and KEEPS exit 0 for
/// every one of them (persona-cli asserts only the exit code).
#[test]
fn test_status_exits_ok_for_every_ownership_state() {
    let guard = ENV_LOCK.lock().unwrap();
    let home = DesktopHome::new();
    let io = make_io();
    let path = home.config();

    // Absent.
    assert_eq!(status(&io, || Ok::<_, Infallible>(DummyHandler)), EXIT_SUCCESS);
    // Foreign.
    seed(&path, &foreign_config(&io.server_key));
    assert_eq!(status(&io, || Ok::<_, Infallible>(DummyHandler)), EXIT_SUCCESS);
    // Ours.
    assert_eq!(register(&io, Target::Desktop, true), EXIT_SUCCESS);
    assert_eq!(status(&io, || Ok::<_, Infallible>(DummyHandler)), EXIT_SUCCESS);

    drop(home);
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
            &crate::register::claude::entry_json(command, &std::collections::BTreeMap::new()),
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
