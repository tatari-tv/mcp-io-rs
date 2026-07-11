# Implementation Notes: mcp-io fleet port (persona-cli, clyde, marquee)

Running record of how the implementation diverged from or interpreted the design doc
(`2026-07-10-mcp-io-fleet-port.md`). Append-only. One section per phase; each covers
Design decisions / Deviations / Tradeoffs / Open questions.

Cross-repo note: the three phases shipped as independent PRs in three separate repos, so
these notes are collected here (next to the design doc in `mcp-io-rs`) rather than in each
consumer repo. Shipped: persona-cli v1.8.0 (#45), clyde v0.9.0 (#40), marquee v1.14.0 (#64).

## Phase 1: persona-cli

### Design decisions
- Kept the 16 tools, the handler, `get_info` with `.with_server_info("persona")`, the typed
  `AuthFailed`/`NoMatch` seams, and the `spawn_blocking` token bridge verbatim per the plan.
- Extended the pre-existing handshake test to assert all 16 tool names + `serverInfo.name`
  (was pinned to `list_orgs` only). Added a `register -> status -> unregister` round-trip
  test and a bundle-manifest test, driven through real `Cli::parse_from` clap parsing
  because mcp-io's inner `McpSub` enum is private to the library.

### Deviations
- Did NOT add a `Commands::name()` arm. The plan's checklist mentioned a "name()/help arm,"
  but persona-cli's `Commands` enum has no such method and `main.rs` never calls one; adding
  it only for `Mcp` would be inconsistent scope creep. Left out.
- Added `#[allow(clippy::result_large_err)]` to the pre-existing `require_single` fn
  (`src/mcp.rs`): resolving mcp-io's transitive deps pulled a newer rmcp whose
  `CallToolResult` grew past clippy's 128-byte `result_large_err` threshold. Pre-existing
  code, not changed by the port, but CI would not go green without it.
- Added `#[allow(clippy::print_stderr)]` scoped to the new test module: the crate-wide
  `#![deny(clippy::print_stdout, clippy::print_stderr)]` is stricter than mcp-io's own, and
  the SKIP-if-`claude`-absent test message needs `eprintln!`.

### Tradeoffs
- None beyond the design's Resolved Decisions (dropped the eager token probe; per-call
  recoverable `AuthFailed` carries the "run persona login" hint).

### Open questions
- None.

## Phase 2: clyde

### Design decisions
- Hand-wrote `impl Default for Config` so `reindex-on-start` defaults to `true` (a derived
  `Default` gives bool `false`, which would diverge from the serde/missing-file default);
  guarded by a test.
- Placed the `Mcp` intercept BEFORE the whole logging ladder in `main.rs` (env_logger and
  mcp-io both claim the global `log` slot).
- Inlined `default_projects_dir()` in `common` (the `common` crate cannot depend on the
  `session` crate); mirrors `session::paths::claude_projects_dir` exactly.

### Deviations
- The plan's `build_server(&cli.db, ...)` snippet: `cli.db` is `Option<PathBuf>`, so resolved
  via `cli.db.clone().unwrap_or_else(session::paths::sessions_db_path)` (same as the old
  `cmd_serve`).
- Removed rmcp + tokio + tracing-subscriber as direct deps of the `clyde` crate (the plan
  named only tracing-subscriber). After the port `clyde/src` references none: the rmcp bump
  lives in `sessions`, and mcp-io owns the runtime. Verified by grep + clean build.

### Tradeoffs
- Needed `cargo update -p env_logger --precise 0.11.11` (mcp-io needs `^0.11.11`; the
  workspace lock pinned 0.11.10; `cargo add` refused the transitive bump).
- rmcp declared `2.1.0`, resolves to `2.2.0` (matches slack-cli). `cargo tree -i rmcp` shows
  exactly one rmcp version - the load-bearing unification constraint held.

### Open questions
- None on code. The one blocker was an org decision, not code: clyde is PUBLIC and could not
  clone the then-private `mcp-io-rs` dep in credential-less CI. RESOLVED by making
  `mcp-io-rs` public (see the design doc's Implementation Addendum).

## Phase 3: marquee

### Design decisions
- `MarqueeMcpServer` (`#[derive(Clone)]`) holds `Arc<Client>` + an `AuthSource`
  (`Dev(String)` | `Okta(Arc<OktaAuth>)`) + a `token_lock`. Each tool resolves `Auth` per
  call inside `spawn_blocking` (the CLI client is `reqwest::blocking`), token acquisition
  serialized under `token_lock`. Mirrors slack/persona.
- Tool inputs REUSE `contract` types: `marquee_publish` takes `Parameters<contract::PublishRequest>`
  directly; `marquee_update` nests `contract::ReplaceRequest`. No forked request schema.
- `marquee_slides` builds the deck in-memory (no temp dir) and publishes via `client.publish`
  with `kind=Html`; 5 MiB input guard mirrors the remote.
- `Mcp` intercept placed above `setup_logging` (marquee inits env_logger early); `mcp_io!(bin = "marquee")`.

### Deviations
- None from the Phase 3 spec. One design-unspecified but required add: `schemars` as a direct
  CLI dep (the rmcp `#[tool]` macro needs it in scope; the plan's checklist anticipated
  "+schemars where absent").

### Tradeoffs
- Typed-401 seam: added `client::ClientError { AuthRejected(String), Server(String) }` (plain
  enum + manual Display/Error, matching slack's `SlackErr` precedent - no thiserror in the
  CLI). Message text is byte-identical, so existing client output/tests are unchanged. The
  handler downcasts it: `AuthRejected`/server error -> recoverable tool error;
  transport/parse faults -> protocol error.
- `--out` on `mcp bundle` is a FILE path, not a directory (mcp-io semantics) - noted for docs/shakedown.

### Open questions
- None. Dev-server verification of `marquee_read` slug/url was PARTIAL (no reachable dev
  server); the unauthenticated recoverable-error path was verified end-to-end, and the read
  payload shape (`slug` present, no `url`) was confirmed against the code - see the design
  doc's `marquee_read` contract correction.
