use std::path::PathBuf;
use std::process::Command;

use log::{debug, warn};
use serde_json::json;

use crate::McpIo;
use crate::error::{Error, Result};
use crate::register::Target;

/// The `claude` CLI binary we shell out to. Named as a const so the "run this by
/// hand" fallback message and the actual invocation can never drift.
const CLAUDE_BIN: &str = "claude";

/// The Claude config-scope flag value for a `user`/`project` [`Target`].
/// `Desktop` never reaches here (it uses the direct-write path).
fn scope(target: Target) -> &'static str {
    match target {
        Target::User => "user",
        Target::Project => "project",
        // Desktop is handled by desktop.rs and never dispatched here; treat it
        // as a programmer error rather than silently mapping it to a scope.
        Target::Desktop => unreachable!("desktop target does not use the claude CLI"),
    }
}

/// The `mcpServers` entry we register: `{ "type":"stdio", "command":<abs>, "args":["mcp","serve"] }`.
/// Verified byte-for-byte against a real `claude mcp add-json` invocation (Phase 3
/// notes). `command` is the resolved absolute path to THIS build, never a $PATH guess.
pub(crate) fn entry_json(command: &str) -> serde_json::Value {
    json!({
        "type": "stdio",
        "command": command,
        "args": ["mcp", "serve"],
    })
}

/// The argv for `claude mcp add-json <key> <json> -s <scope>`.
fn add_json_args(key: &str, json: &str, scope: &str) -> Vec<String> {
    vec![
        "mcp".to_string(),
        "add-json".to_string(),
        key.to_string(),
        json.to_string(),
        "-s".to_string(),
        scope.to_string(),
    ]
}

/// The argv for `claude mcp remove <key> -s <scope>`.
fn remove_args(key: &str, scope: &str) -> Vec<String> {
    vec![
        "mcp".to_string(),
        "remove".to_string(),
        key.to_string(),
        "-s".to_string(),
        scope.to_string(),
    ]
}

/// Register `io.server_key` -> `current_exe()` into Claude Code config scope
/// `target` (`user` or `project`) by shelling out to `claude mcp add-json`. This
/// treats the target config (`~/.claude.json` global state, or `./.mcp.json`) as
/// OPAQUE, so there is zero risk of dropping the user's unrelated keys. Idempotent.
pub(crate) fn register(io: &McpIo, target: Target) -> Result<()> {
    let scope = scope(target);
    debug!(
        "claude::register: bin={} server_key={} target={target:?} scope={scope}",
        io.bin, io.server_key
    );
    let command = super::current_exe()?;
    let json = entry_json(&command).to_string();
    let args = add_json_args(&io.server_key, &json, scope);
    run_claude(&args)?;
    debug!("claude::register: registered {} in {scope} scope", io.server_key);
    Ok(())
}

/// Remove `io.server_key`'s entry from Claude Code config scope `target` via
/// `claude mcp remove`.
pub(crate) fn unregister(io: &McpIo, target: Target) -> Result<()> {
    let scope = scope(target);
    debug!(
        "claude::unregister: bin={} server_key={} target={target:?} scope={scope}",
        io.bin, io.server_key
    );
    let args = remove_args(&io.server_key, scope);
    run_claude(&args)?;
    debug!("claude::unregister: removed {} from {scope} scope", io.server_key);
    Ok(())
}

/// Run `claude <args...>`, inheriting stdout/stderr so the user sees the CLI's own
/// "Added stdio MCP server ..." output. A missing `claude` binary fails LOUD with
/// the exact command to run by hand; a non-zero exit propagates as [`Error::ClaudeFailed`].
fn run_claude(args: &[String]) -> Result<()> {
    let printable = format!("{CLAUDE_BIN} {}", args.join(" "));
    debug!("run_claude: {printable}");
    let output = match Command::new(CLAUDE_BIN).args(args).output() {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            warn!("run_claude: `{CLAUDE_BIN}` not found on PATH");
            return Err(Error::ClaudeMissing { command: printable });
        }
        Err(e) => return Err(Error::Io(e)),
    };
    // Surface the CLI's own output to the user (it went to captured pipes).
    if !output.stdout.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }
    if !output.status.success() {
        let code = output
            .status
            .code()
            .map_or_else(|| "signal".to_string(), |c| c.to_string());
        warn!("run_claude: `{printable}` exited {code}");
        return Err(Error::ClaudeFailed {
            command: printable,
            code,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(())
}

/// The path a Claude Code `target` writes to, used by `status` for READ-ONLY
/// presence detection (writes always go through the `claude` CLI above):
///   - `user`    -> `$CLAUDE_CONFIG_DIR/.claude.json`, else `$HOME/.claude.json`
///   - `project` -> `./.mcp.json`
pub(crate) fn config_path(target: Target) -> Result<PathBuf> {
    match target {
        Target::User => {
            if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
                let path = PathBuf::from(dir);
                if path.is_absolute() {
                    return Ok(path.join(".claude.json"));
                }
            }
            dirs::home_dir()
                .map(|h| h.join(".claude.json"))
                .ok_or_else(|| Error::ConfigPath {
                    what: "claude code user".to_string(),
                })
        }
        Target::Project => Ok(PathBuf::from(".mcp.json")),
        Target::Desktop => unreachable!("desktop target uses desktop::config_path"),
    }
}

#[cfg(test)]
mod tests;
