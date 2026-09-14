# Implementation Notes: Valet Default, MCP Key Ownership, Update Reporting

Design doc: `tatari-tv/slack-cli` `docs/design/2026-09-14-valet-default-mcp-key-update-reporting.md`
(defect D2, workstream W2, is the part that lands in this repo).

## Phase 1 (W2): Ownership check, fail closed

### Design decisions
- `Ownership` is an enum in `src/register/mod.rs`, not a `bool` — `register` has three
  distinct outcomes (proceed silently, proceed after a remove, refuse) and a bool
  cannot carry the foreign entry's `command` for the refusal text.
- `Foreign { command: Option<String> }` rather than the doc's `Foreign { command: String }`
  — `src/register/mod.rs:describe` turns `None` into "an entry with no string
  `command` (non-stdio, or malformed)", which is the designed unhappy path the doc
  asks for. A `String` field would have forced a sentinel value into the type.
- The guard lives in `register::register`/`register::unregister` (one seam, before the
  dispatch to either mechanism), not inside each mechanism — the Claude Code path and
  the desktop path are both destructive and both need the identical refusal.
- `entry_is_ours` sits in `src/register/desktop.rs` beside `read_config`/`key_present`,
  since that module already owns the read-only parse for BOTH mechanisms.
- An unreadable or malformed CONFIG FILE reports `Absent`, not `Foreign`. No destructive
  path follows it: the desktop writer re-reads and fails loudly with
  `Error::MalformedConfig`, and the Claude Code path's `remove` only fires when
  `is_registered` (same read) says the key is present. Reporting `Foreign` would have
  printed "registered to ..." about a file we could not parse, which is a false
  statement. Every undefined ENTRY shape still resolves toward `Foreign`.
- A config path that cannot be RESOLVED (`$HOME` unset) refuses the write, because an
  entry we cannot verify is not one we can safely replace.
- `"stdio"` and `["mcp", "serve"]` became `STDIO` / `SERVE_ARGS` consts in
  `src/register/claude.rs`, used by `entry_json` and by the predicate, so the writer
  and the check cannot drift. `tests/fixtures/contract.json` is unaffected: the emitted
  JSON is byte-identical.
- `status` reports ours | foreign | absent per target and keeps exit 0 for all three,
  per the doc (persona-cli asserts only the exit code).
- `is_registered` survives: after the guard has ruled the entry ours, the Claude Code
  path still needs plain PRESENCE to decide whether a re-add must `remove` first.

### Deviations
- `Foreign` carries `Option<String>`, not `String` (above) — same effect, correct seam.
- The doc's `fn entry_is_ours(io: &McpIo, path: &Path) -> Ownership` signature is
  implemented exactly, with a sibling `register::ownership(io, target)` that resolves
  the per-target path; `config_path(target)` was factored out of `is_registered` so
  both read-only checks share one resolution rule.
- `Error::ForeignEntry` names the key, the TARGET LABEL, the existing command, and
  `--force`, but not the config path. The target label maps to a documented path in the
  README table, and threading the path into the error would have made `guard` resolve
  it twice.
- Scope taken from the doc's PR-level bullet rather than a phase bullet: the four
  places carrying the now-false "zero risk" claim are corrected here, in the commit
  that makes them false — `src/register/claude.rs` (the `register` doc comment),
  `README.md`, and `docs/design/2026-07-09-mcp-io-rs.md` at both the Security bullet
  and the risk-table mitigation. Phase 2 is a test-only phase and would have stranded
  them. README also gains an "ownership check" section documenting the predicate and
  `--force`.

### Tradeoffs
- Guard in `register/mod.rs` vs inside each mechanism: one seam means one extra config
  read per verb even on the Claude Code path, which re-reads for `is_registered`. The
  doc budgets exactly this ("one extra small JSON read per register | unregister |
  status"), and the alternative duplicates the refusal in two places.
- `DesktopHome` in `src/register/tests.rs` redirects BOTH `$HOME` and
  `$XDG_CONFIG_HOME` and restores on `Drop`, rather than following the existing
  inline restore-at-the-end style: a failing assertion in these tests would otherwise
  leak an overridden `$HOME` into the rest of the process.
- Phase 1 tests cover the three ownership OUTCOMES and the two criteria end to end;
  the per-conjunct matrix (`type: "sse"`, absent `type`, wrong `args`) and the
  cross-verb/cross-path matrix are left to Phase 2, as the doc assigns them.

### Open questions
- `ENV_LOCK` is taken with `.lock().unwrap()` throughout `src/register/**/tests.rs`, so
  ONE panicking test poisons the mutex and every other env-touching test then fails with
  `PoisonError` instead of its own assertion. Observed while proving these tests bite.
  Phase 2 adds many more env-touching tests; `lock().unwrap_or_else(|e| e.into_inner())`
  would keep a single failure readable. Not changed here (pre-existing, and not this
  phase's scope).
