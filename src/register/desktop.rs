use log::debug;

use crate::McpIo;

/// Register via a direct, Value-preserving atomic write to Claude Desktop's
/// `claude_desktop_config.json` (macOS; Linux community build). No `claude` CLI
/// exists in a Desktop-only install, so this path never shells out. Phase 3
/// implements this.
pub(crate) fn register(io: &McpIo) -> i32 {
    debug!("desktop::register: bin={} server_key={}", io.bin, io.server_key);
    todo!("Phase 3: direct atomic write to claude_desktop_config.json")
}

/// Remove via the same direct-write path. Phase 3 implements this.
pub(crate) fn unregister(io: &McpIo) -> i32 {
    debug!("desktop::unregister: bin={} server_key={}", io.bin, io.server_key);
    todo!("Phase 3: direct atomic write removing the entry")
}
