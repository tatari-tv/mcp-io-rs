use log::debug;

use crate::McpIo;
use crate::register::Target;

/// Register `io.server_key` -> `current_exe()` into Claude Code config scope
/// `target` (`user` or `project`) via `claude mcp add-json ... -s <target>`.
/// Treats `~/.claude.json` / `.mcp.json` as opaque, so there is no risk of
/// dropping unrelated global keys. Phase 3 implements this.
pub(crate) fn register(io: &McpIo, target: Target) -> i32 {
    debug!(
        "claude::register: bin={} server_key={} target={target:?}",
        io.bin, io.server_key
    );
    todo!("Phase 3: shell out to `claude mcp add-json`")
}

/// Remove `io.server_key`'s entry via `claude mcp remove`. Phase 3 implements this.
pub(crate) fn unregister(io: &McpIo, target: Target) -> i32 {
    debug!(
        "claude::unregister: bin={} server_key={} target={target:?}",
        io.bin, io.server_key
    );
    todo!("Phase 3: shell out to `claude mcp remove`")
}
