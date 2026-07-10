# mcp-io-rs implementation notes

Append-only. One section per phase. Companion to
`docs/design/2026-07-09-mcp-io-rs.md`.

## Phase 0: Prove the generic serve seam (spike, throwaway)

### Design decisions
- Proved the seam with a real headless stdio handshake, not the GUI MCP Inspector -- `examples/spike.rs` -- piping an `initialize` + `notifications/initialized` + `tools/list` JSON-RPC sequence into `cargo run --example spike` over stdin. Reproducible in CI/headless, no manual client. The doc offered "MCP Inspector or `claude mcp`"; a piped JSON-RPC sequence is a real stdio MCP client and is deterministic.
- Generic seam shape (this is the exact pattern Phase 2 reproduces as production code):
  - Imports:
    ```rust
    use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerInfo};
    use rmcp::{ErrorData as McpError, ServerHandler, ServiceExt, tool, tool_handler, tool_router};
    ```
  - Generic function signature (`serve<H>` in `examples/spike.rs`):
    ```rust
    async fn serve<H: ServerHandler + Send + 'static>(handler: H) -> eyre::Result<()> {
        let service = handler.serve((tokio::io::stdin(), tokio::io::stdout())).await?;
        let quit_reason = service.waiting().await?;
        // quit_reason is a QuitReason; on stdin EOF it is `Closed`.
        Ok(())
    }
    ```
    `.serve(...)` comes from the `ServiceExt` trait (must be in scope). Lifecycle is `handler.serve((stdin, stdout)).await` -> `service.waiting().await` -> `Ok(quit_reason)`; `waiting()` returns the quit reason by value.
  - `.with_server_info` call (defeats the rmcp-reports-itself-as-"rmcp" gotcha), inside `get_info` on the `#[tool_handler] impl ServerHandler`:
    ```rust
    ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
        .with_server_info(Implementation::new("spike", env!("CARGO_PKG_VERSION")))
        .with_instructions("...".to_string())
    ```
  - Tool router wiring: `#[tool_router]` on the tools `impl` generates `Self::tool_router()`; `#[tool_handler]` on the `impl ServerHandler` picks it up automatically -- no `tool_router` struct field required (confirmed against `persona-cli/src/mcp.rs`, which has no such field).
- Success `CallToolResult` uses `ContentBlock::text(...)` (rmcp 2.1.0 has no `Content` re-export in `rmcp::model`; the type is `ContentBlock`, matching `persona-cli`).
- All spike logging goes to STDERR (`eprintln!`) because stdout is the JSON-RPC protocol channel. Verified: stdout carried only two JSON-RPC frames; the four lifecycle lines landed on stderr.

### Deviations
- `serve<H>` returns `eyre::Result<()>` in the spike; the production library will return the crate's `thiserror` `Result`. Correct for a throwaway example (eyre is a dev-dependency here, per the doc); Phase 2 swaps the error type. Same effect, correct seam.
- Verification method is a piped JSON-RPC handshake rather than the MCP Inspector named in the doc's Phase 0 bullet. Same protocol, headless and deterministic; lists exactly the dummy tool as the criterion requires.
- Package `edition = "2021"` and `version = "0.0.0"` for the spike crate (per the Phase 0 task brief). Phase 1 scaffolds the real crate; the fleet standard is edition 2024, so Phase 1 should set edition 2024 and a real starting version.

### Tradeoffs
- Piped-stdin handshake vs a spawned MCP client library: the pipe is zero extra deps and CI-friendly; a client lib would exercise more of the protocol but is unnecessary to de-risk the one seam (generic serve compiles + handshakes + lists the tool). Chose the pipe.

### Open questions
- None.

## Phase 1: Scaffold the crate

### Design decisions
- `mcp_io!()` is deliberately simpler than `renew!()`: renew's helper macro
  (`__renew_current_version!`) exists because version selection has real
  fallback logic (`option_env!` -> `env!`); `bin`/`version` here are plain
  `env!()` reads with no fallback, so `mcp_io!` inlines them directly in each
  match arm rather than factoring out an unneeded helper macro (`src/lib.rs`).
- `McpCmd::run<H, F, E>`'s dispatch never lets a stub arm fall through to a
  second `todo!()`: every arm is a tail call into exactly one `todo!()` (either
  directly, or through one level of orchestration, e.g. `register::register`
  dispatching to `claude::register`/`desktop::register`). This keeps
  `-D warnings` happy (a `todo!()` diverges, so any code after it in the same
  arm is unreachable and would fail CI) and it is the CORRECT seam Phase 2/3/5
  fill in, not a shortcut (`src/cmd.rs`, `src/register/mod.rs`).
