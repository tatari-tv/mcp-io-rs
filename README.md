# mcp-io

Shared scaffolding library that gives any Tatari CLI a local stdio MCP server
via one `mcp` subcommand: `serve`, `register`, `unregister`, `status`,
`bundle`. Sibling of `renew` and `okta-auth-rs` -- same shape (git dep by tag,
one clap type embedded in the host's `Command` enum), different job.

## The value split

Four in-house MCP servers exist today, all wired up differently: different
rmcp versions, different registration steps (some manual, some none), no
`.mcpb` bundle anywhere. Every new one re-derives the same scaffolding by
hand.

- `renew` owns its whole feature because "update" is generic across every CLI.
- MCP **tools** are host-specific (Slack's `search_messages`, oracle's
  `knowledge_search`). This library cannot own them, and does not try to.
- So `mcp-io` owns the scaffolding -- the `mcp` subcommand surface, the stdio
  lifecycle, the logging discipline, self-registration into Claude config, and
  the `.mcpb` bundle -- and the host owns its `ServerHandler` (its tools). The
  seam is a generic `serve<H: ServerHandler>` plus a clap type the host embeds.

The library never sets the server's identity (name/instructions/capabilities);
that is entirely the host's `ServerHandler::get_info`. See
[The `.with_server_info` requirement](#the-with_server_info-requirement-read-this)
below -- skipping it is the one gotcha every integrator hits.

## Quickstart

Add the dep, pinned to a tag (see the design doc's Rollout Plan for the tag
`mcp-io-rs` ships first):

```toml
[dependencies]
mcp-io = { git = "https://github.com/tatari-tv/mcp-io-rs", tag = "vX.Y.Z" }
tokio = { version = "1", features = ["rt-multi-thread"] }
```

Consuming a private repo needs the same `CARGO_NET_GIT_FETCH_WITH_CLI` +
`insteadOf` recipe already used for `okta-auth-rs`.

Embed the library's clap type as one variant of your `Command` enum, write your
own `#[tool_router]` handler, and dispatch:

```rust
use clap::Subcommand;

#[derive(Subcommand)]
enum Command {
    // ...your existing commands...
    Mcp(mcp_io::McpCmd),
}

// Your tools, wired to your API. The handler OWNS a cloned `Config` (`Config: Clone`)
// so it can satisfy `H: Send + 'static` without borrowing anything from `main`.
#[derive(Clone)]
struct SlackMcpServer {
    config: Config,
}

#[rmcp::tool_router]
impl SlackMcpServer {
    #[rmcp::tool(description = "Search Slack messages.")]
    async fn search_messages(&self, /* params */) -> Result<CallToolResult, McpError> {
        // rebuild the token source + client per call inside spawn_blocking;
        // see "Auth and concurrency" below
    }
}

#[rmcp::tool_handler]
impl rmcp::ServerHandler for SlackMcpServer {
    fn get_info(&self) -> rmcp::model::ServerInfo {
        rmcp::model::ServerInfo::default()
            // REQUIRED -- see below.
            .with_server_info(rmcp::model::Implementation::new("slack", env!("CARGO_PKG_VERSION")))
    }
}
```

In `main.rs`, intercept the `Mcp` arm early, exactly like `renew`'s `Update` arm:

```rust
if let Command::Mcp(cmd) = &cli.command {
    let io = mcp_io::mcp_io!(key = "slack");
    // `build` is called ONLY for serve/bundle, and is token-free here: the handler
    // owns a cloned Config, no login needed to register/bundle/status.
    std::process::exit(cmd.run(&io, || Ok(SlackMcpServer { config: config.clone() })));
}
```

`mcp_io!()` mirrors `renew!()`: it captures the HOST's `CARGO_PKG_NAME` /
`CARGO_PKG_VERSION` at the host's own compile time (macro expansion happens in
the host's crate), so `bin`/`version` are always the host's, never this
crate's.

```rust
let io = mcp_io::mcp_io!();              // server_key = bin = CARGO_PKG_NAME
let io = mcp_io::mcp_io!(key = "slack");  // override the registration key / bin name
```

That is the full integration. No tokio runtime is built unless the `Mcp` arm
actually runs -- a sync-only host (no async surface yet) stays sync for every
other command.

## The `.with_server_info` requirement (read this)

rmcp's `Implementation::from_build_env()` expands `env!("CARGO_CRATE_NAME")`
**inside rmcp itself**, so a handler that never overrides `get_info` reports
its server name to the client as `"rmcp"`, not your bin name. `mcp-io` cannot
fix this for you -- `get_info` is entirely the host's -- so every host MUST
call `.with_server_info(Implementation::new("<bin>", ...))` explicitly, as
shown above.

`mcp status` catches the mistake if you forget: it builds the handler
(token-free) and compares `get_info().server_info.name` against `io.bin`,
warning on stderr if they differ.

## The five verbs

| Verb | Builds the handler? | What it does |
|---|---|---|
| `mcp serve` | yes | Serves the host's tools over stdio until the client disconnects. |
| `mcp register --target <t>` | no | Writes this build's entry into a Claude config target. |
| `mcp unregister --target <t>` | no | Removes this build's entry from a Claude config target. |
| `mcp status` | yes (token-free) | Reports which targets carry the entry, and warns on a `get_info` name mismatch. |
| `mcp bundle [--out <path>]` | yes | Packages a `.mcpb` for Claude Desktop / Cowork. |

`register`/`unregister`/`status` never build the handler for a login/token --
`status` builds it only far enough to read `get_info()`, so no host needs to
be authenticated to register, unregister, or check status.

### `--target` options

`register`/`unregister` take `--target user|project|desktop` (default
`user`), case-insensitive:

| target | mechanism | resolves to |
|---|---|---|
| `user` | delegates to `claude mcp add-json ... -s user` | Claude Code user scope: `$CLAUDE_CONFIG_DIR/.claude.json` if set and absolute, else `$HOME/.claude.json` |
| `project` | delegates to `claude mcp add-json ... -s project` | Claude Code project scope: `./.mcp.json` |
| `desktop` | direct, Value-preserving atomic write (no `claude` CLI in a Desktop-only install) | macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`; Linux (community build): `$XDG_CONFIG_HOME/Claude/claude_desktop_config.json` if set and absolute, else `~/.config/Claude/claude_desktop_config.json` |

Claude Code targets treat the config as opaque (shelling out to the `claude`
CLI, never parsing `~/.claude.json` for a write), so there is zero risk of
dropping the user's unrelated global state. The `desktop` target parses as
`serde_json::Value`, splices only `mcpServers.<key>`, and writes atomically
(`tempfile_in` the same directory, fsync, rename) -- every other top-level key
and every other registered server survives register and unregister. A missing
`claude` binary on a Claude Code target fails loud with the exact command to
run by hand; a malformed config or a non-object `mcpServers` on the desktop
path errors and leaves the file byte-for-byte untouched, rather than
overwriting it.

The registered `command` is always the resolved absolute path to *this build*
(`std::env::current_exe()`), never a `$PATH` guess, and `args` is always
`["mcp", "serve"]`. This exact shape is the checked contract artifact -- see
below.

## The checked contract artifact

`tests/fixtures/contract.json` is the golden fixture pinning:

- the `mcpServers` entry shape mcp-io writes for every target (verified
  byte-for-byte against a real `claude mcp add-json` invocation), and
- the per-target path-resolution rule from the table above.

`src/register/tests.rs`'s `contract` test module asserts the real writer
(`claude::entry_json`) and the real path resolvers (`claude::config_path`,
`desktop::config_path`) against this fixture, not a hand-copied literal.
Changing the entry shape or a path rule in code without updating the fixture
(or vice versa) fails CI. This exists because the matched-pair
`okta-auth-rs`/`okta-auth-py` shared their contract as prose only, and the two
drifted (cache path diverged, features fell behind). A future `mcp-io-py` port
(or any other rewrite) must reproduce `contract.json` exactly.

## `.mcpb` bundle

`mcp bundle` enumerates the host handler's advertised tools via a real,
in-process MCP handshake (not a guess), generates a `manifest.json` per the
`modelcontextprotocol/mcpb` v0.3 schema, and zips it with a copy of *this
build's binary* under `server/<bin>` -- not a reference to an absolute path
the way `register` does, since a `.mcpb` ships to a colleague's machine that
does not already have the CLI installed.

What ships in the bundle:

- `manifest.json`: `manifest_version: "0.3"`, `name`/`version` from the host's
  `McpIo`, `author.name` (currently a fixed `"Tatari"` constant -- there is no
  per-host author seam yet; see the design doc's Phase 5 open question if you
  need one), `server.type: "binary"`, `server.entry_point` and
  `server.mcp_config.command` both `server/<bin>`, `server.mcp_config.args:
  ["mcp", "serve"]` (matching the registration entry), and `tools: [...]`
  listing every tool name/description the handler advertises.
- `server/<bin>`: the running binary, stored uncompressed (`Stored`, not
  deflate -- compiled binaries barely compress, and deflating one dominated
  the test suite's wall-clock before this was fixed).

## Logging discipline

Once `serve` is running, stdout **is** the JSON-RPC protocol channel -- nothing
may write to it besides protocol frames. `mcp-io` is
`#![deny(clippy::print_stdout)]` crate-wide; all logging goes through the
`log` facade, routed to `<xdg-data>/<bin>/logs/<bin>.log` (honoring
`$XDG_DATA_HOME`, falling back to `$HOME/.local/share`) before `serve` ever
touches stdio. The default level is DEBUG (there is no `--log-level` flag on
`mcp`, and a local stdio MCP is low-volume enough that full DEBUG-to-file is
the right default); `$RUST_LOG` is never consulted. `register`/`unregister`/
`status` write their human-facing output to stderr, never stdout, for the same
reason.

## Auth and concurrency

`mcp-io` never handles a token. The host owns its own auth entirely; a
missing/expired token should surface as a recoverable per-call tool error
(never a protocol error), and a host that wants a fail-fast "run `<bin> login`"
hint probes for it itself, before calling `cmd.run(...)` -- the library has no
probe method and never builds the runtime speculatively.

If your host's client isn't `Clone` and can't be shared across calls (e.g. it
wraps a blocking HTTP client), rebuild it per tool call inside
`tokio::task::spawn_blocking`, under a lock scope that is released before the
blocking call -- never hold a lock across `.await`, and never call a blocking
client directly inside an async tool handler (it will panic on the runtime).

## Non-goals

- Remote transports (streamable-http/SSE) -- stdio local only. `marquee` and
  `persona-mcp` keep their own remote surfaces.
- MCP resources or prompts -- tools-only, matching the whole fleet.
- A `call` verb -- cut entirely; every consumer already has an equivalent CLI
  invocation, so a generic transport-less `call` would be redundant (and
  rmcp 2.1's `Peer::new` being `pub(crate)` makes it impossible to build one
  without a real client peer anyway).

See `docs/design/2026-07-09-mcp-io-rs.md` for the full design, and
`docs/design/2026-07-09-mcp-io-rs-implementation-notes.md` for the
per-phase implementation record.

## License

MIT (per `Cargo.toml`).
