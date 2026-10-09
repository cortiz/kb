# Research: Variables

"Checked" means read in this repository. "From practice" means documented behavior of a shell or
terminal that cannot be run here.

## R1. One engine, pure functions, a `Values` map as the only input

**Decision.** `kb-core::varres` exposes four pure functions over plain values:
`plan_vars(&Entry) -> VarPlan`, `prefill(&VarPlan, &Inputs) -> Vec<Binding>`,
`validate(&Variable, &Value) -> Result<(), VarError>`, `resolve(&Entry, &VarPlan, &Values) ->
Resolved`. `Inputs` holds the `--var` flags, an env lookup closure, and the `VarMemory` snapshot.
The TUI form, the CLI prompt and `--var` all end in a `Values` map.

**Why.** FR-004 and SC-005. If the three front ends produce a `Values` map and call one `resolve`,
interactive and scripted runs cannot differ. The env closure keeps tests from touching the process
environment (checked: `Environment` detection in M4 already works this way).

**Rejected.** An engine struct with state: nothing here needs state across calls, and the form
re-resolves per keystroke from the same plan.

## R2. Field order, locations and the scanner

**Decision.** `plan_vars` sorts variables by the smallest `VarRef.span.start` among their references
(file order, so frontmatter fields come before the payload). Declared-but-unreferenced variables are
dropped from the plan (lint already warns). Resolution scans, with `vars::scan_refs` and
`body::unescape`:

- the payload's raw text (as `payload::payload_pieces` does, `\r\n` normalized);
- the parsed string value of `range`, `url`, `cwd` and each `log_groups` item.

**Why.** Checked: `Entry::vars()` is declaration order (undeclared appended), so it cannot give
"first appearance". `references[].span` are file offsets, so min-span is exactly file order. Field
values cannot be sliced by file span (YAML quoting/escapes shift offsets), so they are scanned as
parsed strings, which is also what the user means.

`ResolutionResult` records, per output string, the byte ranges of each substituted value (FR-019),
computed during the single pass (R4), never by searching the output.

**Rejected.** Ordering by `vars()`: contradicts FR-001. Searching the output for values to highlight:
wrong when a value equals surrounding text.

## R3. Escaping, and why the shell needs a quote-state tracker

**Decision.**

| Where | `auto` | Rule |
|---|---|---|
| `cmd` payload | shell-aware | Track state while scanning the template: *unquoted*, *single*, *double*; a backslash outside single quotes escapes the next char. Unquoted: quote the value for the shell if it has any char outside `[A-Za-z0-9_@%+=:,./-]`, else as is. Inside `'…'`: replace `'` with `'\''` (fish: `\'`). Inside `"…"`: backslash-escape `\ " $ \``; fish: `\ " $`. |
| `cwd` | unquoted context | same as unquoted |
| `url` (link) | percent-encode | every byte outside `A-Za-z0-9-._~` as `%XX` over UTF-8 |
| `log_groups`, `range`, other payloads | unchanged | as typed |

- Unquoted quoting is `'…'` with `'\''` for `sh`, `bash`, `zsh`; fish uses `'…'` with `\\` and `\'`.
  Empty value → `''`.
- `escape: shell` applies the *unquoted* rule everywhere (inside quotes it would nest; the user chose it).
  For non-`cmd` entries the shell is `sh`. `escape: sql` → `'…'` with `'` doubled. `escape: none` → as typed.
- Bytes `\0` and newlines cannot occur (values are single-line and validated; NUL is rejected).

**Why a tracker.** Checked against the sample entries in `tests/fixtures`: saved commands put
variables inside double quotes (`grep "{{pattern}}" file`). Quoting only by value would produce
`"'a b'"` (wrong) and, for a value like `$(id)` that has no unquoted-special char in some rule sets,
leave command substitution live inside `"…"`. The tracker makes the quoting follow where the
variable sits.

**Ceiling (marked `ponytail:` in code).** The tracker does not understand heredocs, comments (`#`),
`$'…'` strings, `$(…)` nesting or backticks. Inside those, it treats the text by the plain quote
rules above, which still quotes conservatively. The user can pin behavior with `escape:`.

**Rejected.** Always `'…'` regardless of context (breaks `"{{x}}"`); a shell-parsing crate (a
dependency for a 40-line job, and it parses whole scripts we do not need); refusing templates that
quote variables (rejects real entries).

**Note for FR-008/US4.** The spec text says "quoted when they contain any special character"; the
tracker is a refinement for quoted contexts, recorded as an assumption in the spec.

## R4. Single pass and highlight ranges

**Decision.** One loop over `scan_refs(template)` output: append literal text (unescaped), then the
escaped value, recording `start..end` of the appended value in the output. Values are never rescanned.
The quote-state tracker runs over the *template's* literal pieces only (never over values), so a value
cannot change the state.

**Why.** FR-003 and the "`{{` in a value" edge case: a value is appended after the scan, so it cannot
be expanded. Ranges come for free.

## R5. Memory in `state.json`

**Decision.** `State` gains `vars: VarMemory { entries: BTreeMap<id, BTreeMap<name, String>>, recent:
BTreeMap<name, Vec<String>> }`, serialized as top-level key `"vars": { "entries": {...}, "recent":
{...} }`. `STATE_VERSION` stays 1.

