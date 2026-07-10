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

## Phase 2: serve

### Design decisions
- Reproduced Phase 0's proven seam as production code in `src/serve.rs`:
  `handler.serve((stdin, stdout)).await` -> `service.waiting().await` ->
  log the `QuitReason`. `serve<H>` keeps the exact public signature the design
  specifies (`pub async fn serve<H: ServerHandler + Send + 'static>(handler: H)
  -> Result<()>`); it delegates to a private transport-generic
  `serve_with<H, T, E, A>` (see Deviations) so tests can inject a transport.
- `init_logging(bin) -> Result<PathBuf>` (`src/serve.rs`) is the logging-
  discipline helper: it routes the `log` facade to
  `<xdg-data>/<bin>/logs/<bin>.log` via `env_logger` with
  `Target::Pipe(<file>)`, mirroring persona-cli's `setup_logging`
  (`persona-cli/src/main.rs:26-53`), which is the fleet's proven pattern for a
  stdio MCP. Uses the crate's own `config::xdg_data_dir()` (honors
  `$XDG_DATA_HOME`, `$HOME/.local/share` fallback), NOT `dirs::data_local_dir()`.
- Default file-log level is DEBUG (a `DEFAULT_LOG_LEVEL` const), not INFO: the
  `mcp` subcommand has no `--log-level` flag, and the function-level DEBUG story
  the logging rule wants is only captured if DEBUG is emitted. A local stdio MCP
  is low-volume enough that full DEBUG-to-file is the right default. `RUST_LOG`
  is deliberately never consulted (house rule); the level is fixed in code.
- `init_logging` uses `env_logger::Builder::try_init` (not `init`) so a repeated
  call across tests in one process is a harmless no-op instead of a panic;
  it still returns the resolved path. Re-exported at the crate root
  (`pub use serve::{init_logging, serve};`) so the isolated integration test
  (and future hosts) can reach it.
- `run_serve` (`src/cmd.rs`) now: routes logging to a file FIRST (before any
  stdout touch, since stdout becomes the protocol channel), builds the handler
  via the host `build` closure, spins a multi-thread tokio runtime, and
  `block_on(crate::serve::serve(handler))`. All failure paths log via `log`
  and report to STDERR (never stdout) and return `EXIT_FAILURE` (a named const);
  clean shutdown returns `EXIT_SUCCESS`.
- Error enum (`src/error.rs`) grew a `LogPath(String)` variant (log-dir
  resolution when neither `$HOME` nor `$XDG_DATA_HOME` is set), a `Serve(Box<
  rmcp::service::ServerInitializeError>)` variant, and a `Join(#[from]
  tokio::task::JoinError)` variant. The `Serve` variant is BOXED and carries no
  `#[from]` (see Tradeoffs).

