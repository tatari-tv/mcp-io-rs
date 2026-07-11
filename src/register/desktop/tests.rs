#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::fs;

use serde_json::{Value, json};
use tempfile::TempDir;

use super::*;
use crate::register::claude::entry_json;

const KEY: &str = "slack";
const CMD: &str = "/abs/path/slack";

/// A config pre-populated with two OTHER servers AND unrelated top-level keys,
/// exactly the shape AC #3 demands survive register + unregister.
fn populated() -> Value {
    json!({
        "theme": "dark",
        "someTopLevelSetting": { "nested": true, "count": 7 },
        "mcpServers": {
            "other-a": { "type": "stdio", "command": "/bin/a", "args": ["x"] },
            "other-b": { "type": "stdio", "command": "/bin/b", "args": ["y", "z"] }
        },
        "trailingKey": [1, 2, 3]
    })
}

fn write(path: &std::path::Path, value: &Value) {
    fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
}

fn read(path: &std::path::Path) -> Value {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

#[test]
fn test_register_fresh_creates_file() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("Claude").join("claude_desktop_config.json");
    // Parent dir does NOT exist yet: register must create it.
    register_at(&path, KEY, CMD, &BTreeMap::new()).unwrap();

    let cfg = read(&path);
    assert_eq!(cfg["mcpServers"][KEY], entry_json(CMD, &BTreeMap::new()));
}

#[test]
fn test_register_preserves_all_keys_and_servers() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("claude_desktop_config.json");
    write(&path, &populated());

    register_at(&path, KEY, CMD, &BTreeMap::new()).unwrap();
    let cfg = read(&path);

    // Our key landed.
    assert_eq!(cfg["mcpServers"][KEY], entry_json(CMD, &BTreeMap::new()));
    // Both other servers survived, byte-value intact.
    assert_eq!(cfg["mcpServers"]["other-a"]["command"], json!("/bin/a"));
    assert_eq!(cfg["mcpServers"]["other-b"]["args"], json!(["y", "z"]));
    // Every unrelated TOP-LEVEL key survived, values intact (AC #3).
    assert_eq!(cfg["theme"], json!("dark"));
    assert_eq!(cfg["someTopLevelSetting"], json!({ "nested": true, "count": 7 }));
    assert_eq!(cfg["trailingKey"], json!([1, 2, 3]));
}

#[test]
fn test_unregister_preserves_all_keys_and_other_servers() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("claude_desktop_config.json");
    write(&path, &populated());

    register_at(&path, KEY, CMD, &BTreeMap::new()).unwrap();
    unregister_at(&path, KEY).unwrap();
    let cfg = read(&path);

    // Our key is gone...
    assert!(cfg["mcpServers"].get(KEY).is_none());
    // ...but the two other servers AND every top-level key survived.
    assert_eq!(cfg["mcpServers"]["other-a"]["command"], json!("/bin/a"));
    assert_eq!(cfg["mcpServers"]["other-b"]["command"], json!("/bin/b"));
    assert_eq!(cfg["theme"], json!("dark"));
    assert_eq!(cfg["someTopLevelSetting"], json!({ "nested": true, "count": 7 }));
    assert_eq!(cfg["trailingKey"], json!([1, 2, 3]));
}

#[test]
fn test_second_register_is_byte_identical() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("claude_desktop_config.json");
    write(&path, &populated());

    register_at(&path, KEY, CMD, &BTreeMap::new()).unwrap();
    let after_first = fs::read(&path).unwrap();
    register_at(&path, KEY, CMD, &BTreeMap::new()).unwrap();
    let after_second = fs::read(&path).unwrap();

    assert_eq!(
        after_first, after_second,
        "second register must be a byte-identical no-op"
    );
}

#[test]
fn test_malformed_json_errors_and_leaves_file_untouched() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("claude_desktop_config.json");
    let garbage = b"{ this is not valid json ]";
    fs::write(&path, garbage).unwrap();

    let err = register_at(&path, KEY, CMD, &BTreeMap::new()).unwrap_err();
    assert!(matches!(err, Error::MalformedConfig { .. }), "got {err:?}");
    assert_eq!(
        fs::read(&path).unwrap(),
        garbage,
        "file must be byte-for-byte untouched"
    );
}

#[test]
fn test_non_object_mcpservers_errors_and_leaves_file_untouched() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("claude_desktop_config.json");
    let original = json!({ "mcpServers": "not an object", "keep": true });
    write(&path, &original);
    let bytes_before = fs::read(&path).unwrap();

    let err = register_at(&path, KEY, CMD, &BTreeMap::new()).unwrap_err();
    assert!(matches!(err, Error::McpServersNotObject { .. }), "got {err:?}");
    assert_eq!(fs::read(&path).unwrap(), bytes_before, "file must be untouched");
}

#[test]
fn test_non_object_toplevel_errors_and_leaves_file_untouched() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("claude_desktop_config.json");
    fs::write(&path, b"[1, 2, 3]").unwrap();

    let err = register_at(&path, KEY, CMD, &BTreeMap::new()).unwrap_err();
    assert!(matches!(err, Error::ConfigNotObject { .. }), "got {err:?}");
    assert_eq!(fs::read(&path).unwrap(), b"[1, 2, 3]", "file must be untouched");
}

#[test]
fn test_zero_byte_file_starts_fresh() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("claude_desktop_config.json");
    fs::write(&path, b"").unwrap();

    register_at(&path, KEY, CMD, &BTreeMap::new()).unwrap();
    let cfg = read(&path);
    assert_eq!(cfg["mcpServers"][KEY], entry_json(CMD, &BTreeMap::new()));
}

#[test]
fn test_unregister_missing_file_is_noop() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("claude_desktop_config.json");
    // File never created.
    unregister_at(&path, KEY).unwrap();
    assert!(!path.exists(), "unregister on a missing file must not create one");
}

#[test]
fn test_unregister_absent_key_does_not_rewrite() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("claude_desktop_config.json");
    write(&path, &populated());
    let bytes_before = fs::read(&path).unwrap();

    unregister_at(&path, "never-registered").unwrap();
    assert_eq!(
        fs::read(&path).unwrap(),
        bytes_before,
        "no-op must not rewrite the file"
    );
}

#[test]
fn test_key_present_detection() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("claude_desktop_config.json");
    write(&path, &populated());

    assert!(key_present(&path, "other-a"));
    assert!(!key_present(&path, KEY));
    register_at(&path, KEY, CMD, &BTreeMap::new()).unwrap();
    assert!(key_present(&path, KEY));
    // A missing file reports false, never errors.
    assert!(!key_present(&dir.path().join("nope.json"), KEY));
}

#[cfg(unix)]
#[test]
fn test_register_preserves_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("claude_desktop_config.json");
    write(&path, &populated());
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

    register_at(&path, KEY, CMD, &BTreeMap::new()).unwrap();

    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "atomic write must preserve the original file mode");
}
