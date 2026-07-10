#![allow(clippy::unwrap_used)]

use serde_json::json;

use super::*;
use crate::register::ENV_LOCK;

/// True when a real `claude` binary is on PATH. The round-trip / entry-match tests
/// exercise the live CLI when present, and are skipped (with a note) when absent so
/// CI on a claude-less host still passes; the pure construction tests always run.
fn claude_available() -> bool {
    Command::new(CLAUDE_BIN)
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn test_entry_json_shape() {
    // Pins the exact entry shape verified against a real `claude mcp add-json`.
    assert_eq!(
        entry_json("/abs/path/slack"),
        json!({
            "type": "stdio",
            "command": "/abs/path/slack",
            "args": ["mcp", "serve"]
        })
    );
}

#[test]
fn test_scope_mapping() {
    assert_eq!(scope(Target::User), "user");
    assert_eq!(scope(Target::Project), "project");
}

#[test]
fn test_add_json_args() {
    let args = add_json_args("slack", "{\"x\":1}", "user");
    assert_eq!(args, vec!["mcp", "add-json", "slack", "{\"x\":1}", "-s", "user"]);
}

#[test]
fn test_remove_args() {
    let args = remove_args("slack", "project");
    assert_eq!(args, vec!["mcp", "remove", "slack", "-s", "project"]);
}

#[test]
fn test_user_config_path_honors_env() {
    let guard = ENV_LOCK.lock().unwrap();
    let prior = std::env::var("CLAUDE_CONFIG_DIR").ok();

    let dir = tempfile::TempDir::new().unwrap();
    unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", dir.path()) };
    assert_eq!(config_path(Target::User).unwrap(), dir.path().join(".claude.json"));

    unsafe { std::env::remove_var("CLAUDE_CONFIG_DIR") };
    // Falls back to $HOME/.claude.json.
    assert!(config_path(Target::User).unwrap().ends_with(".claude.json"));

    match prior {
        Some(v) => unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", v) },
        None => unsafe { std::env::remove_var("CLAUDE_CONFIG_DIR") },
    }
    drop(guard);
}

#[test]
fn test_project_config_path() {
    assert_eq!(
        config_path(Target::Project).unwrap(),
        std::path::PathBuf::from(".mcp.json")
    );
}

/// The AC: `<bin> mcp register` and a hand `claude mcp add-json` produce the SAME
/// entry. We drive the real CLI into an isolated `$CLAUDE_CONFIG_DIR` and assert the
/// entry the CLI wrote equals what `entry_json` produces.
#[test]
fn test_claude_add_json_matches_our_entry() {
    if !claude_available() {
        eprintln!("SKIP test_claude_add_json_matches_our_entry: `claude` not on PATH");
        return;
    }
    let guard = ENV_LOCK.lock().unwrap();
    let prior = std::env::var("CLAUDE_CONFIG_DIR").ok();

    let dir = tempfile::TempDir::new().unwrap();
    unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", dir.path()) };

    let command = "/abs/path/slack";
    let json = entry_json(command).to_string();
    let args = add_json_args("slack", &json, "user");
    let status = Command::new(CLAUDE_BIN).args(&args).output().unwrap();
    assert!(status.status.success(), "claude add-json failed: {status:?}");

    let written: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join(".claude.json")).unwrap()).unwrap();
    assert_eq!(
        written["mcpServers"]["slack"],
        entry_json(command),
        "real claude entry must equal entry_json()"
    );

    match prior {
        Some(v) => unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", v) },
        None => unsafe { std::env::remove_var("CLAUDE_CONFIG_DIR") },
    }
    drop(guard);
}
