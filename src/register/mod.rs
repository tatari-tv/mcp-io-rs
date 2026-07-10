mod claude;
mod desktop;

use std::fmt::Display;

use clap::ValueEnum;
use log::{debug, warn};
use rmcp::ServerHandler;

use crate::McpIo;
use crate::error::{Error, Result};

/// Process exit codes, mirroring `cmd.rs` (renew's contract: the host exits these).
const EXIT_SUCCESS: i32 = 0;
const EXIT_FAILURE: i32 = 1;

/// Claude Code / Claude Desktop registration target. Selects which mechanism
/// `register`/`unregister`/`status` use: Claude Code `user`/`project` delegate
/// to `claude mcp add-json` (opaque-safe, idempotent); `desktop` is a direct
/// Value-preserving atomic write (no `claude` CLI in a Desktop-only install).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub enum Target {
    /// Claude Code user scope (`~/.claude.json`, honoring `$CLAUDE_CONFIG_DIR`).
    User,
    /// Claude Code project scope (`./.mcp.json`).
    Project,
    /// Claude Desktop config (macOS; Linux community build).
    Desktop,
}

/// Every target, in report order, for `status` to survey.
const ALL_TARGETS: [Target; 3] = [Target::User, Target::Project, Target::Desktop];

impl Target {
    /// Human label for messages (matches the `--target` value).
    fn label(self) -> &'static str {
        match self {
            Target::User => "user",
            Target::Project => "project",
            Target::Desktop => "desktop",
        }
    }
}

/// Resolve the absolute path to the running binary so a registered `command`
/// points at THIS build, not a `$PATH` guess. Shared by the claude and desktop
/// mechanisms.
pub(crate) fn current_exe() -> Result<String> {
    let exe = std::env::current_exe().map_err(Error::CurrentExe)?;
    Ok(exe.display().to_string())
}

/// Register this build's `server_key` -> `current_exe()` entry into `target`.
/// Returns a process exit code (renew's contract).
pub fn register(io: &McpIo, target: Target) -> i32 {
    debug!(
        "register: bin={} server_key={} target={target:?}",
        io.bin, io.server_key
    );
    let result = match target {
        Target::User | Target::Project => claude::register(io, target),
        Target::Desktop => desktop::register(io),
    };
    finish("register", io, target, result)
}

/// Remove this build's entry from `target`.
pub fn unregister(io: &McpIo, target: Target) -> i32 {
    debug!(
        "unregister: bin={} server_key={} target={target:?}",
        io.bin, io.server_key
    );
    let result = match target {
        Target::User | Target::Project => claude::unregister(io, target),
        Target::Desktop => desktop::unregister(io),
    };
    finish("unregister", io, target, result)
}

/// Map a register/unregister `Result` to an exit code, reporting the outcome to
/// STDERR (the crate denies `print_stdout`; stderr keeps the protocol channel clean).
fn finish(verb: &str, io: &McpIo, target: Target, result: Result<()>) -> i32 {
    match result {
        Ok(()) => {
            eprintln!("{}: {verb}ed '{}' ({})", io.bin, io.server_key, target.label());
            debug!("{verb}: ok server_key={} target={target:?}", io.server_key);
            EXIT_SUCCESS
        }
        Err(e) => {
            warn!("{verb}: failed server_key={} target={target:?}: {e}", io.server_key);
            eprintln!("{}: {verb} failed ({}): {e}", io.bin, target.label());
            EXIT_FAILURE
        }
    }
}

/// Report where `io.server_key` is registered across all targets, and warn if the
/// host's handshake name (from `get_info`) does not match `bin` (the
/// rmcp-reports-itself-as-"rmcp" gotcha). `build` is token-free (register/status
/// never need a token); a build failure only skips the name check, it is not fatal.
pub fn status<H, F, E>(io: &McpIo, build: F) -> i32
where
    H: ServerHandler + Send + 'static,
    F: FnOnce() -> std::result::Result<H, E>,
    E: Display,
{
    debug!("status: bin={} server_key={}", io.bin, io.server_key);

    for target in ALL_TARGETS {
        let present = is_registered(io, target);
        let mark = if present { "registered" } else { "not registered" };
        eprintln!("{}: {} -> {mark}", io.server_key, target.label());
        debug!("status: target={target:?} present={present}");
    }

    // Serve-readiness: the host must set `.with_server_info(Implementation::new(bin,..))`,
    // or rmcp reports the server as "rmcp". Build (token-free) and inspect get_info.
    match build() {
        Ok(handler) => {
            let name = handler.get_info().server_info.name;
            if name == io.bin {
                debug!("status: handshake name '{name}' matches bin");
            } else {
                warn!(
                    "status: handshake name '{name}' != bin '{}' (host must call \
                     .with_server_info(Implementation::new(\"{}\", ..)))",
                    io.bin, io.bin
                );
                eprintln!(
                    "{}: WARNING handshake name is '{name}', expected '{}'; the host is \
                     missing .with_server_info(Implementation::new(\"{}\", ..))",
                    io.bin, io.bin, io.bin
                );
            }
        }
        Err(e) => {
            warn!("status: could not build handler to verify handshake name: {e}");
            eprintln!("{}: could not verify handshake name: {e}", io.bin);
        }
    }
    EXIT_SUCCESS
}

/// Read-only presence check for one target (writes never go through here).
fn is_registered(io: &McpIo, target: Target) -> bool {
    match target {
        Target::User | Target::Project => match claude::config_path(target) {
            Ok(path) => desktop::key_present(&path, &io.server_key),
            Err(e) => {
                warn!("is_registered: cannot resolve {} path: {e}", target.label());
                false
            }
        },
        Target::Desktop => match desktop::config_path() {
            Ok(path) => desktop::key_present(&path, &io.server_key),
            Err(e) => {
                warn!("is_registered: cannot resolve desktop path: {e}");
                false
            }
        },
    }
}

/// Serialize every test that mutates process env (`CLAUDE_CONFIG_DIR`) across the
/// `register` submodules, since env is process-global and tests run in parallel.
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests;
