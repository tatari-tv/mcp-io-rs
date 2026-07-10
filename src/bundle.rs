use std::path::PathBuf;

use log::debug;

use crate::McpIo;

/// Package a `.mcpb` bundle for Claude Desktop / Cowork, built from the host
/// handler's advertised tool list plus [`McpIo`] metadata. Ordered after
/// slack-cli integration (Phase 5) so it is built and smoke-tested against a
/// real handler, not a stub.
pub fn bundle(io: &McpIo, out: Option<PathBuf>) -> i32 {
    debug!("bundle: bin={} server_key={} out={out:?}", io.bin, io.server_key);
    todo!("Phase 5: generate manifest.json + package .mcpb")
}
