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

## Phase 2 (W2): Cover the reported scenario end to end

Test-only phase: no production code in `src/register/desktop.rs`, `claude.rs`, or
`mod.rs` changed. `git diff` on `src/register/desktop.rs` is empty at commit time.

### Design decisions
- The two `type`-conjunct fixtures (success criterion 3) land as direct unit tests on
  `entry_is_ours` in `src/register/desktop/tests.rs`, the same style as Phase 1's three
  outcome tests: `test_entry_is_ours_reports_foreign_when_type_is_not_stdio` (our
  command basename + our args + `type: "sse"` -> `Foreign`) and
  `test_entry_is_ours_accepts_a_bundle_entry_with_no_type_key` (command + args, no
  `type` key at all -> `Ours`, the `.mcpb` bundle-installed shape).
- Claude Code path coverage (`src/register/tests.rs`) needed no faked `claude` binary.
  `guard()` reads the target's config file directly via `ownership()` and refuses
  BEFORE `claude::register`/`claude::unregister` ever calls `Command::new("claude")`,
  so `test_register_refuses_a_foreign_entry_on_claude_user_scope_...` and its
  `unregister` counterpart exercise the real production guard unconditionally, with no
  `claude_available()` skip and no fixture CLI. A `ClaudeUserHome` helper mirrors the
  existing `DesktopHome` (`CLAUDE_CONFIG_DIR` redirected to a temp dir, restored on
  `Drop`), pointed at `claude::config_path(Target::User)` instead of the desktop
  resolver.
- `status`'s foreign-entry coverage (`test_status_reports_foreign_ownership_and_never_writes`)
  asserts `ownership(&io, target)` directly for both `Target::Desktop` and
  `Target::User`, since that private fn is exactly what backs every line `status`
  prints, plus a call to `status` itself (asserting `EXIT_SUCCESS`, its documented
  behavior) and a before/after byte comparison on both config files, since `status`
  never writes.
- `entry_is_ours`, `ownership`, and `Ownership` are `pub(crate)`/private items reached
  through `use super::*;`; `tests` is a child module of `register`, so it sees them
  without any visibility change.

### Deviations
- Success criterion 1, read literally ("each asserts BOTH the non-zero result AND
  byte-identical", "per verb" naming `register`/`unregister`/`status`), does not hold
  for `status` as specified: `status` is documented in this same design (Architecture,
  and W2 Phase 1's own success criteria) to keep exit 0 for all three ownership states,
  and Phase 1 already shipped and tested that. Making `status` return non-zero on a
  foreign entry would be a PRODUCTION behavior change, which this phase is not
  authorized to make and which the design doc itself forbids elsewhere. Implemented at
  the correct seam instead: `status`'s foreign-entry test asserts the exit code stays
  `EXIT_SUCCESS` (matching the documented contract) and asserts the `ownership()` call
  that backs its report returns `Foreign`, which is the assertion that actually breaks
  if the predicate regresses. The config-file-untouched half of the criterion holds
  trivially for `status` (it is read-only on every path) and is still asserted for both
  configs.
- `ENV_LOCK.lock().unwrap()` -> `.lock().unwrap_or_else(|e| e.into_inner())` across
  `src/register/tests.rs` and `src/register/claude/tests.rs` (12 call sites; no
  occurrences in `src/register/desktop/tests.rs`). Authorized by the parent as scope
  beyond this phase's bullets: it is what Phase 1 recorded as an open question, and
  without it the break-to-prove step (criterion 2) is unreadable, since the first test
  that panics against a reverted predicate poisons the shared mutex and every later
  env-touching test in the same run then fails with `PoisonError` instead of its own
  assertion, masking which assertions actually caught the regression. No behavior
  change outside the test binary: `ENV_LOCK` only ever guards process-env mutation
  inside `#[cfg(test)]` code.

### Tradeoffs
- Faked `claude` CLI vs relying on the guard's short-circuit: the phase brief allowed
  for a faked `claude` CLI on the Claude Code path. Not built, because the refusal
  path, which is the behavior success criteria 1 and 2 need, never reaches
  `Command::new`, so a fake binary would add a moving part (a script to keep
  byte-compatible with `claude mcp add-json`/`remove` output) that no assertion needs.
  The existing `claude_available()`-gated round-trip tests (`test_claude_user_roundtrip`,
  `test_register_is_idempotent_and_bakes_env`) already cover the live-CLI success
  path and are unmodified.
- Break-to-prove was done by temporarily short-circuiting `entry_is_ours` to
  presence-only (`return Ownership::Ours;` right after the key-presence check) rather
  than reverting to a saved pre-Phase-1 copy of the function, since Phase 1's version
  already replaced the old presence-only body; the short-circuit reproduces the exact
  pre-Phase-1 defect (any present entry reads `Ours`) with a two-line, easy-to-revert
  change. Reverted before running `cargo fmt`/`otto ci`; `git diff` on
  `src/register/desktop.rs` is empty at commit time.

### Open questions
- None new. Phase 1's ENV_LOCK poisoning item is resolved by this phase's authorized
  fix and does not carry forward. This is the last phase in this repo (W2 Phase 2 of
  2); nothing else is open on the mcp-io-rs side of this design doc.
