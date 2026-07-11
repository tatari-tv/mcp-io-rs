use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use log::{debug, warn};
use serde_json::{Map, Value};
use tempfile::NamedTempFile;

use crate::McpIo;
use crate::error::{Error, Result};
use crate::register::claude::entry_json;

/// The `mcpServers` key. Named once so read (status), splice, and remove agree.
const MCP_SERVERS: &str = "mcpServers";

/// Resolve Claude Desktop's config path. This is a THIRD-PARTY app's own location,
/// so it is matched to where Claude Desktop actually reads (NOT the crate's XDG
/// helper on macOS):
///   - macOS:  `~/Library/Application Support/Claude/claude_desktop_config.json`
///   - Linux:  `$XDG_CONFIG_HOME`/`~/.config` `/Claude/claude_desktop_config.json` (community build)
pub(crate) fn config_path() -> Result<PathBuf> {
    let base = if cfg!(target_os = "macos") {
        dirs::home_dir().map(|h| h.join("Library").join("Application Support"))
    } else {
        crate::config::xdg_config_dir()
    };
    base.map(|b| b.join("Claude").join("claude_desktop_config.json"))
        .ok_or_else(|| Error::ConfigPath {
            what: "claude desktop".to_string(),
        })
}

/// Register via a direct, Value-preserving atomic write to Claude Desktop's
/// `claude_desktop_config.json`. No `claude` CLI exists in a Desktop-only install,
/// so this splices ONLY `mcpServers.<key>` and preserves every other key.
pub(crate) fn register(io: &McpIo) -> Result<()> {
    let path = config_path()?;
    debug!(
        "desktop::register: bin={} server_key={} path={}",
        io.bin,
        io.server_key,
        path.display()
    );
    let command = super::current_exe()?;
    register_at(&path, &io.server_key, &command, &io.env)
}

/// Remove `io.server_key` from Claude Desktop's config via the same direct-write path.
pub(crate) fn unregister(io: &McpIo) -> Result<()> {
    let path = config_path()?;
    debug!(
        "desktop::unregister: bin={} server_key={} path={}",
        io.bin,
        io.server_key,
        path.display()
    );
    unregister_at(&path, &io.server_key)
}

/// Read `path` into a JSON object map. A missing or zero-byte file is a fresh
/// start (empty map). A present-but-malformed file, or one whose top-level value
/// is not an object, is an ERROR and the file is left byte-for-byte untouched.
fn read_config(path: &Path) -> Result<Map<String, Value>> {
    let display = path.display().to_string();
    match fs::metadata(path) {
        Ok(meta) if meta.len() == 0 => {
            debug!("read_config: {display} is zero-byte, starting fresh");
            return Ok(Map::new());
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            debug!("read_config: {display} missing, starting fresh");
            return Ok(Map::new());
        }
        Err(e) => return Err(Error::Io(e)),
    }
    let bytes = fs::read(path)?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|source| Error::MalformedConfig {
        path: display.clone(),
        source,
    })?;
    match value {
        Value::Object(map) => Ok(map),
        _ => Err(Error::ConfigNotObject { path: display }),
    }
}

/// Splice `key` -> stdio entry into `mcpServers`, preserving every other key, then
/// write atomically. `env` is baked into the entry when non-empty (mirrors the
/// Claude Code path). See [`read_config`] for the file-state edge handling.
fn register_at(path: &Path, key: &str, command: &str, env: &BTreeMap<String, String>) -> Result<()> {
    debug!("register_at: path={} key={key} env_count={}", path.display(), env.len());
    let mut config = read_config(path)?;

    // `or_insert_with` only fires when `mcpServers` is absent; a present-but-non-object
    // value is left in place and rejected below (never silently overwritten).
    let servers = config.entry(MCP_SERVERS).or_insert_with(|| Value::Object(Map::new()));
    match servers {
        Value::Object(servers) => {
            servers.insert(key.to_string(), entry_json(command, env));
        }
        _ => {
            return Err(Error::McpServersNotObject {
                path: path.display().to_string(),
            });
        }
    }

    write_atomic(path, &config)?;
    debug!("register_at: wrote {key} -> {command} into {}", path.display());
    Ok(())
}

/// Remove `key` from `mcpServers`, preserving every other key, writing atomically
/// only when something actually changed (a missing key / file is a clean no-op).
fn unregister_at(path: &Path, key: &str) -> Result<()> {
    debug!("unregister_at: path={} key={key}", path.display());
    // Missing / zero-byte file: nothing registered here, do not create one.
    match fs::metadata(path) {
        Ok(meta) if meta.len() == 0 => {
            debug!("unregister_at: {} is zero-byte, nothing to remove", path.display());
            return Ok(());
        }
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            debug!("unregister_at: {} missing, nothing to remove", path.display());
            return Ok(());
        }
        Err(e) => return Err(Error::Io(e)),
    }

    let mut config = read_config(path)?;
    let removed = match config.get_mut(MCP_SERVERS) {
        Some(Value::Object(servers)) => servers.remove(key).is_some(),
        Some(_) => {
            return Err(Error::McpServersNotObject {
                path: path.display().to_string(),
            });
        }
        None => false,
    };

    if removed {
        write_atomic(path, &config)?;
        debug!("unregister_at: removed {key} from {}", path.display());
    } else {
        debug!("unregister_at: {key} not present in {}, no write", path.display());
    }
    Ok(())
}

/// Read-only presence check for `status`: is `key` under `mcpServers` in `path`?
/// A missing/empty/malformed file reports `false` (and warns on malformed) rather
/// than erroring, since status must survey all targets without aborting.
pub(crate) fn key_present(path: &Path, key: &str) -> bool {
    match read_config(path) {
        Ok(config) => match config.get(MCP_SERVERS) {
            Some(Value::Object(servers)) => servers.contains_key(key),
            _ => false,
        },
        Err(e) => {
            warn!("key_present: cannot read {}: {e}", path.display());
            false
        }
    }
}

/// Atomic, permission-preserving write: serialize `config` (pretty + trailing
/// newline), write to a temp file IN THE TARGET'S OWN DIRECTORY (a `/tmp` temp
/// fails EXDEV across partitions), fsync, match the original file mode, then rename
/// over the target. A crash leaves the original intact, never a torn file.
fn write_atomic(path: &Path, config: &Map<String, Value>) -> Result<()> {
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    fs::create_dir_all(&parent)?;

    // Capture the original mode BEFORE we touch anything, to restore it on the temp.
    #[cfg(unix)]
    let original_mode = fs::metadata(path).ok().map(|m| {
        use std::os::unix::fs::PermissionsExt;
        m.permissions().mode()
    });

    let mut contents = serde_json::to_vec_pretty(config)?;
    contents.push(b'\n');

    let mut tmp = NamedTempFile::new_in(&parent)?;
    tmp.write_all(&contents)?;
    tmp.as_file().sync_all()?;

    #[cfg(unix)]
    if let Some(mode) = original_mode {
        use std::os::unix::fs::PermissionsExt;
        tmp.as_file().set_permissions(fs::Permissions::from_mode(mode))?;
    }

    tmp.persist(path).map_err(|e| Error::Io(e.error))?;
    debug!("write_atomic: persisted {} ({} bytes)", path.display(), contents.len());
    Ok(())
}

#[cfg(test)]
mod tests;
