# Design Document: Port mcp-io into persona-cli, clyde, marquee

**Author:** Scott Idler
**Date:** 2026-07-10
**Status:** Implemented
**Review Passes Completed:** 5/5
**Shipped in:** persona-cli v1.8.0, clyde v0.9.0, marquee v1.14.0 (three independent PRs, all merged + released 2026-07-10)

## Summary

- Migrate three fleet CLIs off their hand-rolled (or absent) local-MCP scaffolding and onto the shared `mcp-io` library, the same way `slack-cli` already consumes it.
- One phase per repo: persona-cli (cleanest), clyde (rmcp 1.7 -> 2.x major migration), marquee (new local stdio MCP on the CLI, remote server MCP untouched).
- No new library code. `mcp-io` v0.1.2 is tagged and shipped; this doc is pure consumer-side integration, three independent PRs.

## Problem Statement

### Background

- `mcp-io` (tatari-tv/mcp-io-rs, v0.1.2) gives any Tatari CLI a local stdio MCP server via one `mcp` subcommand (`serve`/`register`/`unregister`/`status`/`bundle`). The host embeds `Mcp(mcp_io::McpCmd)` in its clap `Command` enum, writes its own rmcp `#[tool_router]` `ServerHandler` (its tools), and dispatches via `cmd.run(&io, || Ok(handler))`. `slack-cli` is the proven first consumer (PR #11).
- The mcp-io design doc's own Addendum names this exact follow-on: "Migrating oracle/clyde/persona-cli onto mcp-io: desirable convergence, not built here. Each is a follow-on PR once the library proves out on slack-cli." It has proven out.
- Fleet state today (from the mcp-io survey table, verified by recon):
  - persona-cli: `persona mcp` subcmd, rmcp 2.1.0, ~230 lines of hand-rolled serve + runtime + token probe, manual `claude mcp add`, okta-auth, 16 tools. Its `src/mcp.rs` is the reference mcp-io was extracted from.
  - clyde: `clyde session serve` subcmd (nested), rmcp 1.7.0, hand-rolled `serve_stdio` + tracing setup, manual `claude mcp add`, no auth (local), 5 tools.
  - marquee: NO local MCP on the CLI. A separate remote streamable-http MCP (`marquee-mcp` crate, rmcp 1.8.0, 6 tools, Okta OAuth) runs in-pod inside `marquee-server`. mcp-io does not touch it.

### Problem

- Each CLI that wants a local MCP either re-derives the scaffolding by hand on a different rmcp version (persona, clyde) or has none (marquee CLI). This is the exact divergence mcp-io exists to kill: different rmcp versions, different registration steps (all manual `claude mcp add`), no `.mcpb` bundle anywhere, the `ServerInfo`-reports-as-"rmcp" gotcha unhandled (clyde), a bespoke runtime + probe per repo.
- Converging these three onto mcp-io yields: one dependency, one rmcp version per repo, one `mcp` subcommand shape, self-registration (`register`/`unregister`/`status`), a `.mcpb` bundle, and the house stdout/logging disciplines baked in.

### Goals

- persona-cli and clyde: delete the hand-rolled scaffolding, keep the tools + auth + `get_info`, wire mcp-io. Net deletion.
- marquee CLI: add a NEW local stdio MCP (mcp-io) wrapping the CLI's existing HTTP client, mirroring the remote MCP's tool names. Remote MCP byte-unchanged.
- All three self-register via `mcp register`; no more manual `claude mcp add` in any doc.
- Every host advertises its real server name (`.with_server_info`), not "rmcp".

### Non-Goals

- Touching marquee's remote streamable-http MCP (`marquee-mcp` crate). mcp-io is stdio-only; the remote surface stays its own concern, per the mcp-io design doc. Parked, revisit condition: a second CLI wants a hosted MCP AND wants to share this code.
- Bumping the remote `marquee-mcp` crate from rmcp 1.8.0 to 2.x. Separate concern with its own deploy story; not in this doc (see Resolved Decisions). Parked, revisit condition: workspace-wide rmcp consolidation is prioritized on its own.
- Migrating oracle (`sb oracle serve`) onto mcp-io. Not in Scott's list for this doc. Parked, revisit condition: Scott requests it.
- Adding remote transport, MCP resources/prompts, or a `call` verb to mcp-io. All are settled non-goals of the library itself.
- Changing any tool's behavior/output shape. The ports preserve each tool's existing contract; marquee's new tools mirror the remote ones.

## Proposed Solution

### Overview

- Each repo follows the slack-cli integration template (the mechanical reference): add `mcp-io` git dep by tag, add `Mcp(mcp_io::McpCmd)` to `Command`, early-intercept in `main` before any logging, dispatch via `cmd.run(&io, build_closure)`, add a `name()` arm and (if present) a `dispatch` bail arm, update the README, bump inside the same PR. (slack additionally allowlisted `mcp` in `.otto.yml`; verified NOT needed here - see the checklist.)
- The three ports are independent (no inter-repo dependency). Recommended order is easy -> hard so the template is exercised on the cleanest repo first: persona-cli -> clyde -> marquee.

### Architecture

- The value split is unchanged from mcp-io: the library owns the scaffolding (`mcp` subcommand, stdio lifecycle, logging discipline, self-registration, `.mcpb`); the host owns its `ServerHandler` (its tools) and its auth. The seam is `cmd.run(&io, || Ok(handler))` + a `mcp_io!()`-built `McpIo`.
- Per-repo bin/key:
  - persona-cli: crate `persona`, binary `persona` -> `mcp_io!()` (no override).
  - clyde: crate `clyde`, binary `clyde` -> `mcp_io!()` (no override).
  - marquee: crate `marquee-cli`, binary `marquee` -> `mcp_io!(bin = "marquee")` (crate != binary, exactly like slack's `bin = "slack"`).
- rmcp unification (the load-bearing constraint): mcp-io does NOT re-export rmcp, so each host depends on rmcp directly. Each host declares `rmcp = "2.1.0"`, which resolves to 2.2.0 in `Cargo.lock` (the same version mcp-io resolves); the two MUST unify or the `H: rmcp::ServerHandler` trait bound fails to compile.
  - persona-cli: already rmcp 2.1.0. No bump.
  - clyde: rmcp 1.7.0 -> 2.x. Major migration (see Data Model / clyde phase).
  - marquee CLI: adds rmcp 2.x fresh (its `cli` crate has no rmcp today); the `mcp` crate keeps 1.8.0. Two majors coexist in the workspace.

### Data Model

- Registration entry (unchanged, the pinned mcp-io contract): `{"type":"stdio","command":"<abs current_exe>","args":["mcp","serve"]}`. Every ported CLI registers with `args: ["mcp","serve"]`, which forces the `mcp` subcommand to the TOP LEVEL (clyde's `session serve` nesting cannot survive the port).
- clyde rmcp 1.7 -> 2.x migration surface (verified against rmcp 2.1.0 source):
  - `rmcp::model::Content` -> `ContentBlock` (pure rename, identical signature; 5 `Content::json` call sites in `sessions/src/mcp.rs` + the tests decode helper).
  - `Parameters<P>` extractor path unchanged (`rmcp::handler::server::wrapper::Parameters` still valid).
  - `ServerHandler`, `ServiceExt`, `ErrorData as McpError`, `tool`, `tool_handler`, `tool_router` still root-exported.
  - `get_info` gains `.with_server_info(Implementation::new("clyde", env!("CARGO_PKG_VERSION")))` (clyde omits it today -> would advertise "rmcp"). Correctness gain, not just mechanical.
  - schemars aligns (clyde 1.2.1 == mcp-io 1.2.1).
- clyde serve config (new, per Resolved Decisions): clyde's config lives in the `common::config` module (shared across clyde crates, `#[serde(deny_unknown_fields)]`, currently carrying `date-tz`), loaded via `common::config::load()`. Add `projects-dir` and `reindex-on-start` (kebab-case via serde `rename_all`) to that struct, with defaults `projects-dir = ~/.claude/projects`, `reindex-on-start = true`, plus accessors, so a Claude-Code-spawned `clyde mcp serve` (fixed args, no flags reachable) works zero-config. This replaces today's `--projects-dir` / `--no-reindex` serve-local flags. Because `deny_unknown_fields` is on, a malformed shared `clyde.yml` makes `clyde mcp serve` FAIL LOUD/CLOSED (a plus, matching the house standard, not a silent empty default).
- marquee local tool set: mirror the remote MCP's implemented tool NAMES so an agent sees identical tools locally or remote: `marquee_read`, `marquee_publish`, `marquee_slides`, `marquee_update`, `marquee_delete`. `marquee_search` is a stub remotely; it stays deferred locally too (parity with the remote's own stub status). Input/output schemas derive from the shared `contract` crate types (do not fork a second schema set), authored against rmcp 2.x/schemars.

### API Design

- Host wiring, per repo. The early-intercept mirrors slack-cli `main.rs:98-105` and renew's `Update` arm. It MUST precede any stdout write (logging init, renew notice), because once `mcp serve` runs stdout IS the JSON-RPC channel and mcp-io owns it.

persona-cli (`src/main.rs`, before the renew notice at `main.rs:96`):

```rust
if let Commands::Mcp(cmd) = &cli.command {
    let config = Config::load(...)?;              // token-free
    let auth = Arc::new(OktaAuth::new(...)?);     // token-free construction; NO probe (dropped)
    let io = mcp_io::mcp_io!();                   // crate == bin == "persona"
    std::process::exit(cmd.run(&io, || {
        Ok::<_, std::convert::Infallible>(PersonaMcpServer::new(base_url, auth, okta_issuer))
    }));
}
```

clyde (`src/main.rs`, right after arg parse, before `setup_logging`):

```rust
if let Command::Mcp(cmd) = &cli.command {
    let cfg = common::config::load()?;            // shared clyde.yml (token-free); deny_unknown_fields
    let io = mcp_io::mcp_io!();                    // crate == bin == "clyde"
    std::process::exit(cmd.run(&io, || {
        sessions::mcp::build_server(&cli.db, cfg.projects_dir(), cfg.reindex_on_start())
    }));
}
```

Note: `build_server` runs the startup reindex synchronously, and mcp-io builds the handler BEFORE it begins the stdio handshake. This preserves clyde's current `session serve` behavior (it reindexes then serves), but it means a slow reindex delays the MCP `initialize` response. Not a regression; see the reindex risk row.

marquee (`cli/src/main.rs`, mirroring the existing `Update(renew::UpdateCmd)` arm):

```rust
if let Command::Mcp(cmd) = &cli.command {
    // `Client` does NOT own auth: every cli::Client method takes `&Auth` separately.
    // Resolve the token-free AUTH SOURCE here (OktaAuth and/or --dev-email); the token
    // itself is acquired per tool call inside the handler.
    let client = Client::new(...)?;                    // HTTP client, token-free build
    let auth_src = marquee_auth_source(&cli)?;         // OktaAuth and/or dev-email, token-free
    let io = mcp_io::mcp_io!(bin = "marquee");         // crate marquee-cli, binary marquee
    std::process::exit(cmd.run(&io, || {
        Ok::<_, std::convert::Infallible>(MarqueeMcpServer::new(client, auth_src))
    }));
}
```

- Handler ownership / concurrency, per repo:
  - persona-cli: `PersonaMcpServer` (`#[derive(Clone)]`) holds `base_url`, `Arc<OktaAuth>`, `okta_issuer`, `token_lock`. Per-call token rebuild inside `spawn_blocking` (reqwest::blocking), token behind a std `Mutex` held only in the closure. Unchanged; already correct. `Config` need not be `Clone` (the handler extracts strings, not `Config`).
  - clyde: `SessionsMcpServer` (`#[derive(Clone)]`) holds `Arc<Mutex<Db>>`. Sync rusqlite under `block_in_place_compat` (lock released before serialize). No auth. Unchanged except the rmcp rename.
  - marquee: `MarqueeMcpServer` (`#[derive(Clone)]`) holds the CLI's HTTP `Client` PLUS the auth source. The `Client` does NOT own auth: every `cli::Client` method takes `&Auth` separately (`cli/src/client.rs:143-234`), resolved via `resolve_auth` (`cli/src/main.rs:278`) from `--dev-email` or `OktaAuth::get_token()`. `cli::Client` is BLOCKING (`reqwest::blocking`, `cli/src/client.rs:10`), so each tool acquires `Auth` (serialized behind a `token_lock`, held only in the closure) and calls the client INSIDE `spawn_blocking`, exactly like persona/slack. A 401 is opaque today (`eyre::Result`, not a typed error), so the handler MUST capture the HTTP status at the call site (or introduce a typed error variant) to map it to a RECOVERABLE tool error; without that seam it degrades to a protocol error. This typed-401 seam is specified in Phase 3, not deferred to the spike.
- `.with_server_info` is mandatory in every `get_info` (persona already has it; clyde gains it; marquee writes it fresh as `Implementation::new("marquee", ...)`).

### Implementation Plan

Three phases, one per repo (per Scott). Each phase is one PR in that repo, otto-ci-green, with the version bump riding inside the PR (the slack-cli precedent: no standalone bump branch). All three pin `mcp-io` v0.1.2. The `mcp-io` v0.1.2 tag already exists, so any phase can start immediately; they are independent.

Per-phase wiring checklist (applies to all three, from the slack template):
- `cargo add` the `mcp-io` git dep at tag v0.1.2 (+ `rmcp`/`tokio`/`schemars` where absent).
- `Mcp(mcp_io::McpCmd)` variant in the `Command` enum + a `name()`/help arm.
- early-intercept in `main` BEFORE any logging/stdout; `std::process::exit(cmd.run(...))`.
- `dispatch` bail arm if the repo has a lib-level dispatch (slack does; mirror `Update`).
- README: replace manual `claude mcp add` with the `mcp register`/verb table; note the re-register operator step.
- NOTE: no `.otto.yml` change is needed. slack has a skill-coverage guard that required allowlisting `mcp` in EXCEPTIONS; persona and clyde have no such guard, and marquee's `.otto.yml` guard only checks manifest/drift. Verified per repo - do not add a no-op exception.

#### Phase 1: persona-cli (cleanest port: delete + rewire)
**Model:** sonnet
- persona's `src/mcp.rs` handler, 16 tools, `get_info` with `.with_server_info("persona")`, typed `AuthFailed`/`NoMatch` seams, and the `spawn_blocking` token bridge ALL stay verbatim. rmcp is already 2.1.0; no bump.
- Change `cli.rs` `Mcp` unit variant -> `Mcp(mcp_io::McpCmd)` (breaking: `persona mcp` -> `persona mcp serve`).
- Introduce `pub fn PersonaMcpServer::new(base_url, auth, okta_issuer) -> Self` (today the struct has private fields and NO public ctor; construction happens inside `mcp::serve` at `mcp.rs:865`, which this phase deletes - so the build closure needs a public/crate-visible constructor to call).
- DELETE `main.rs::run_mcp` (the fail-fast probe + tokio runtime, `main.rs:285-301`) and `mcp::serve` (`mcp.rs:865-880`). mcp-io owns the runtime + lifecycle.
- Drop the eager token probe (Resolved Decision): rely on the per-call recoverable `AuthFailed`, which already returns the "run persona login" hint. This keeps `register`/`status`/`bundle` token-free. Consequence (matches slack): an unauthenticated `persona mcp serve` starts fine and EVERY tool call returns a recoverable `AuthFailed` with the login hint, rather than a startup error.
- Add the early `Mcp` intercept BEFORE the renew notice (`main.rs:96`), building `OktaAuth` token-free inside the closure.
- **Success criteria:**
  - `PersonaMcpServer::new(...)` exists and is callable from `main`'s build closure.
  - `persona mcp serve` handshakes over stdio; `tools/list` returns all 16 tools with `serverInfo.name == "persona"`.
  - `persona mcp register --target user` writes `{"type":"stdio","command":"<abs>/persona","args":["mcp","serve"]}`; `persona mcp status` confirms it and reports no name mismatch.
  - `persona mcp register`/`status`/`bundle` run with NO token present (token-free; `status`/`bundle` build the handler but never acquire a token, `register` does not build it). `otto ci` green; `run_mcp` and `mcp::serve` are gone.
  - `persona mcp bundle` produces a `.mcpb` whose `manifest.json` lists the 16 tool names.

#### Phase 2: clyde (rmcp 1.7 -> 2.x major migration + subcommand reshape)
**Model:** opus
- SPIKE FIRST (de-risk the #1 constraint before wiring): on a throwaway branch, `cargo add rmcp@2 --features server,macros` in the `sessions` crate, apply `Content`->`ContentBlock` + `.with_server_info("clyde")`, `cargo build -p sessions`. Success: compiles on rmcp 2.x with the `Parameters<T>` + `JsonSchema` tool pattern intact; `cargo tree -i rmcp` shows exactly one rmcp 2.x version. If it does not compile, that is discovered HERE.
- Land the rmcp bump in `sessions`; add `mcp-io` dep to `clyde/Cargo.toml`.
- Extract `sessions::mcp::build_server(db_path, projects_dir, reindex_on_start) -> Result<SessionsMcpServer>` carrying the old `serve_stdio` DB-open + startup-reindex logic; DELETE `serve_stdio`/`ServeOpts`.
- Add `projects-dir` and `reindex-on-start` (kebab-case, defaults `~/.claude/projects` / `true`) + accessors to the `common::config` struct (NOT a new `clyde::config` module); DELETE the `session serve` subcommand, `ServeArgs`, `is_serve`, `cmd_serve`, `setup_serve_tracing`, and the serve branch in the logging ladder.
- Add the top-level `Mcp(mcp_io::McpCmd)` variant + early-intercept before `setup_logging`; the 5 tools + `dispatch()` + `block_in_place_compat` + grep/read helpers stay (rmcp rename applied).
- Remove the `tracing-subscriber` direct dep IF nothing else uses it (grep first).
- Rewrite `clyde/tests/serve.rs` to drive `clyde mcp serve` (it loses the `--projects-dir`/`--no-reindex` args; drive config via a temp `clyde.yml` + `$XDG_CONFIG_HOME`).
- **Success criteria:**
  - `cargo tree -i rmcp` shows exactly ONE rmcp version (2.x) in `Cargo.lock` after the bump.
  - `clyde mcp serve` handshakes; `tools/list` returns all 5 tools with `serverInfo.name == "clyde"` (not "rmcp").
  - config: a test asserts `projects-dir`/`reindex-on-start` defaults when absent, an override from a temp `clyde.yml`, and a LOUD failure on a malformed `clyde.yml` (deny_unknown_fields).
  - `clyde mcp register --target user` writes `args:["mcp","serve"]`; the stdout-cleanliness test (`tests/serve.rs`) passes; `clyde mcp bundle` lists the 5 tool names; `session serve` and its scaffolding are gone.

#### Phase 3: marquee (new local stdio MCP on the CLI, remote MCP untouched)
**Model:** opus
- SPIKE FIRST (de-risk the two-rmcp-majors coexistence): add `mcp-io` + `rmcp@2` to `cli/Cargo.toml` only, `otto ci` on the workspace. Success: green with both `marquee-mcp` (rmcp 1.8.0) and the CLI (rmcp 2.x) compiled; `cargo tree` shows both majors as expected. Also confirm the shared `contract` crate's `JsonSchema` derives compile under rmcp 2.x/schemars in the CLI handler. If coexistence fails, that is discovered HERE before building the handler.
- Add `Mcp(mcp_io::McpCmd)` to `cli/src/cli.rs`'s `Command` enum, mirroring the existing `Update(renew::UpdateCmd)`; wire `mcp_io!(bin = "marquee")` + the early-intercept in `cli/src/main.rs`.
- Author `cli/src/mcp.rs`: `MarqueeMcpServer` over rmcp 2.x, `#[derive(Clone)]`, holding the CLI's HTTP `Client` AND the token-free auth source (`OktaAuth` and/or dev-email). Each tool resolves `Auth` per call (serialized behind a `token_lock`) and calls the blocking `cli::Client` inside `spawn_blocking`. Tools wrap `cli::Client` HTTP calls (NOT `marquee-core`/S3): `marquee_read`, `marquee_publish`, `marquee_slides`, `marquee_update`, `marquee_delete`. Schemas derive from the `contract` crate. `.with_server_info(Implementation::new("marquee", ...))`.
- Add the typed-401 seam: capture the HTTP status at the `cli::Client` call site (or introduce a typed error variant) so a 401/token failure maps to a RECOVERABLE tool error carrying a "run marquee login" hint, not a protocol error. This is real work, not a spike output.
- The remote `marquee-mcp` crate is NOT touched (stays rmcp 1.8.0). README/CLAUDE.md track the new `marquee mcp` surface and state its relationship to the remote one (same tool names, different backend: local -> HTTP-to-server, remote -> in-pod S3).
- **Success criteria:**
  - `otto ci` green on the workspace with both rmcp majors present; `marquee-mcp` is byte-unchanged (`git diff` empty for `mcp/`).
  - `marquee mcp serve` handshakes; `tools/list` returns the 5 mirrored tool names with `serverInfo.name == "marquee"`.
  - a `marquee_read` tool call in `--dev-email` mode against a dev server returns a `CallToolResult` whose payload contains the artifact's `slug` field (alongside `space`, `kind`, `content`); an unauthenticated call returns a RECOVERABLE tool error with the login hint (not a protocol error). NOTE (corrected at implementation, 2026-07-10): `marquee_read` does NOT return a `url` field - `url` is returned by `marquee_publish`/`marquee_update`, matching the remote MCP's own `ReadOutput` which also omits `url`. The original criterion's "`slug` and `url`" overstated the read contract; parity with the remote read shape is the correct bar.
  - `marquee mcp register --target user` writes a valid `args:["mcp","serve"]` entry; `marquee mcp bundle` lists the 5 tool names.

## Acceptance Criteria

- [ ] For each of persona/clyde/marquee: `<bin> mcp serve` handshakes over stdio and `tools/list` returns the expected tool set with `serverInfo.name == "<bin>"` (not "rmcp").
- [ ] For each: `<bin> mcp register --target user` writes `{"type":"stdio","command":"<abs current_exe>","args":["mcp","serve"]}` and `<bin> mcp status` confirms presence with no name mismatch.
- [ ] persona-cli and clyde delete ALL hand-rolled MCP scaffolding (serve lifecycle, manual registration, bespoke logging/tracing); no `claude mcp add` remains in either repo's docs.
- [ ] clyde's `Cargo.lock` contains exactly one rmcp version (2.x) after the bump; marquee's workspace builds with both rmcp 1.8.0 (remote, unchanged) and 2.x (CLI) present.
- [ ] marquee's local tools mirror the remote MCP's implemented tool names; the remote `marquee-mcp` crate is byte-unchanged.
- [ ] For each: `<bin> mcp bundle` produces a `.mcpb` whose `manifest.json` lists the host's tool names (bundle is provided by mcp-io; smoke-tested once per repo).

## Resolved Decisions

- 2026-07-10 (Scott): one phase per repo (persona-cli, clyde, marquee); each is one PR, bump inside the PR.
- 2026-07-10 (Scott): marquee gets a NEW local stdio MCP on the CLI, mirroring the remote MCP's implemented tool names; the remote streamable-http MCP is untouched, and the two sit alongside cleanly (separate processes/transports/backends; two rmcp majors coexist in the workspace).
- 2026-07-10 (Scott): persona drops the eager fail-fast token probe; the per-call recoverable `AuthFailed` already carries the "run persona login" hint, and dropping it keeps `register`/`status`/`bundle` token-free.
- 2026-07-10 (Scott): clyde's serve config (`projects-dir`, `reindex-on-start`) moves to `~/.config/clyde/clyde.yml` with zero-config defaults, since a Claude-Code-spawned `mcp serve` gets fixed args and cannot receive flags.
- 2026-07-10 (Scott, default pending pushback): the remote `marquee-mcp` crate stays on rmcp 1.8.0. Bumping it to 2.x is a separate concern with its own deploy story; not folded into this doc. Revisit condition: a workspace-wide rmcp consolidation is prioritized.
- 2026-07-10 (default pending pushback): all three repos pin `mcp-io` v0.1.2 (the current tag; slack-cli is already on it). No lockstep bump needed.
- 2026-07-10 (default pending pushback): recommended order is persona -> clyde -> marquee (cheap/deterministic first, expensive/risky last); the three are independent, so parallel is possible if desired.
- 2026-07-10 (panel, both reviewers): clyde ACCEPTS the loss of rmcp/tokio internal `tracing` capture (mcp-io routes only the `log` facade to a file). Those events never hit stdout, so protocol safety holds; only diagnostics are lost, matching the rest of the fleet (siblings behave identically). If clyde later needs them, its own tracing subscriber must be installed BEFORE `cmd.run` (mcp-io logging is already initialized inside it), not in the build closure.
- 2026-07-10 (panel, verified): marquee's `cli::Client` is BLOCKING (`reqwest::blocking`), so the handler uses `spawn_blocking` per call (persona pattern). Not left to the spike.

## Alternatives Considered

### Alternative 1: Keep the hand-rolled scaffolding per repo (status quo)
- **Description:** leave persona/clyde on their bespoke serve/register/logging; leave marquee CLI with no local MCP.
- **Cons:** the exact divergence mcp-io exists to kill (different rmcp versions, manual registration, no bundle, the "rmcp"-name gotcha unhandled in clyde).
- **Why not chosen:** this is the problem. Rejected.

### Alternative 2: Fold marquee's remote MCP into mcp-io
- **Description:** absorb marquee's streamable-http/OAuth surface into the shared library so there is one MCP.
- **Cons:** pulls axum + OAuth discovery into a stdio-only library; explicit mcp-io non-goal; the remote MCP has a deploy story stdio does not.
- **Why not chosen:** out of scope by the library's own charter. The local and remote surfaces are complementary, not a merge target.

### Alternative 3: Keep clyde's MCP nested as `clyde session serve`
- **Description:** preserve the `session serve` nesting rather than promoting to top-level `clyde mcp`.
- **Cons:** mcp-io's registration writes a fixed `args: ["mcp","serve"]` contract; the nesting is structurally incompatible with `register`/`bundle`.
- **Why not chosen:** the port forces top-level `mcp`; top-level also matches the org standard (`slack mcp`, `persona mcp`) so siblings behave identically.

## Technical Considerations

### Dependencies
- Each host adds `mcp-io = { git = "https://github.com/tatari-tv/mcp-io-rs", tag = "v0.1.2", version = "0.1.2" }`, plus `rmcp = "2.1.0"` (features `server`, `macros`), `tokio` (`rt-multi-thread`, `io-std`, `macros`), and `schemars` where absent. All via `cargo add`.
- persona-cli: rmcp/tokio/schemars already present; only `mcp-io` is new.
- clyde: `sessions` crate rmcp 1.7.0 -> 2.x; `mcp-io` new on `clyde`. CORRECTION (2026-07-10, found at implementation): the original premise "private-git plumbing already works (renew is a git-dep-by-tag)" was WRONG. `renew` is a PUBLIC repo, so it only ever proved *public* git-dep plumbing. clyde is itself PUBLIC (since 2026-07-08) with a deliberately credential-less CI and had NO private git deps before this port. Adding the then-private `mcp-io-rs` broke clyde's CI in 31s (`failed to authenticate` cloning the private dep) - a public runner cannot clone a private dependency. RESOLUTION: `mcp-io-rs` was made PUBLIC on 2026-07-10, so clyde clones it anonymously exactly like `renew`, with zero CI change and no injected token. See the "mcp-io-rs made public" addendum below.
- marquee: `cli` crate gains `mcp-io` + rmcp 2.x + tokio; the `mcp` crate's rmcp 1.8.0 is left alone (two majors in `Cargo.lock`).

### Performance
- Irrelevant at this scale (local single-user stdio children). Carry the rust-rules disciplines already in place: never hold a lock across `.await`; blocking host work (persona/marquee reqwest::blocking, clyde rusqlite) runs under `spawn_blocking`/`block_in_place`.

### Security
- mcp-io handles no tokens. Each host's auth is unchanged: persona (okta-auth v0.6.0, per-call rebuild), clyde (none, local), marquee (the CLI's existing okta-auth token cache). A missing/expired token surfaces as a recoverable per-call tool error.
- Registration: Claude Code targets delegate to `claude mcp add-json` (opaque-safe); desktop is the Value-preserving atomic write. No `env` block, no secrets in the entry.
- marquee's local MCP has the SAME privilege as the marquee CLI already running on the user's laptop (HTTP to marquee-server, okta-authed) - no new capability or blast radius beyond what the CLI already grants.

### Testing Strategy
- Per repo: a headless `initialize` + `notifications/initialized` + `tools/list` handshake asserting the tool set + `serverInfo.name`. A `register -> status -> unregister` round-trip in an isolated `$CLAUDE_CONFIG_DIR`. clyde keeps its rewritten stdout-cleanliness integration test. persona/clyde keep their existing dispatch/handler unit tests (rmcp rename applied for clyde).
- Tests-must-bite per the house rule: break a tool-name propagation / a `.with_server_info` and watch the handshake assertion fail before trusting it.

### Rollout Plan
- Three independent PRs, one per repo, each otto-ci-green, bump inside the PR, pin mcp-io v0.1.2. No inter-repo ordering dependency; recommended sequence persona -> clyde -> marquee.
- Breaking CLI change (persona `persona mcp` -> `persona mcp serve`; clyde `clyde session serve` -> `clyde mcp serve`). Existing `claude mcp add` configs that invoke the old form BREAK on upgrade. The re-register step is an explicit OPERATOR ACTION, stated in each PR/README:
  - `<bin> mcp register --target user` - run UNCONDITIONALLY after upgrade. mcp-io's `register` replaces the entry value idempotently (it is derived from `current_exe()` + fixed `args:["mcp","serve"]`), so re-registering overwrites a stale `session serve`/`mcp` entry in place. Do NOT rely on `mcp status` to detect staleness: it only checks whether the key is PRESENT, not whether its `command`/`args` are current, so a stale entry reads as "registered."
- persona and clyde are `renew`-self-updating published binaries; a user who upgrades before re-registering gets a broken MCP entry until they run `register`. Call it out in the release notes.

## Risks and Mitigations

| Risk | Likelihood | Impact | Mitigation |
|------|------------|--------|------------|
| clyde rmcp 1.7 -> 2.x migration does not compile / breaks the `Parameters<T>` tool pattern | Med | High | Spike FIRST in Phase 2 (compile `sessions` on rmcp 2.x before any wiring); the rename surface is verified and small |
| clyde rmcp version splits (2.1.x vs 2.2.0) and the `H: ServerHandler` bound fails to unify | Med | High | `cargo tree -i rmcp` must show exactly one rmcp version after the bump (a Phase 2 success criterion) |
| marquee two rmcp majors do not coexist in one workspace build | Low | High | Spike FIRST in Phase 3 (`otto ci` with both crates); different majors legally coexist in cargo when no type crosses the boundary (the CLI handler is authored fresh, never reuses `marquee-mcp`) |
| Breaking `persona mcp` / `clyde session serve` surface change orphans existing configs | High | Med | Explicit UNCONDITIONAL `register` step in each README + release notes (idempotent, overwrites the stale entry). `mcp status` canNOT detect staleness (key-presence only), so do not lean on it |
| A stdout write (renew notice, logging init) precedes JSON-RPC framing and corrupts the protocol | Med | High | Early-intercept BEFORE any logging/notice in every repo (the slack ordering); mcp-io owns stdout + file logging |
| clyde startup reindex (default on) delays the MCP `initialize` handshake when the catalog is large | Low | Med | Not a regression (`session serve` already reindexes then serves); `reindex-on-start` is config-tunable to `false`; revisit an incremental/async reindex if it bites |

## Open Questions

None. All three prior questions were closed against the code during review (see Review Panel Disposition): clyde tracing loss -> accepted; marquee `cli::Client` -> confirmed blocking; `.otto.yml` exception -> confirmed not needed for any of the three repos.

## Review Panel Disposition (2026-07-10)

Architect (Gemini) + Staff Engineer (Codex), Design Review mode; both read the target repos. Every finding dispositioned; none dropped, none contradicts a Resolved Decision.

- **FOLDED IN (blockers verified against code):**
  - marquee `Client` does not own auth (every `cli::Client` method takes `&Auth`) -> handler holds `Client` + auth source, resolves `Auth` per call in `spawn_blocking`; ownership written into Data Model + Phase 3, not left to the spike.
  - persona has no public `PersonaMcpServer::new` (construction lived in the deleted `mcp::serve`) -> added as an explicit Phase 1 bullet + success criterion.
  - marquee 401 was an unbacked claim (opaque `eyre::Result`, no typed 401) -> added the typed-401 seam (capture HTTP status at the call site) as real Phase 3 work.
  - clyde config is `common::config` (`deny_unknown_fields`, carries `date-tz`), not a `clyde::config` module -> corrected the module/struct/accessors + fail-closed-on-malformed + config tests.
  - re-register mitigation over-claimed (`status` only checks key presence) -> rollout now says run `register` UNCONDITIONALLY (idempotent overwrite); dropped the false "status reports absence."
  - per-phase acceptance criteria tightened (persona ctor exists; clyde config defaults/override/malformed; marquee named response fields + dev-auth mode; `bundle` smoked per phase).
  - factual fixes: rmcp declared 2.1.0 / resolves 2.2.0; clyde has 5 `Content::json` sites; `status`/`bundle` build the handler (only `register`/`unregister` do not).
- **CLOSED OPEN QUESTIONS (locally verified, per the author-closes-questions rule):**
  - clyde tracing loss -> ACCEPT (both reviewers; siblings behave identically).
  - marquee `cli::Client` -> BLOCKING (verified `reqwest::blocking`); `spawn_blocking`, not a spike output.
  - `.otto.yml` `mcp` exception -> NOT needed for persona/clyde/marquee (only slack had the guard); dropped the no-op step.
- **NOTED / KEPT AS RISK:** clyde synchronous startup reindex can exceed an MCP client's `initialize` timeout on a large catalog; kept as a risk with the `reindex-on-start=false` escape hatch (both reviewers raised it).
- **NO PHASING PUSHBACK:** neither reviewer called one-phase-per-repo too big; the spike-first structure for clyde and marquee is judged adequate; two-rmcp-major coexistence confirmed architecturally sound.

## References
- mcp-io library: `tatari-tv/mcp-io-rs` (`docs/design/2026-07-09-mcp-io-rs.md`, `README.md`, `src/lib.rs`, `src/cmd.rs`).
- Reference integration: `tatari-tv/slack-cli` PR #11 (`src/mcp.rs`, `src/main.rs:98-105`, `.otto.yml`).
- persona-cli current MCP: `tatari-tv/persona-cli/src/mcp.rs` (the pattern mcp-io was extracted from), `src/main.rs:285-301` (`run_mcp`).
- clyde current MCP: `tatari-tv/clyde/main/sessions/src/mcp.rs`, `sessions/Cargo.toml:18` (rmcp 1.7.0).
- marquee CLI + remote MCP: `tatari-tv/marquee/main/cli/src/client.rs`, `mcp/src/lib.rs` (remote, rmcp 1.8.0), `contract` crate (shared wire types).

## Implementation Addendum (2026-07-10)

Recorded after all three phases shipped. Captures the one decision the plan did not anticipate and the road not taken.

### mcp-io-rs made PUBLIC (the plan's one wrong premise, resolved)

- The plan assumed clyde could consume the (then-private) `mcp-io-rs` git dep because "renew already works." That was wrong: `renew` is public, clyde is public with credential-less CI, and clyde had no private git deps. Adding private `mcp-io-rs` failed clyde CI in 31s on a `failed to authenticate` clone.
- Two resolutions were weighed: (A) make `mcp-io-rs` public so clyde clones it anonymously like `renew`; (B) inject `ACTIONS_CLONE_SUBMODULE_TOKEN` + `insteadOf` into public clyde's CI (the pattern the *private* consumers - slack-cli/persona/marquee - already use). B was rejected: it re-introduces the private-credential coupling clyde deliberately shed when it went public, and puts an org token into a public repo's CI.
- **Chosen: A.** `mcp-io-rs` was flipped to public on 2026-07-10. Due diligence first: no private transitive deps (crates.io only), no secrets in content or history, MIT-licensed. Private consumers keep cloning it unchanged (a public dep needs no token); public clyde and any external `cargo install` now work credential-free.
- Related, noted-not-done: `okta-auth-rs` and `okta-auth-py` are similarly publishable (PKCE public clients, no secret; `okta-auth-rs/src/tatari.rs` already commits the client_id with a cited "public client identity" doc comment). Not required for this port; parked for a broader "publish the shared Tatari CLI libs" decision.

### Release facts

- Shipped as three independent PRs, each merged then released via an annotated tag (the fleet `build.rs` derives `--version` from `git describe`, so the tag is the version of record): persona-cli `v1.8.0` (#45), clyde `v0.9.0` (#40), marquee `v1.14.0` (#64). All three release workflows published the full binary matrix (linux amd64/arm64, macos arm64/x86_64).
- Bump level: persona rode a minor bump inside its PR (1.7.0 -> 1.8.0). clyde and marquee rode *patch* bumps inside their PRs (their `Cargo.toml` fallbacks read 0.8.3 / 1.13.5) but were released as *minor* tags per Scott (v0.9.0 / v1.14.0). Because `git describe` drives the version, the released binaries report the tag; the two `Cargo.toml` fallbacks trail cosmetically and true up in each repo's next feature PR (no bump-only PR, per the git rules).

### marquee_read contract correction

- Phase 3 success criterion #3 originally asserted `marquee_read` returns `slug` and `url`. Corrected inline: `marquee_read` returns `slug` (with `space`, `kind`, `content`) but NOT `url`, matching the remote MCP's own `ReadOutput`. `url` is a publish/update output. The remaining criteria held as written.
