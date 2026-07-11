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
        entry_json("/abs/path/slack", &BTreeMap::new()),
        json!({
            "type": "stdio",
            "command": "/abs/path/slack",
            "args": ["mcp", "serve"]
        })
    );
}

#[test]
fn test_entry_json_omits_empty_env() {
    // Empty env keeps the verified contract shape: no `env` key at all.
    let entry = entry_json("/abs/path/slack", &BTreeMap::new());
    assert!(entry.get("env").is_none());
    assert_eq!(entry["args"], json!(["mcp", "serve"]));
}

#[test]
fn test_entry_json_includes_env_sorted() {
    let mut env = BTreeMap::new();
    env.insert(
        "SLACK_VALET_URL".to_string(),
        "https://valet.test.tatari.dev".to_string(),
    );
    env.insert("A_FIRST".to_string(), "1".to_string());
    let entry = entry_json("/abs/path/slack", &env);

    assert_eq!(entry["env"]["SLACK_VALET_URL"], "https://valet.test.tatari.dev");
    assert_eq!(entry["env"]["A_FIRST"], "1");
    // Base fields untouched by the env splice.
    assert_eq!(entry["command"], "/abs/path/slack");
    assert_eq!(entry["args"], json!(["mcp", "serve"]));
    // Deterministic sorted key order (BTreeMap source): A_FIRST precedes SLACK_VALET_URL.
    let serialized = entry["env"].to_string();
    assert!(serialized.find("A_FIRST").unwrap() < serialized.find("SLACK_VALET_URL").unwrap());
}

/// Re-registering must be idempotent (the bug: `claude mcp add-json` refuses to
/// overwrite, so the second call errored "already exists") AND the host's env must
/// land in the written entry. Drives the real CLI into an isolated config dir.
#[test]
fn test_register_is_idempotent_and_bakes_env() {
    if !claude_available() {
        eprintln!("SKIP test_register_is_idempotent_and_bakes_env: `claude` not on PATH");
        return;
    }
    let guard = ENV_LOCK.lock().unwrap();
    let prior = std::env::var("CLAUDE_CONFIG_DIR").ok();

    let dir = tempfile::TempDir::new().unwrap();
    unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", dir.path()) };

    let mut env = BTreeMap::new();
    env.insert(
        "SLACK_VALET_URL".to_string(),
        "https://valet.test.tatari.dev".to_string(),
    );
    let io = crate::McpIo::new("slack", "0.0.0", None).with_env(env);

    register(&io, Target::User).expect("first register");
    register(&io, Target::User).expect("re-register must be idempotent, not `already exists`");

    let written: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join(".claude.json")).unwrap()).unwrap();
    assert_eq!(
        written["mcpServers"]["slack"]["env"]["SLACK_VALET_URL"],
        "https://valet.test.tatari.dev"
    );
    assert_eq!(written["mcpServers"]["slack"]["args"], json!(["mcp", "serve"]));

    match prior {
        Some(v) => unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", v) },
        None => unsafe { std::env::remove_var("CLAUDE_CONFIG_DIR") },
    }
    drop(guard);
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
    let json = entry_json(command, &BTreeMap::new()).to_string();
    let args = add_json_args("slack", &json, "user");
    let status = Command::new(CLAUDE_BIN).args(&args).output().unwrap();
    assert!(status.status.success(), "claude add-json failed: {status:?}");

    let written: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join(".claude.json")).unwrap()).unwrap();
    assert_eq!(
        written["mcpServers"]["slack"],
        entry_json(command, &BTreeMap::new()),
        "real claude entry must equal entry_json()"
    );

    match prior {
        Some(v) => unsafe { std::env::set_var("CLAUDE_CONFIG_DIR", v) },
        None => unsafe { std::env::remove_var("CLAUDE_CONFIG_DIR") },
    }
    drop(guard);
}