- `Target` (the register/unregister `--target` enum) lives in
  `src/register/mod.rs`, not `cmd.rs`, per the design's module-layout comment
  ("register/mod.rs: target selection"); `cmd.rs` imports it. Follows the
  house cli.md rule: `#[clap(rename_all = "kebab-case")]` on the enum +
  `ignore_case = true` on the arg, verified by a case-insensitive parse test
  (`src/cmd/tests.rs`).
- `register/claude.rs` and `register/desktop.rs` got real (if `todo!()`-bodied)
  `register`/`unregister` functions rather than comment-only placeholders,
  because `register::register`/`unregister` already need somewhere concrete to
  dispatch to for the seam to be correct; each stub's `todo!()` message names
  its own mechanism (`claude mcp add-json` vs the direct atomic write) so the
  dispatch tests (`should_panic(expected = ...)`) prove the routing is right
  without pretending the logic exists.
- `xdg_config_dir()`/`xdg_data_dir()` are re-exported at the crate root
  (`pub use config::{McpIo, xdg_config_dir, xdg_data_dir};`) even though the
  design's lib.rs bullet only names `McpCmd, serve, McpIo, Error, Result,
  mcp_io!`. Rationale in Deviations below.
- `DummyHandler` (a unit struct with a bare `impl rmcp::ServerHandler for
  DummyHandler {}`) is enough to exercise `McpCmd::run`'s full generic
  dispatch in tests: rmcp 2.1.0's `ServerHandler` trait defaults every method
  (`server_handler_methods!()`, `#[allow(unused_variables)]` at the trait
  level), so no `#[tool_router]`/`#[tool_handler]` machinery is needed until a
  real handler shows up in Phase 2/4 (`src/cmd/tests.rs`).
- Added `log` as a real dependency (not just a stub reference) so the
  function-level DEBUG logging Scott's logging rule requires (`McpIo::new`,
  `McpCmd::run`, every stub's entry) has somewhere to go; matches `renew`/
  `okta-auth-rs`, which both depend on `log` directly. No `env_logger` yet -
  Phase 2 owns the logging-discipline helper that decides where the file
  target actually points.

### Deviations
- The Phase 0 handoff and design's dep list (`rmcp, tokio, schemars, serde,
  serde_json, clap, dirs, thiserror`) did not name `log`. Added it anyway
  (see Design decisions) because instruction #7 ("add house function-level
  debug logging") is unsatisfiable without a logging facade. Same effect as
  the rest of the fleet; just not spelled out in the explicit dep list.
- `xdg_config_dir()`/`xdg_data_dir()` are `pub` at the crate root. Without a
  re-export (or a caller), `#![deny(dead_code)]` fails the build: `mod
  config;` is private, so a `pub fn` inside it is not externally reachable and
  is NOT called yet (Phase 3's `desktop.rs` is the first real caller, and
  building out the macOS-vs-Linux path resolution now would be scope creep
  into Phase 3). Re-exporting them matches the sibling-crate convention
  (`renew::xdg_data_dir`, `okta-auth::xdg_data_dir` are both `pub fn` at their
  crate roots) and keeps them exercised by the mandatory platform-path tests
  (`src/config/tests.rs`) instead of dead.
- `run_bundle` discards its `build` closure entirely (`let _ = build;`)
  rather than threading `H`'s tool list into `bundle::bundle`. The design
  says bundle's manifest is generated from "the host handler's advertised
  tool list", which needs the built handler; wiring that through is Phase 5's
  job (ordered after slack-cli integration per the doc). `bundle()`'s
  signature will change in Phase 5 to accept it.
- Package `.gitignore`/`README.md`/`docs/` were untouched; Phase 6 owns the
  README integration guide.

### Tradeoffs
- Comment-only module stubs vs `todo!()`-bodied functions for
  `register/claude.rs` and `register/desktop.rs`: chose real functions
  (called from `register::register`/`unregister`'s match) over doc-comment
  placeholders, because a called stub proves the dispatch wiring under test
  (`should_panic` per branch) where a comment-only file proves nothing. The
  cost is `todo!()` messages that must stay in sync with Phase 3's real
  implementation; low cost, high signal.
- `should_panic(expected = "...")` tests for every `McpCmd::run` arm and every
  `register`/`unregister`/`status` branch vs skipping tests until real logic
  lands: chose to test now. It is the only way to prove the dispatch tree is
  wired correctly in Phase 1, and it stays honest (the panic message names
  the exact phase that will replace it) rather than faking a passing result.

### Open questions
- None.