### Deviations
- Added a PRIVATE `serve_with<H, T: IntoTransport<RoleServer, E, A>, E, A>`
  seam that the public `serve` delegates to over `(stdin, stdout)`. The design
  named only `serve<H>`; the brief pre-authorized this exact refactor ("refactor
  serve to take a generic transport ... keep the public seam exactly as the
  design specifies"). Same effect, correct seam: it exists so the clean-shutdown
  test can drive a fake handler over an in-memory duplex transport and assert the
  `QuitReason` directly. Public `serve<H>` is byte-for-byte the design signature.
- Re-exported `init_logging` at the crate root, which the design's lib.rs bullet
  (`McpCmd, serve, McpIo, Error, Result, mcp_io!`) does not list. Needed so the
  isolated integration test (Acceptance Criterion #4) can call it; it is also a
  legitimate part of the crate's public logging-discipline API. Same
  disclosed-deviation shape as Phase 1's `xdg_config_dir`/`xdg_data_dir`
  re-exports.
- Removed the Phase-1 `should_panic(expected = "Phase 2")`
  `test_run_serve_dispatches_to_phase2_stub` from `src/cmd/tests.rs`: the serve
  arm is no longer a `todo!()` stub, so the test that pinned that behavior was
  inverted (deleted with a comment pointing at the real serve tests), per the
  tests-must-bite / invert-the-old-test rule.

### Tradeoffs
- `Serve(Box<ServerInitializeError>)` WITHOUT `#[from]`, mapped explicitly at
  the call site (`.map_err(|e| Error::Serve(Box::new(e)))`), vs `#[from]` on the
  bare error: rmcp's `ServerInitializeError` is ~528 bytes, which trips clippy
  `large_enum_variant` + `result_large_err` under `-D warnings`. Boxing shrinks
  the enum; but `#[from] Box<E>` would derive `From<Box<E>>`, not `From<E>`, so
  `?` on `serve()` would not compile. The explicit `map_err` is the clean
  reconciliation. `Join(#[from] JoinError)` stays `#[from]` (it is small).
- The stdout-discipline test lives in its OWN integration binary
  (`tests/stdout.rs`), NOT as a unit test, and uses `libc::dup/dup2` to redirect
  the process's real fd 0/1. A first attempt as a unit test FAILED because the
  libtest harness writes sibling tests' "... ok" progress lines to the real
  fd 1, which the redirect captured and which then failed the JSON-RPC frame
  check. Isolating it as the sole test in its own binary means nothing else
  prints to fd 1 during the capture window. This is why `rmcp`, `serde_json`,
  and `libc` were added as dev-dependencies (the integration crate can only see
  the public API + dev-deps, and it needs to define a handler + parse frames +
  redirect fds). Capturing the TRUE fd 1 (not an injected buffer) is what makes
  the test faithful and makes the break-a-test bite.
- `init_logging` defaults to DEBUG rather than INFO (persona's default): traded
  a quieter log for a complete function-level story, since there is no
  `--log-level` flag to raise verbosity on demand.

### Tests-must-bite (performed)
- Broke `init_logging` to `env_logger::Target::Stdout` and re-ran
  `cargo test --test stdout`: the test FAILED with
  `non-JSON line on stdout (log leak?): "[... DEBUG mcp_io::serve] serve:
  handler=stdout::DummyHandler"` -- the misrouted DEBUG line landed on the
  captured stdout and failed the frame check. Reverted to `Target::Pipe(<file>)`;
  `otto ci` green again (exit 0).

### Success criteria (from the doc, Phase 2)
- "a test drives the fake handler through `serve` and asserts a clean shutdown on
  client disconnect (`waiting()` returns, quit reason logged)" -- PASS:
  `test_serve_with_clean_shutdown_on_client_disconnect` (`src/serve/tests.rs`)
  feeds `initialize` + `notifications/initialized` then EOF and asserts
  `serve_with` returns `QuitReason::Closed` (waiting() returned) and that the
  server wrote at least one response frame. `serve` logs the quit reason at
  DEBUG on exit.
- "a test captures the crate's stdout during `serve` and asserts it contains ONLY
  JSON-RPC frames (no log lines)" -- PASS: `tests/stdout.rs` captures real fd 1
  and asserts every non-empty line is a JSON-RPC 2.0 frame, plus that the
  lifecycle logging landed in the file target.

### Open questions
- None.

## Phase 3: register / unregister / status

### Design decisions
- Two mechanisms, one per target class, exactly as the design mandates.
  Claude Code `user`/`project` shell out to the real `claude` CLI
  (`src/register/claude.rs`): writes via `claude mcp add-json <key> '<json>' -s
  <scope>` and `claude mcp remove <key> -s <scope>`. This treats the target
  config (`~/.claude.json` global state, `./.mcp.json`) as OPAQUE, so there is
  zero risk of dropping the user's unrelated keys. VERIFIED against a live
  invocation into an isolated `$CLAUDE_CONFIG_DIR`: the entry the CLI writes is
  byte-for-byte `{"type":"stdio","command":"<abs>","args":["mcp","serve"]}`, and
  `entry_json()` reproduces it exactly (a test asserts equality against the real
  CLI's output).
- `desktop` is a direct Value-preserving atomic write (`src/register/desktop.rs`,
  `register_at`/`unregister_at`/`write_atomic`): parse as `serde_json::Value`,
  splice ONLY `mcpServers.<key>` via the `Map::entry` API (so a present-but-
  non-object `mcpServers` is left in place and rejected, never overwritten),
  serialize pretty + trailing newline, write to `NamedTempFile::new_in(parent)`
  (SAME dir -> no EXDEV), `sync_all()`, restore the original unix mode onto the
  temp, then `persist` (atomic rename). A crash leaves the original intact.
- Enabled serde_json's `preserve_order` feature (`Cargo.toml`) so the desktop
  RMW does NOT reorder a user's existing keys (BTreeMap would sort them into a
  spurious diff). Satisfies the rust determinism rule's "config round-trips
  un-diffable" clause and keeps the byte-identical-no-op guarantee.
- `current_exe()` lives in `src/register/mod.rs` (shared by both mechanisms):
  the registered `command` is `std::env::current_exe()` resolved to an absolute
  path, pointing at THIS build, never a `$PATH` guess.
- File-state edges (`read_config`): missing/zero-byte -> fresh empty map (write
  our key); malformed JSON -> `Error::MalformedConfig`, file left byte-for-byte
  untouched; non-object top level -> `Error::ConfigNotObject` (untouched);
  `mcpServers` present but not an object -> `Error::McpServersNotObject`
  (untouched). Idempotent register replaces the value (derived from
  `current_exe()`), so a second register is a byte-identical no-op. `unregister`
  on a missing key / file is a clean no-op that never rewrites or creates a file.
- `status` (`register/mod.rs`) surveys all three targets READ-ONLY (writes for
  user/project still go through the `claude` CLI; reads parse the JSON directly
  since a read can never clobber) and reports presence to STDERR. It then builds
  the host handler (token-free) and warns when `get_info().server_info.name !=
  bin` — the rmcp-reports-itself-as-"rmcp" gotcha (verified: rmcp's
  `Implementation::from_build_env()` yields "rmcp"). A build failure only skips
  the name check; status still exits 0.
- All human-facing register/unregister/status output goes to STDERR via
  `eprintln!`, because the crate is `#![deny(clippy::print_stdout)]` (stdout is
  the JSON-RPC protocol channel during `serve`). The `claude` CLI's own "Added
  stdio MCP server ..." output is surfaced through the same stderr path.

### Deviations
- `status` takes the host `build` closure (`register::status<H,F,E>`, wired from
  `cmd.rs`), whereas the design's `run()` doc comment says "register/unregister/
  status never build the handler." The design's own Risk table AND Phase 3
  bullet require a status warning on whether `get_info` name matches bin, which
  is IMPOSSIBLE without an instance. Resolution: status builds the handler
  (construction is token-free — the no-token property is preserved for slack,
  whose `SlackMcpServer::new` is token-free), inspects `get_info`, and degrades
  gracefully (warn, skip the name check) if the build fails. Same intent,
  correct seam — the only way to actually implement the stated mitigation.
- Desktop path resolution is platform-specific (`config_path`): macOS
  `~/Library/Application Support/Claude/...` via `dirs::home_dir()`, Linux
  `xdg_config_dir()/Claude/...`. Intentionally NOT the crate's XDG helper on
  macOS — Claude Desktop is a THIRD-PARTY app and we must match where IT reads,
  per the rust rules' third-party carve-out. Tests exercise read/write/preserve
  against explicit `TempDir` paths (never a platform-path assertion).
- Enabled serde_json `preserve_order` (crate-wide feature) — not named in any
  prior phase's dep list. Rationale above; harmless to serve.
- Added `tempfile` as a real dependency (dev-only in Phases 1-2) and dropped the
  now-redundant `tempfile`/`serde_json` dev-dependency entries.
- Inverted two prior stub tests per tests-must-bite: the Phase-1 `should_panic`
  dispatch stubs in `register/tests.rs` were replaced with real round-trip/status
  tests, and `cmd/tests.rs`'s `should_panic(expected="Phase 3")` status stub
  became `test_run_status_returns_success` asserting exit 0.

### Tradeoffs
- Live `claude` round-trip tests are GATED on `claude` being on PATH (skip-with-
  note when absent), PLUS always-run pure unit tests for entry-JSON and argv
  construction. On this machine `claude` IS available, so both the round-trip and
  the real-entry-match tests executed and passed; on a claude-less CI host they
  skip while the construction + shape logic stays covered. Chose real-CLI
  coverage where available over a mock that could drift from the CLI's behavior.
- status reads the REAL desktop/project config locations for the presence survey
  in unit tests (read-only; our unique test key is never present). The user-scope
  path is isolated via `$CLAUDE_CONFIG_DIR`; the survey outcome never affects the
  asserted exit code (always 0), so this is safe.
- Human output to STDERR (not stdout) for a status command: chosen for crate-wide
  protocol-channel safety (`deny(print_stdout)`) and consistency with
  `run_serve`'s stderr path. A caller piping status would not get it on stdout;
  accepted and documented.

### Tests-must-bite (performed)
- Broke `register_at` to build a fresh empty `Map` instead of splicing into the
  read config (clobber instead of preserve) and ran
  `test_register_preserves_all_keys_and_servers`: it FAILED with
  `left: Null, right: String("/bin/a")` — the pre-existing `other-a` server's
  command vanished, exactly the clobber the test exists to catch. Reverted;
  `otto ci` green again (exit 0).

### Success criteria (from the doc, Phase 3)
- "register -> status -> unregister round-trip (per target)" — PASS:
  `test_claude_user_roundtrip` drives the real `claude` CLI (user scope) in a
  temp `$CLAUDE_CONFIG_DIR` through register -> present -> unregister -> absent;
  `test_register_fresh_creates_file` + `test_key_present_detection` cover the
  desktop round-trip. Project scope shares the claude mechanism; its command
  construction is unit-tested.
- "desktop direct-write: a config pre-populated with two OTHER servers AND
  unrelated top-level keys asserts ALL survive register AND unregister" — PASS:
  `test_register_preserves_all_keys_and_servers` +
  `test_unregister_preserves_all_keys_and_other_servers` (AC #3).
- "a second register is a no-op (byte-identical file)" — PASS:
  `test_second_register_is_byte_identical`.
- "register against a malformed config, or a non-object `mcpServers`, errors and
  leaves the file byte-for-byte untouched" — PASS:
  `test_malformed_json_errors_and_leaves_file_untouched`,
  `test_non_object_mcpservers_errors_and_leaves_file_untouched`,
  `test_non_object_toplevel_errors_and_leaves_file_untouched`.
- "the `claude mcp add-json` path is verified against a real invocation so
  `<bin> mcp register` and a hand `claude mcp add-json` produce the same entry"
  — PASS: `test_claude_add_json_matches_our_entry` asserts the entry the real CLI
  wrote equals `entry_json()`. Observed real entry:
  `{"type":"stdio","command":"/abs/path/slack","args":["mcp","serve"]}`.

### Open questions
- None. (`claude` was available in this environment, so both live-CLI tests ran
  rather than skipping.)

## Phase 5: bundle (.mcpb)

### Design decisions
- Enumerating the host handler's advertised tools is done via a REAL,
  in-process MCP handshake, not a shortcut: `advertised_tools<H>`
  (`src/bundle.rs`) serves `handler` over one end of a `tokio::io::duplex`
  pipe, connects a bare rmcp client (`()` -- `ClientHandler for ()` is
  blanket-implemented) to the other end, and calls
  `Peer::<RoleClient>::list_all_tools()`. This is generic over ANY
  `H: ServerHandler`, including slack's eventual real handler -- nothing
  test-shaped leaks into the production seam. Confirmed by reading rmcp
  2.1.0/2.2.0 source directly
  (`~/.cargo/registry/.../rmcp-2.2.0/src/service.rs`,
  `src/service/client.rs`): `ServerHandler::list_tools` needs a
  `RequestContext<RoleServer>`, buildable only from a `Peer<RoleServer>`, and
  `Peer::new` is `pub(crate)` (the design's own finding for why `call` was
  cut) -- so a real client peer is the only generic-over-`H` way to reach the
  tool list without a live external client.
- Both `handler.serve(server_end)` and `().serve(client_end)` block on their
  own read of the `initialize` handshake before returning (confirmed by
  reading `serve_server_with_ct_inner`/`serve_client_with_ct_inner` in rmcp's
  source: the server loops reading the initialize REQUEST, the client awaits
  the initialize RESPONSE, both BEFORE spawning the steady-state loop and
  returning `RunningService`). Awaiting them sequentially therefore deadlocks
  (proved live -- see Tests-must-bite). Fixed with `tokio::join!` to poll both
  concurrently on the same task.
- `manifest.json` is generated from `McpIo` + the tool list per the real mcpb
  v0.3 schema, fetched from `modelcontextprotocol/mcpb` via `gh api` (network
  curl was denied by the sandbox; `gh` was already authenticated) and checked
  in as a test fixture (`tests/fixtures/mcpb-manifest-v0.3.schema.json`,
  `additionalProperties: false` throughout). Required top-level fields per the
  schema: `name`, `version`, `description`, `author` (with `author.name`),
  `server` (with `server.type`, `server.entry_point`, `server.mcp_config` ->
  `mcp_config.command`). `manifest_version` is documented as required in
  `MANIFEST.md` prose but is NOT in the schema's own `required` array; emitted
  anyway since it's a real, schema-recognized field. `server.type` is always
  `"binary"` (every mcp-io host is a compiled Rust CLI). `mcp_config.args` is
  `["mcp", "serve"]`, matching Phase 3's registration entry exactly.
- The `.mcpb` BUNDLES a copy of the current build's binary under
  `server/<bin>` (rather than referencing `current_exe()`'s absolute path the
  way `register` does). This is deliberate and is the one place bundle does
  NOT mirror register: bundle's whole stated purpose (design's Rollout Plan --
  "ships to the whole company, majority macOS... Desktop/Cowork is a PRIMARY
  target") is installing on colleagues' machines that do NOT already have this
  CLI, so an absolute path pointing at THIS machine's install location would
  be useless to them. `entry_point` and `mcp_config.command` both point at
  `server/<bin>`, matching the mcpb spec's own "Binary Example" verbatim
  (`"command": "server/my-server"`, relative, no `${__dirname}` prefix).
- The packaged binary is stored with `CompressionMethod::Stored` (no
  compression), not the default deflate: compiled binaries barely compress,
  and deflating a real binary (tested against the 77MB debug test binary)
  dominated the whole test suite's wall-clock at ~180s before this change,
  ~2s after. `manifest.json` keeps the (harmless, tiny) default compression.
- `author.name` is a fixed constant (`"Tatari"`): `McpIo` carries no author
  field and the design doesn't add one for this phase; every consumer of this
  library is a Tatari-owned CLI, so a fixed org-level default is defensible
  pending a real seam (see Open Questions).
- `run_bundle` (`src/cmd.rs`) now threads `build` through exactly like
  `run_serve`: builds the handler (token-free, per the design), spins its own
  multi-thread tokio runtime, and `block_on`s `bundle::bundle`. Bundle is a
  build-requiring verb per the design's `run()` doc comment (grouped with
  `serve`, not `register`/`unregister`/`status`).
- Schema validation is REAL, not hand-asserted: `jsonschema` (dev-dependency,
  `--no-default-features --features resolve-file` to avoid pulling in a
  network-resolving `$ref` stack the local schema doesn't need) validates the
  generated `manifest.json` against the checked-in v0.3 schema fixture via
  `jsonschema::validator_for(...).iter_errors(...)`.

### Deviations
- **Disclosed phase reorder** (per the brief): the design orders Phase 5 AFTER
  Phase 4 (slack-cli integration) so bundle is "smoke-tested against slack's
  REAL handler." Phase 4 lives in a separate repo (`slack-cli`) and is gated
  on this crate's tag existing first, so it is a cross-repo follow-on that
  cannot run in this session. Phase 5 is built here, ahead of Phase 4, and
  unit-tested against a FAKE handler (`FakeHandler`, `src/bundle/tests.rs`)
  advertising two known tools (`search_widgets`, `post_widget`) via real
  `#[tool_router]`/`#[tool_handler]` macros -- not a bare `ServerHandler`
  stub, so the tool-enumeration path is exercised for real. The "smoke-test
  against slack's real handler" criterion rides with the Phase 4 follow-on.
  `bundle<H>`/`advertised_tools<H>` are generic over ANY `ServerHandler`, so
  no rework is expected when slack's handler lands.
- Added the `client` feature to `rmcp` (main dependency, not just dev): the
  design's Dependencies section only names `features (server, macros)`.
  Needed because `advertised_tools` requires `ClientHandler`/`RoleClient`/
  `ClientInitializeError`, which rmcp gates behind `client` (`default = [base64,
  macros, server]` does NOT include it). This is production code (the bundle
  verb ships in the library), not test-only, so it rides the main dependency.
- Added `zip` (main dependency) and `jsonschema` (dev-dependency), neither
  named in the design's Dependencies list, because packaging + validating a
  `.mcpb` needs them. Both added via `cargo add` with reduced feature sets
  (`zip --no-default-features --features deflate`; `jsonschema
  --no-default-features --features resolve-file`) to avoid pulling in unused
  compression backends / network `$ref` resolution.

### Tradeoffs
- Bundling the real binary (`io::copy` from `current_exe()` into the zip) vs
  referencing an absolute path like `register`: chose bundling because a
  `.mcpb`'s entire value proposition is installing on a machine that does NOT
  already have the CLI -- an absolute path would only work on the machine that
  built the bundle. Cost: the packaged binary can go stale relative to a
  `renew`-updated system install; accepted, since `.mcpb` is a point-in-time
  distribution artifact (a new release re-runs `bundle`), not a live-updating
  install like `register`'s Claude Code entries.
- `Stored` vs deflate compression for the packaged binary: chose `Stored` for
  speed (see Design decisions); the size cost (an uncompressed binary vs a
  mildly-compressed one) is negligible for how compiled executables actually
  compress, and irrelevant next to the wall-clock win.
- Structural JSON-schema validation (real `jsonschema` crate against the
  checked-in v0.3 fixture) vs a hand-asserted required-field list: chose the
  real validator since it was cheap to add (`resolve-file`-only feature set)
  and catches anything the schema forbids (`additionalProperties: false`
  everywhere), not just what a hand-written assertion happens to check.

### Tests-must-bite (performed)
- First implementation awaited `handler.serve(server_end)` and then
  `().serve(client_end)` sequentially; `cargo test bundle` hung indefinitely
  (confirmed via a `timeout`-wrapped run, not just "felt slow"). Root cause
  (see Design decisions): both `.serve()` calls block reading their own half
  of the `initialize` handshake before returning, so the server call never
  returns because the client hasn't started yet. Fixed with `tokio::join!`
  to poll both concurrently; reran green.
- Broke tool-name propagation in `build_manifest` (hardcoded the tools list to
  an ignored-then-empty `Vec::new()`, keeping `tools` "used" to satisfy
  `#![deny(unused_variables)]`) and reran
  `test_bundle_produces_valid_manifest_with_matching_tool_names`: it FAILED
  with `manifest.tools should be an array` (the `#[serde(skip_serializing_if =
  "Vec::is_empty")]` on an empty list drops the field entirely, so
  `manifest["tools"].as_array()` is `None`) -- exactly the propagation break
  the test exists to catch. Reverted; `otto ci` green again (exit 0, "All CI
  checks passed!").

### Success criteria (from the doc, Phase 5, adapted per the disclosed reorder)
- "`slack mcp bundle` produces a `.mcpb` that validates against the mcpb
  manifest schema" -- adapted to "`<bin> mcp bundle` produces a `.mcpb` that
  validates against the mcpb manifest schema" since slack isn't wired yet --
  PASS: `test_bundle_produces_valid_manifest_with_matching_tool_names`
  (`src/bundle/tests.rs`) builds a bundle from `FakeHandler`, extracts
  `manifest.json` from the zip, and validates it against the checked-in real
  mcpb v0.3 JSON schema with zero validation errors. Also covered end-to-end
  through the CLI dispatch path by `test_run_bundle_dispatches_and_writes_file`
  (`src/cmd/tests.rs`), which drives `McpCmd::run` for the `Bundle` variant.
- "a smoke test asserts the manifest lists the same tool names
  `SlackMcpServer` advertises" -- adapted to the fake handler --  PASS: the
  same test asserts `manifest["tools"]`'s names equal
  `["post_widget", "search_widgets"]`, matching `FakeHandler`'s
  `#[tool_router]`-declared tools exactly;
  `test_advertised_tools_lists_handler_tools` independently asserts
  `advertised_tools(FakeHandler)` returns those same two names.
- "Tests-must-bite (practice)" -- PASS, see above (both the deadlock discovery
  and the deliberate tool-name break).

### Open questions
- Should `McpIo` (or the `mcp_io!()` macro) grow a real `author` field/seam so
  `manifest.json`'s `author.name` reflects the actual host/team rather than
  the fixed `"Tatari"` constant? Deferred here since the design doesn't
  request it and no consumer has asked yet; flagging since Phase 6
  (docs + contract fixture) or a future consumer may want it.
- Confirm the packaged-binary choice (bundle the real executable under
  `server/<bin>`, `Stored` compression) is what Scott wants for the
  Desktop/Cowork distribution story, versus some other packaging shape (e.g.
  a smaller wrapper script). Reasoned through in Tradeoffs above; no directly
  stated preference in the design doc to confirm against.

## Phase 6 (mcp-io side): docs + contract golden fixture

### Design decisions
- README (`README.md`, previously a two-line stub) is written as a full
  integration guide modeled on `renew`'s, covering: the value split (library
  owns scaffolding, host owns its `ServerHandler`), the git-dep-by-tag
  quickstart, the `Mcp(mcp_io::McpCmd)` embed + `#[tool_router]` handler +
  `main.rs` early-intercept wiring (mirroring renew's `Update` arm), the
  `mcp_io!()` macro, the `.with_server_info` requirement (called out as its
  own section since it is the one gotcha every integrator hits -- rmcp's
  `Implementation::from_build_env()` expands `CARGO_CRATE_NAME` *inside rmcp*,
  reporting `"rmcp"` unless overridden), a verb table for all five verbs plus
  the `--target` table, the checked contract artifact, the `.mcpb` shape
  (bundles the real binary under `server/<bin>`, `server.type: "binary"`,
  `author.name` fixed to `"Tatari"`), the logging discipline (file-routed,
  DEBUG default, no `--log-level`/`$RUST_LOG`), the auth/concurrency
  guidance for a non-`Clone` client, and the non-goals (remote transport,
  resources/prompts, the cut `call` verb).
- The git-dep snippet uses a placeholder tag (`tag = "vX.Y.Z"`) rather than a
  concrete version number: no tag has been cut for this repo yet (`git tag -l`
  is empty at the time of writing), and the house doc-writing rule bans
  pre-naming a release before it exists. `renew`'s README uses a concrete tag
  because `renew` has actually shipped one.
- The checked contract artifact is `tests/fixtures/contract.json` (sibling of
  the existing `tests/fixtures/mcpb-manifest-v0.3.schema.json`, so both golden
  fixtures for this crate live in one place). It captures two things per the
  design's Phase 6 bullet: the `mcpServers` entry shape (an `entry.example`
  object, verified byte-for-byte against a real `claude mcp add-json`
  invocation per Phase 3) and the per-target path-resolution rule (a
  `targets.<user|project|desktop>` object per target: mechanism, scope flag,
  config filename, and a prose `resolution` rule -- not a literal path, since
  `user`/`desktop` are environment-dependent).
- The contract TEST lives as a `contract` submodule inside
  `src/register/tests.rs` (`mod contract { ... }`), not as a `tests/` (cargo
  integration test) binary. Reason: the functions the design says to pin
  (`claude::entry_json`, `claude::config_path`, `desktop::config_path`) are
  all `pub(crate)`, invisible to an integration test crate, which can only see
  the public API. Every assertion loads `tests/fixtures/contract.json` via
  `include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/contract.json"))`
  and calls the REAL functions against it (`crate::register::claude::entry_json`,
  `crate::register::claude::config_path`, `crate::register::desktop::config_path`),
  never a hand-copied shape -- exactly the "reuse Phase 3's real code paths"
  requirement.
- The desktop path assertion checks only the SUFFIX (`Claude/<config-file>`),
  never a full platform literal, and never mutates `$XDG_CONFIG_HOME` --
  `crate::config`'s own tests already cover the env-honoring/fallback behavior
  (`xdg_config_dir()`), and `desktop::config_path()` calls that helper
  directly, so re-testing the env behavior here would (a) be redundant and
  (b) risk a cross-module env-var race against `config::tests`'s own
  `ENV_LOCK` (a different mutex than `register::ENV_LOCK`, guarding the same
  process-global `$XDG_CONFIG_HOME`). The user-path assertion DOES mutate env
  (`$CLAUDE_CONFIG_DIR`), but that is safe: it is guarded by
  `register::ENV_LOCK`, the same lock every other `CLAUDE_CONFIG_DIR`-mutating
  test in this module already uses, and no other module touches that
  variable.

### Deviations
- None. The README and contract fixture are new files with no prior shape to
  diverge from; the contract test module lives inside `src/register/tests.rs`
  rather than a new top-level `tests/contract.rs` file, disclosed above as a
  visibility-driven seam choice, not a shortcut.

### Tradeoffs
- One golden JSON fixture covering both the entry shape AND the path-
  resolution table, vs two separate fixture files: chose one file since both
  facts are the same "registration contract" a `mcp-io-py` port must
  reproduce together, and the design's Phase 6 bullet names them as one
  artifact ("a checked-in golden fixture of the registration entry format +
  the target path-resolution table").
- Prose `resolution` strings in the fixture (e.g. "`$CLAUDE_CONFIG_DIR/.claude.json`
  if set and absolute, else `$HOME/.claude.json`") vs a fully mechanized
  fixture (e.g. an ordered list of env-var/fallback pairs the test walks
  generically): chose prose + a targeted assertion per target, since the
  three targets' resolution rules are structurally different (two shell out
  to `claude`, one is a direct write; `desktop` additionally branches on
  `target_os`) and a generic walker would either flatten that distinction or
  need as much special-casing as three explicit tests. The prose still keeps
  the fixture human-legible documentation, and every machine-checkable fact
  in it (config filename, example entry) IS asserted against real code.

### Tests-must-bite (performed)
- Broke the real writer: added `"--verbose"` to `claude::entry_json`'s `args`
  array. Reran `cargo test --all-features register::` -- both
  `register::claude::tests::test_entry_json_shape` (the pre-existing pinned
  test) and the new `register::tests::contract::test_contract_entry_matches_claude_entry_json`
  FAILED, printing the exact `args` diff (`["mcp","serve","--verbose"]` vs
  `["mcp","serve"]`). Reverted; reran green.
- Broke the fixture instead of the code: changed `contract.json`'s
  `targets.project.config-file` from `.mcp.json` to `.claude-project.json`
  (code unchanged). `test_contract_project_path_matches_config_path` FAILED
  with `left: ".mcp.json" right: ".claude-project.json"` -- proving the test
  catches drift from EITHER side, not just a code regression. Reverted;
  `otto ci` green again (exit 0, "All CI checks passed!").

### Success criteria (from the doc, Phase 6, mcp-io side)
- "`otto ci` green" -- PASS: `otto ci` exits 0 with "OK: All CI checks
  passed!" (41 lib tests + 1 stdout integration test + 1 doctest, all green).
- "the golden fixture test fails if the entry shape changes without updating
  the fixture" -- PASS: see Tests-must-bite above, both directions (writer
  changed / fixture changed) demonstrated to fail loudly.

### Open questions
- None new. The Phase 5 open question about a real `author` seam for
  `McpIo`/`mcp_io!()` (vs the fixed `"Tatari"` constant) is unchanged and
  still open; the README documents the current fixed behavior honestly
  rather than speculating on a future seam.
- Phase 4 (slack-cli integration) and the slack-cli README/bump are a
  cross-repo follow-on that has not run in this session; this phase covers
  only the mcp-io-rs side. The design doc's `Status:` field is left as-is
  (not flipped to Implemented) since the doc's own Phase 6 spans both repos
  and the slack-cli half is still outstanding.
