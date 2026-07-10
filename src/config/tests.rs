#![allow(clippy::unwrap_used)]

use std::sync::Mutex;

use super::*;

// Serialize all env-var-touching tests to prevent parallel races.
static ENV_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn test_mcp_io_new_defaults_server_key_to_bin() {
    let io = McpIo::new("slack", "1.2.3", None);
    assert_eq!(io.bin, "slack");
    assert_eq!(io.version, "1.2.3");
    assert_eq!(io.server_key, "slack");
}

#[test]
fn test_mcp_io_new_honors_server_key_override() {
    let io = McpIo::new("slack", "1.2.3", Some("slack-mcp".to_string()));
    assert_eq!(io.bin, "slack");
    assert_eq!(io.server_key, "slack-mcp");
}

#[test]
fn test_xdg_config_dir_honors_env_and_falls_back() {
    let guard = ENV_LOCK.lock().unwrap();
    let prior = std::env::var("XDG_CONFIG_HOME").ok();

    let dir = tempfile::TempDir::new().unwrap();
    unsafe { std::env::set_var("XDG_CONFIG_HOME", dir.path()) };
    assert_eq!(xdg_config_dir().as_deref(), Some(dir.path()));

    // Unset -> fall back to $HOME/.config, never ~/Library/... on mac.
    unsafe { std::env::remove_var("XDG_CONFIG_HOME") };
    assert!(xdg_config_dir().unwrap().ends_with(".config"));

    match prior {
        Some(v) => unsafe { std::env::set_var("XDG_CONFIG_HOME", v) },
        None => unsafe { std::env::remove_var("XDG_CONFIG_HOME") },
    }
    drop(guard);
}

#[test]
fn test_xdg_data_dir_honors_env_and_falls_back() {
    let guard = ENV_LOCK.lock().unwrap();
    let prior = std::env::var("XDG_DATA_HOME").ok();

    let dir = tempfile::TempDir::new().unwrap();
    unsafe { std::env::set_var("XDG_DATA_HOME", dir.path()) };
    assert_eq!(xdg_data_dir().as_deref(), Some(dir.path()));

    // Unset -> fall back to $HOME/.local/share, never ~/Library/... on mac.
    unsafe { std::env::remove_var("XDG_DATA_HOME") };
    assert!(xdg_data_dir().unwrap().ends_with(".local/share"));

    match prior {
        Some(v) => unsafe { std::env::set_var("XDG_DATA_HOME", v) },
        None => unsafe { std::env::remove_var("XDG_DATA_HOME") },
    }
    drop(guard);
}