- Remember on success: for every non-secret binding, `entries[id][name] = value`; `recent[name]`
  moves the value to the front, dedupes, truncates to 20 (FR-012).
- `carry_renames(...)` moves `entries[old]` to `entries[new]` with the usage record; `prune(existing)`
  drops `entries` for missing ids (FR-014). `recent` is never pruned by id.
- A remembered value is skipped (not stored; FR-011 allows this) when `lint::secrets::scan_secrets` flags it, and a
  one-line notice says so: a token typed into a `text` field must not land in `state.json`
  (constitution VI spirit; the spec only excludes `secret` kind).

**Why.** Checked: `usage.rs` already preserves unknown top-level keys in `extra`, so an older build
round-trips the new key; a version bump would force a rebuild for no gain. Deleting `state.json` loses
only usage and last-used values (constitution II).

**Rejected.** A second file: more files, no benefit. Storing under each usage record: `usage` records
are keyed and parsed strictly (`parse` returns `None` on a bad record), adding fields there risks the
whole file being reset on a bad value.

## R6. Secrets as a type

**Decision.** `Value { text: String, secret: bool }` where `Debug` for a secret prints `••••`,
`Serialize` is not implemented, and `Binding`/`Values` expose `reveal()` only to `resolve`. `VarMemory`
takes `&[(name, String)]` built from non-secret bindings only (a function that filters by `secret`,
tested). `VarError` messages are built from names and kinds, never values (the "expected a number"
message does not echo the input for secrets; for other kinds it does).

**Why.** `AppState`, `Options` and the form derive `Debug`/`Clone`, and `main` catches panics
(checked: `catch_unwind` in `main.rs`). A redacting `Debug` is the only protection that also covers a
panic message. Best-effort memory zeroing is not possible without `unsafe` (forbidden); documented as
the ceiling.

**SC-004** is a test: run `kb use` and the pty form with a marker token through env, typed and
`--var`, then grep `.kb/`, the config dir, the temp dir, and captured stdout/stderr for the marker.
(`--print` of a resolved secret is the one place the secret is *meant* to appear; the test captures
that stream separately and asserts it appears there only.)

## R7. Opening and submitting the form (built without the worker: see contracts/tui-form.md)

**Decision.** Enter / Ctrl-Y on an entry whose post-pick delivers the payload sends
`Request::LoadForm { id }`. The worker reads the entry fresh, builds `VarPlan` and prefilled bindings
(memory + env), and replies `Reply::Form(FormSpec)` or `Reply::NoVars` (then the old Enter path
runs: copy as in M4). Submit sends `Request::Verify { id, signature }`; the worker re-reads and
compares the plan *signature* (names, kinds, options, defaults, escape, shell, a hash of the template
pieces). Same → `Outcome::Resolved`; different → `Reply::FormChanged(FormSpec)` and the UI reopens
with a notice, keeping typed values for variables that still exist and validate.

**Why.** Matches the existing request/reply pattern (checked: `worker.rs` for preview and refresh),
gives both "re-read" points of the edge case, and keeps `update()` pure.

**Rejected.** Reading synchronously in `update()`: breaks the pure-update tests. Checking only in the
CLI after exit: the spec says the form reopens.

## R8. Prompts on `/dev/tty` and masked input

**Decision.** `varcli::prompt` opens `Platform::open_tty()` for both reading and writing (FR-023).
Plain prompts: write `name (kind: a|b|c) [prefill]: ` and read a line (the tty is in cooked mode, so
the terminal echoes and edits). Secret prompts: `crossterm::terminal::enable_raw_mode`, read key
events (`use-dev-tty` is already enabled) and echo nothing; Enter ends, Backspace edits, Ctrl-C/Esc
cancel (exit 130). Raw mode is restored by a drop guard, including on panic.

**Why.** No `termios` without `unsafe`; `crossterm` is already a dependency and already reads from
`/dev/tty`. Prompting only when `has_terminal()` is true and `--yes` is absent gives US2.4: no
terminal → no prompt → list missing and exit 1.

**Rejected.** A `rpassword`-style crate: new dependency for what `crossterm` already does.

## R9. Where `--yes`, `--print`, `--no-remember` and `vars.remember` act

**Decision.** `--yes` means "never ask" (spec US2.3); any variable with no flag and no prefill is *missing* and
the command exits 1 listing it (same path as no terminal, so the two cannot drift). `--print` writes
the resolved payload to stdout, bytes exactly as the clipboard would get (no trailing newline added
beyond what the payload has), and still records use and memory. `--no-remember` or `vars.remember =
false` skips only the memory write; the use is still recorded. A secret passed by `--var` prints the
shell-history warning to stderr once.

## R10. `kb vars` source labels

**Decision.** Each variable reports `source` ∈ `flag | env | remembered | default | none` and, for
non-secrets, the value that would be pre-filled; for secrets `value` is null and `source` is `env` with
`env_name` and `env_set: bool`. In text output a secret shows `•••• (from $NAME)` or `(prompt)`.

**Why.** FR-010 asks for the source; US3.3 forbids the value; `env_set` lets a script check without
seeing it.
