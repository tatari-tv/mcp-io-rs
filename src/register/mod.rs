mod claude;
mod desktop;

use std::fmt::Display;
use std::path::PathBuf;

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

/// The basename of the running binary, for the ownership check's accepted command
/// set. `None` when `current_exe()` is unavailable, which simply drops that rung
/// (the `io.bin` rung still stands) rather than widening the set.
pub(crate) fn current_exe_basename() -> Option<String> {
    std::env::current_exe()
        .ok()?
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_string)
}

/// Whether `io.server_key`'s EXISTING entry in a config belongs to this tool.
/// Produced by [`desktop::entry_is_ours`]; consumed by the [`guard`] in front of
/// every destructive write and by [`status`].
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Ownership {
    /// The entry is one we wrote (or one an older build of ours wrote).
    Ours,
    /// The key is taken by something that is not us. Carries the entry's existing
    /// `command` when it has a string one, `None` when it does not (a non-stdio or
    /// malformed entry), so the refusal can say which without inventing a value.
    Foreign { command: Option<String> },
    /// The key is not present at all.
    Absent,
}

/// How a foreign entry's `command` reads in a refusal. Spelled out rather than
/// printing an empty string when the entry has no string `command`.
fn describe(command: Option<String>) -> String {
    match command {
        Some(command) => format!("`{command}`"),
        None => "an entry with no string `command` (non-stdio, or malformed)".to_string(),
    }
}

/// The config file a `target` resolves to. Shared by the read-only checks
/// ([`is_registered`], [`ownership`]); writes go through each mechanism.
fn config_path(target: Target) -> Result<PathBuf> {
    match target {
        Target::User | Target::Project => claude::config_path(target),
        Target::Desktop => desktop::config_path(),
    }
}

/// Ownership of `io.server_key`'s entry in `target`'s config.
fn ownership(io: &McpIo, target: Target) -> Result<Ownership> {
    let path = config_path(target)?;
    Ok(desktop::entry_is_ours(io, &path))
}

/// The fail-closed check in front of every destructive write. `register` removes
/// before it re-adds and `unregister` removes outright, so BOTH verbs refuse a key
/// that is taken by somebody else's server. Ours and absent proceed; `--force`
/// skips the check entirely (the intentional-replacement / cleanup path). A config
/// path we cannot even resolve refuses too: an unverifiable entry is not a safe one.
fn guard(io: &McpIo, target: Target, force: bool) -> Result<()> {
    if force {
        debug!("guard: --force, skipping the ownership check for {}", io.server_key);
        return Ok(());
    }
    match ownership(io, target)? {
        Ownership::Ours | Ownership::Absent => Ok(()),
        Ownership::Foreign { command } => Err(Error::ForeignEntry {
            key: io.server_key.clone(),
            target: target.label(),
            command: describe(command),
        }),
    }
}

/// Register this build's `server_key` -> `current_exe()` entry into `target`.
/// Returns a process exit code (renew's contract).
pub fn register(io: &McpIo, target: Target, force: bool) -> i32 {
    debug!(
        "register: bin={} server_key={} target={target:?} force={force}",
        io.bin, io.server_key
    );
    let result = guard(io, target, force).and_then(|()| match target {
        Target::User | Target::Project => claude::register(io, target),
        Target::Desktop => desktop::register(io),
    });
    finish("register", io, target, result)
}

/// Remove this build's entry from `target`.
pub fn unregister(io: &McpIo, target: Target, force: bool) -> i32 {
    debug!(
        "unregister: bin={} server_key={} target={target:?} force={force}",
        io.bin, io.server_key
    );
    let result = guard(io, target, force).and_then(|()| match target {
        Target::User | Target::Project => claude::unregister(io, target),
        Target::Desktop => desktop::unregister(io),
    });
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

    // Ownership, not presence: "registered" alone is what let a foreign entry under
    // our key look like ours. Every state still exits 0 -- status only reports.
    for target in ALL_TARGETS {
        match ownership(io, target) {
            Ok(state) => {
                let report = match state {
                    Ownership::Ours => "registered (ours)".to_string(),
                    Ownership::Foreign { command } => format!(
                        "registered (foreign) to {}; re-run register with --force to replace it",
                        describe(command)
                    ),
                    Ownership::Absent => "not registered".to_string(),
                };
                eprintln!("{}: {} -> {report}", io.server_key, target.label());
                debug!("status: target={target:?} report={report}");
            }
            Err(e) => {
                warn!("status: cannot resolve {} path: {e}", target.label());
                eprintln!("{}: {} -> unknown ({e})", io.server_key, target.label());
            }
        }
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

/// Read-only presence check for one target (writes never go through here). Kept
/// alongside [`ownership`]: the Claude Code path needs plain PRESENCE to decide
/// whether a re-add has to `remove` first, after the guard has already ruled the
/// entry ours.
fn is_registered(io: &McpIo, target: Target) -> bool {
    match config_path(target) {
        Ok(path) => desktop::key_present(&path, &io.server_key),
        Err(e) => {
            warn!("is_registered: cannot resolve {} path: {e}", target.label());
            false
        }
    }
}

/// Serialize every test that mutates process env (`CLAUDE_CONFIG_DIR`) across the
/// `register` submodules, since env is process-global and tests run in parallel.
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests;
