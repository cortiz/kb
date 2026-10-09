---
description: "Task list for M5: Variables"
---

# Tasks: Variables

**Input**: Design documents from `specs/006-variables/`
**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md),
[data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: Required. Constitution VIII: `kb-core` work (engine, escaping, prefill, memory, lint) is
red-green: the test task is written and seen failing before its implementation task. Glue (CLI
wiring, prompt, TUI form) is tested alongside, as in M3/M4. Unit tests live in `#[cfg(test)] mod tests`
at the bottom of the file they test.

**Organization**: Grouped by user story, but **phases run in this order**: Foundation → US4 (safe
escaping, core only) → US2 (`kb use`, scriptable) → US3 (secrets) → US1 (TUI form) → US5
(suggestions). Reason: nothing may copy a substituted value before escaping exists, and the CLI is
the cheapest end-to-end check of the engine before the form is built. US1 is the headline feature
but depends on all of them.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an incomplete task)
- **[Story]**: US1 to US5

## Path Conventions

Core: `crates/kb-core/src/` (engine in `varres/`). Binary: `crates/kb/src/` (`varcli.rs`, `tui/form.rs`).
Binary tests: `crates/kb/tests/`. Fixtures: `tests/fixtures/vars/`. FR/SC numbers refer to
[spec.md](spec.md); R-numbers to [research.md](research.md).

## Ground rules for every task

- **Never run `kb` or a test against `~/kb` or the real config dir.** Set `KB_ROOT` (temp copy of a
  fixture), `HOME`, `XDG_CONFIG_HOME` to temp dirs. Never write to the real clipboard from a test:
  use fake `pbcopy`/`wl-copy`/`xclip` scripts on `PATH` and `KB_CLIPBOARD_METHOD`, as M4's harness does.
- Tests use a marker such as `S3CR3T-MARKER` for secrets, never a realistic token (the lint secret
  scanner would flag fixtures).
- Every change keeps `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings`
  green and all existing tests passing (changed tests are listed per task).

---

## Phase 1: Setup

- [X] T001 Create `tests/fixtures/vars/` with one entry per case, each valid under `kb lint`: `sumo/5xx-by-authorizer.md` (enum `env`, number `minutes`, both in the payload), `cmd/git-checkout.md` (text `branch`, `shell: bash`), `cmd/grep-quoted.md` (`grep "{{pattern}}" file` and `echo '{{pattern}}'`), `cmd/fish-echo.md` (`shell: fish`), `link/search.md` (`url: https://example.com/?q={{query}}`), `cmd/curl-token.md` (secret `token`, `env: KB_TEST_TOKEN`), `sql/escape-sql.md` (`escape: sql`), `cmd/escape-none.md` (`escape: none`), `cmd/repeat.md` (same variable twice), `cmd/bad-default.md` (number with `default: 1e3`), `cmd/undeclared.md`, `cmd/escaped-braces.md` (`\{{literal}}`), `cmd/fields.md` (`cwd: {{dir}}`), `sumo/range.md` (`range: {{window}}`), `cw/log-groups.md` (`log_groups: [/aws/{{svc}}]`), `prompt/default-empty.md` (text with `default: ""`). Document them in `tests/fixtures/README.md`.
- [X] T002 [P] Create empty module files so the crates build: `crates/kb-core/src/varres/{mod,escape,secret}.rs` (declare `pub mod varres;` in `lib.rs`), `crates/kb/src/varcli.rs` and `crates/kb/src/tui/form.rs` (declared in `lib.rs` / `tui/mod.rs`).

**Checkpoint**: `cargo build --workspace` passes; behavior unchanged.

---

## Phase 2: Foundation (blocks every story)

**Purpose**: the `escape` key, the secret value type, planning, prefill, validation, plain
substitution, and memory in `state.json`. No command changes.

### Tests first (must fail)

- [X] T003 [P] Lint tests in `crates/kb-core/src/vars.rs`: `escape: shell|sql|none|auto` parse into `Variable.escape`; absent = `Auto`; `escape: bogus` gives KB054 `invalid-escape` (error) with a closest-value suggestion; a non-scalar `escape` gives KB054; `escape` on a `secret` is accepted (FR-007). Add a `tests/lint_fixtures.rs` case if that file lists rule codes.
- [X] T004 [P] Tests in `crates/kb-core/src/varres/secret.rs`: `format!("{:?}", Value{secret:true,..})` contains `••••` and not the text; a non-secret prints its text; `Value` is `Clone` and a cloned secret still prints `••••`; it does not implement `Serialize` (not compile-tested; the T035 scan covers it).
- [X] T005 [P] Tests in `crates/kb-core/src/varres/mod.rs` for `plan_vars`: order is by smallest `references[].span.start` (a variable in `url:` before the payload comes first; a variable declared first but used later comes later); the same variable used twice appears once with both locations; a declared-but-unreferenced variable is absent; undeclared = `text`, no default, `declared = false`; `\{{x}}` creates no variable; notes are never scanned (FR-001, FR-002).
- [X] T006 [P] Tests for `prefill` in `varres/mod.rs`, one per row of FR-010: flag beats env beats remembered beats default beats none; env set-but-empty is skipped; a remembered value that fails `validate` (option removed from the enum, number remembered for a now-enum field) is skipped silently; a secret never takes memory or default; an invalid declared default is skipped (C1); each binding reports its `source`.
- [X] T007 [P] Tests for `validate` in `varres/mod.rs`, quoting data-model.md verbatim: text "one line, no NUL; non-empty unless `default == Some("")`" (`default-empty.md` accepts `""`; a text with no default rejects `""`); a declared number default `1e3`, `.5`, `5.` is ignored by prefill (add `cmd/bad-default.md` to T001) and the binding falls through to `None`; number `^-?\d+(\.\d+)?$` accepts `30`, `-5`, `0.25` and rejects `abc`, `1e3`, `.5`, `5.`, `+3`, ` 3`; enum compared exactly (case-sensitive); secret non-empty one line. Error texts equal the data-model table; the secret error never contains the input; enum errors list the options (FR-005, FR-006).
- [X] T008 [P] Tests for plain `resolve` in `varres/mod.rs` (escaping off, `Escape::None` forced): single pass (a value `{{x}}` stays literal and is not expanded, FR-003); `\{{lit}}` comes out as `{{lit}}`; repeated variable substituted everywhere; fields `range`, `url`, `cwd`, `log_groups` are resolved, notes are not; `spans` ranges point at exactly the substituted text; `resolve` returns **all** errors, not the first; missing value is an error; same input gives identical bytes twice.
- [X] T009 [P] Tests in `crates/kb-core/src/usage.rs` for `VarMemory`: round trip of the `vars` key through `save`/`load`; absent or malformed `vars` gives empty memory and keeps `usage`; `STATE_VERSION` is still 1; `remember` moves a value to the front of `recent[name]`, dedupes, caps at 20; `carry_renames` moves `entries[old]` to `entries[new]`; `prune` drops entries of missing ids and keeps `recent`; a value flagged by `lint::secrets::scan_secrets` is not stored and is reported back to the caller; an older-format file with only `usage` still loads.

### Implementation

- [X] T010 In `crates/kb-core/src/vars.rs` add `pub enum Escape { Auto, None, Shell, Sql }` (`from_name`, `NAMES`) and `Variable.escape: Escape`; read it in `definition()`; update every `Variable { .. }` constructor. In `lint/mod.rs` add `InvalidEscape = 52, "invalid-escape", Error;` and wire the suggestion with `lint::suggest`. Makes T003 pass.
- [X] T011 [P] In `crates/kb-core/src/varres/secret.rs` implement `Value { text, secret }` with the redacting `Debug`, `Value::plain`, `Value::secret`, `reveal(&self) -> &str` (`pub(super)` only), no `Serialize`. Makes T004 pass.
- [X] T012 In `crates/kb-core/src/varres/mod.rs` implement `VarPlan`, `PlannedVar`, `Location`, `plan_vars` (R2) reusing `scan_refs` and `body::unescape`, including the signature hash. Makes T005 pass.
- [X] T013 In `varres/mod.rs` implement `Inputs`, `Binding`, `Source`, `prefill` (FR-010) and `validate` + `VarError` (FR-005/006). Makes T006 and T007 pass.
- [X] T014 In `varres/mod.rs` implement `Values`, `Resolved` (the spec's ResolutionResult), `resolve` (single pass, spans, all-errors, `has_secret`) with escaping stubbed to "as typed". Makes T008 pass.
- [X] T015 In `crates/kb-core/src/usage.rs` add `VarMemory`, `State.vars`, parse/save of the `vars` key, `remember`, and extend `carry_renames` and `prune`. In `search.rs` add `Kb::record_resolved_use(id, now, &[(name, String)]) -> Result<Vec<String>, KbError>` (one load/save; returns names skipped by the secret scan). Makes T009 pass.
- [X] T016 [P] In `crates/kb-core/src/config.rs` add the `vars.remember` setting (bool, default `true`, env `KB_VARS_REMEMBER`) to `SETTINGS`; extend the config tests that count settings or compare `config list` output, update `specs/004-platform-clipboard/contracts/config.md` ("ten settings" becomes eleven) and the `kb doctor` setting listing and its tests in `crates/kb/src/doctor.rs`.
- [X] T017 [P] In `crates/kb-core/src/entry_json.rs` add `escape` to each variable's JSON and update its snapshot/tests.
- [X] T018 [P] Boundary test `crates/kb/tests/varres_boundary.rs` (FR-004): scan `crates/kb/src/**` and `crates/kb-core/src/**` outside `varres/` for `scan_refs(` / `payload_pieces(` used to build output text and for the literal `"{{"` in non-test code; allow-list the lint and display call sites by file. It is an allow-list of the files that may name the scanners (it was green from the start, so it was never ignored).

**Checkpoint**: `cargo test -p kb-core` green; the engine plans, prefills, validates and substitutes (unescaped). Nothing user-visible yet.

---

## Phase 3: US4 — Values land safely in commands and URLs (P1)

**Goal**: FR-007 to FR-009, SC-003. Core only; the first caller arrives in US2.

**Independent test**: `cargo test -p kb-core varres::escape` and the hostile-value table pass for every shell and for URLs.

### Tests first (must fail)

- [X] T019 [P] [US4] Tests in `crates/kb-core/src/varres/escape.rs` from contracts/engine.md: `unquoted`, `in_single`, `in_double` for sh, bash, zsh and fish; `sql`; `percent`. The hostile-value table (`a b`, `a'b`, `a"b`, `` a`b` ``, `$(id)`, `${HOME}`, `a;b`, `a&&b`, `a|b`, `a>b`, `*`, `~`, `-rf`, `\`, `''`, empty, `{{x}}`, `é日本`, a 10,000-char string, `feature/x; rm -rf ~`, `a&b c`) with **literal expected outputs** per shell and per context, not recomputed with the code under test. Safe values (`abc`, `a-b.c/d`) stay unquoted; empty becomes `''`.
- [X] T020 [P] [US4] Tests for the quote-state tracker in `escape.rs`: `grep "{{p}}"` escapes inside double quotes (no nested single quotes); `echo '{{p}}'` escapes `'` as `'\''`; `echo {{p}}` quotes; `echo \"{{p}}` is unquoted (the backslash escapes the quote); `echo "a" {{p}}` is unquoted again after the closing quote; a value cannot change the state for later references; `cwd` is always unquoted context.
- [X] T021 [P] [US4] `resolve` tests in `varres/mod.rs` for FR-008/FR-009: `auto` in `cmd` payload and `cwd` = shell-aware; in `url` = percent-encoded; in `sql`, `sumo`, `cw`, `prompt` payloads, `range`, `log_groups` = as typed; `escape: shell` applies the unquoted rule everywhere including a `prompt` payload; `escape: sql` single-quoted with `''`; `escape: none` is raw in a `cmd` payload; a `link` url with `a&b c` gives `a%26b%20c`; spans cover the quoted text, quotes included (FR-019).
- [X] T022 [P] [US4] Round-trip test, skipped when the shell is missing: for each of `sh`, `bash`, `zsh`, `fish` found on `PATH`, run `<shell> -c 'printf %s <quoted>'` for every hostile value and assert stdout equals the original, in unquoted, single and double contexts (SC-003 "never runs anything": also assert that the file `$TMP/pwned` was not created by `$(touch …)` values).

### Implementation

- [X] T023 [US4] Implement `unquoted`, `in_single`, `in_double`, `sql`, `percent` and the tracker in `escape.rs` with a `ponytail:` comment stating the ceiling (no heredocs, comments, `$'…'`, `$(…)`, backticks). Makes T019 and T020 pass.
- [X] T024 [US4] Wire escaping into `resolve` in `varres/mod.rs` per the table in research R3, one function `escape_for(var, location, entry_type, shell, state)`. Makes T021 and T022 pass.

**Checkpoint**: US4 acceptance scenarios 1–4 hold at the engine level.

---

## Phase 4: US2 — `kb use` from the shell (P1)

**Goal**: FR-020 to FR-024, SC-005. The MVP of the feature: parameterized entries copy resolved from the CLI.

**Independent test**: `cargo test -p kb --test cli_use --test cli_vars`.

### Tests first

- [X] T025 [P] [US2] `crates/kb/tests/cli_use.rs` (assert_cmd, fake clipboard tool, temp `KB_ROOT`): all `--var` given → no prompt, exit 0, clipboard text equals expected; `--yes` uses pre-fills; `--yes` with a variable that has no pre-fill → exit 1 with the missing list; no terminal and a missing variable → exit 1, nothing copied, stderr lists name and kind; `--print` writes exactly the resolved text to stdout (and `$(kb use … -y --print)` style capture works); unknown `--var` → exit 2 and stderr lists the entry's variables; `--var` without `=` → exit 2; invalid `--var minutes=abc` → exit 2 with the FR-006 text; no match → exit 1; close matches without a terminal → listed, exit 1; with a terminal (pty) the picker opens and `--var`/`--yes` produce the stderr notice and are ignored, `--print` still applies; `--json` shape from contracts/cli.md; `--no-remember` and `vars.remember=false` leave `state.json` without `vars`; success records use and writes memory; `--method` is honored.
- [X] T026 [P] [US2] SC-005 test in `cli_use.rs`: for every fixture entry, answer the prompts through a pty with the pre-filled values by pressing Enter, and compare bytes with `kb use … --yes --print`; then with explicit `--var` for every variable; all three equal.
- [X] T027 [P] [US2] `crates/kb/tests/cli_vars.rs`: text and `--json` output for an entry with env, default, remembered and none cases, with each `source` label (R10); `no variables` for a plain entry; not found/ambiguous exit 1; `kb vars` does not record a use.
- [X] T028 [P] [US2] Prompt tests in `crates/kb/src/varcli.rs` using `FakePlatform` with a scripted tty: prompt line shows name, kind, options and `[prefill]`; Enter accepts the pre-fill; invalid answer re-asks with the reason; enum answer must be an option; EOF or Ctrl-C → exit 130 and nothing copied; `kb use x --print > file` still prompts (FR-023: the prompt goes to the tty, not stdout).
- [X] T029 [P] [US2] Test in `cli_use.rs` that `kb copy <entry-with-vars>` behaves as `kb use` with the same flags, `kb copy --raw` copies the template, `--raw` with `--var`/`--yes`/`--no-remember` exits 2, and an entry without variables copies exactly as in M4 (re-run the M4 `cli_copy.rs` unchanged).

### Implementation

- [X] T030 [US2] In `crates/kb/src/varcli.rs` add `VarArgs` (`--var`, `-y/--yes`, `--print`, `--no-remember`, `--method`, `--json`) with parsing of `NAME=VALUE` (first `=`, last wins) and the usage errors (exit 2).
- [X] T031 [US2] In `varcli.rs` implement `collect(platform, entry, plan, args, memory) -> Result<Values, Exit>`: prefill with `std::env` through the `Inputs` closure, missing-values report, and the interactive loop on `Platform::open_tty` (plain line prompts now; the masked branch is added in T038). Makes T028 pass.
- [X] T032 [US2] In `crates/kb/src/commands.rs` add `run_use` and route `copy_entry` through the same resolve step when the plan is non-empty: resolve, `clipboard::copy` or print, `record_resolved_use` (honoring `--no-remember` and `vars.remember`), report with the M4 line plus `--json` fields. `Print(Payload)` post-pick resolves too. Makes T025 and T029 pass.
- [X] T033 [US2] In `main.rs` add `Command::Use(UseArgs)` and `Command::Vars(VarsArgs)`, flatten `VarArgs` into `CopyArgs`, add `copy --raw`, include both commands in the `--json` flag mapping (~line 337) and the `-tag:` rewrite; implement `run_vars` in `varcli.rs`. Makes T027 pass; T026 should now pass.
- [X] T034 [US2] Update the M4 tests that assumed `kb copy` leaves `{{x}}`: grep `crates/kb/tests` and `src` for fixtures with variables (`tests/fixtures/search` has some) and add `--raw` or expected resolved text. List each changed test in the commit message.

**Checkpoint**: `quickstart.md` steps 1–7 and 14 pass. This is the MVP: every parameterized entry works from the shell. US2 acceptance scenarios 1–7.

---

## Phase 5: US3 — Secrets are used but never kept (P1)

**Goal**: constitution VI, FR-013, SC-004.

**Independent test**: `cargo test -p kb --test secrets_scan`.

### Tests first

- [X] T035 [P] [US3] `crates/kb/tests/secrets_scan.rs` (SC-004): with marker `S3CR3T-MARKER`, run `kb use curl-token` three ways (env `KB_TEST_TOKEN`, typed at the masked prompt through a pty, `--var token=…`), then `kb vars`, `kb use --json`, a failing run (invalid number alongside a secret) and a pty form run; afterwards grep every file under `$KB_ROOT/.kb`, the config dir, the temp dir, and all captured stdout/stderr **except** the `--print` stream for the marker. Assert it appears in the clipboard fake and in `--print` output only. Also assert a panic-path: `format!("{:?}", values)` and the `Debug` of the TUI `AppState` do not contain it.
- [X] T036 [P] [US3] In `cli_use.rs`: env-sourced secret shows `•••• (from $KB_TEST_TOKEN)` in `kb vars` and `value: null` with `env_set: true` in `--json`; the clipboard confirmation adds the secret notice; `--var token=…` prints the shell-history warning once and still works; a secret with no env, `--yes` and no terminal → exit 1 listing `token  secret`; memory never contains the variable.
- [X] T037 [P] [US3] Masked-prompt test through the pty harness (`tests/support/pty.rs`): typed characters are not echoed to the screen buffer (vt100 contents lack the marker and any `*`), Backspace edits, Enter submits, Esc/Ctrl-C exits 130, and the terminal is back in cooked mode afterwards (a following plain prompt echoes).

### Implementation

- [X] T038 [US3] In `varcli.rs` implement the masked prompt (R8): `crossterm::terminal::enable_raw_mode` on the tty, read key events, no echo, drop guard restoring the mode (also on panic). Makes T037 pass.
- [X] T039 [US3] In `commands.rs`/`varcli.rs`: the secret clipboard notice (`Resolved::has_secret`), the `--var` history warning, and `kb vars` secret rendering. Ensure `VarError` and every `eprintln!` in the new code use names, never `reveal()`. Makes T036 pass.
- [X] T040 [US3] Make T035 pass; fix every leak it finds. If `Debug` on a type holding `Values` was derived without the redacting type, replace the field type, do not hand-write `Debug` around it.

**Checkpoint**: US3 acceptance scenarios 1–5 except the TUI parts (done in US1). `quickstart.md` steps 8–10.

---

## Phase 6: US1 — Fill variables in the TUI (P1)

**Goal**: screen 2, FR-015 to FR-019, SC-001, SC-002.

**Independent test**: `cargo test -p kb --test tui_form_pty`; M3/M4 snapshots unchanged.

### Tests first

- [X] T041 [P] [US1] Pure `update()` tests in `crates/kb/src/tui/form.rs` (no terminal): Tab/Shift-Tab/↓/↑ move focus with wrap; ←/→ change an enum and move the text cursor; typing edits text and number fields with M3 line-editing keys (Home/End/Ctrl-A/Ctrl-W/Ctrl-U); Ctrl-Z resets to the pre-filled value; Ctrl-R on an env-sourced secret clears it and unlocks input; Enter with an invalid field focuses the first invalid one and sets its message, no outcome; Enter with all valid emits submit; Alt-Enter emits submit with `print = true`; pasted text keeps the first line and sets the notice (edge case); Esc closes and stores the session values.
- [X] T042 [P] [US1] Key map tests in `crates/kb/src/tui/keys.rs`: a new `Mode::Form` with the FR-017 bindings (including Alt-↓ for the suggestion list), `Alt-C` in list modes, `Alt-Enter` in form mode; the existing "no binding listed twice" check passes; help overlay lists them; the key bar text for the form equals `Tab next · Enter copy · Alt-Enter print · ^Z reset · Esc back`.
- [X] T043 [P] [US1] App-level tests in `tui/app.rs`: Enter on an entry whose preview says it has variables emits `Effect::OpenForm`; `Reply::Form` sets `Overlay::Vars`; `Reply::NoVars` falls through to the M4 `Pick`; Ctrl-Y opens the form with `then = Stay`; `Reply::FormChanged` reopens with the notice and keeps still-valid typed values; Esc returns to the same selection; typed values reappear when the same entry is opened again in the session and are never written; Enter on `Show` or `--print id` post-pick never opens the form; `Alt-C` emits a copy of the raw payload, shows `Copied template`, records a use and writes no memory.
- [X] T044 [P] [US1] Snapshot tests in `tui/view.rs` (follow the existing snapshot helper): wide (≥100 cols) with enum + number + text; narrow (80 cols, preview under fields, max 8 lines); secret from env (`•••• (from $KB_TEST_TOKEN)`); invalid field with inline message; scrolled field list with many variables and the preview still visible; "raw" marker for `escape: none`; "undeclared" marker; quoted value shown with quotes; header `title  field 2/3`.
- [X] T045 [P] [US1] (no worker: the same checks are the `FormSpec::load` / `recheck` behavior, covered by `tui_form_pty.rs` and the app tests) Worker tests in `tui/worker.rs`: `LoadForm` returns pre-filled bindings (memory and env applied) and suggestions; `NoVars` for plain entries; `Verify` returns `Ok` for an unchanged file and `FormChanged` after the test rewrites the entry's variables on disk.
- [X] T046 [P] [US1] `crates/kb/tests/tui_form_pty.rs` (pty, fake clipboard tool): select `5xx-by-authorizer`, Enter, choose `prod`, type `30`, Enter → clipboard equals the expected query and the TUI exited; invalid `abc` blocks and keeps the form; Esc returns to the list; Alt-Enter prints to stdout; Ctrl-Y copies and stays open and a second Esc/q quits; entry file edited between open and Enter reopens with the notice; an entry without variables copies on Enter exactly as in M4; SC-001 key-press count equals the plain-entry count plus one when the pre-fills are right.
- [X] T047 [P] [US1] Perf tests in `crates/kb/tests/perf.rs` (`#[ignore]`, release): `resolve` of a 1,000-line payload with 5 variables is ≤ 16 ms at p95 over 200 keystroke simulations (SC-002); and `kb use <id> -y --print` on the 10,000-entry perf KB starts and finishes in ≤ 100 ms (constitution VII cold start).

### Implementation

- [X] T048 [US1] Implement `FormState`, `Field`, `update()` and `FormEvent` in `tui/form.rs` per data-model.md, reusing `LineEditor` from `tui/input.rs`; secrets held as `Value`. Makes T041 pass.
- [X] T049 [US1] Add `Mode::Form` bindings, `Alt-C`, `Alt-Enter` to `tui/keys.rs` and the help text. Makes T042 pass.
- [X] T050 [US1] (done in the effect handlers of `tui/mod.rs` instead of the worker) In `tui/worker.rs` add `Request::LoadForm`, `Request::Verify` and the replies (R7); `FormSpec` built with `plan_vars` + `prefill` + `Kb` state. Makes T045 pass.
- [X] T051 [US1] In `tui/app.rs` add `Overlay::Vars(FormState)`, `Effect::{OpenForm, SubmitForm, CopyResolved}`, route Enter/Ctrl-Y/Alt-C, session values map, notices. Add `Outcome::Resolved { id, values, print }` in `tui/mod.rs`. Makes T043 pass.
- [X] T052 [US1] In `tui/view.rs` add `draw_form` (wide and narrow), preview highlighting from `Resolved::spans`, secret masking via `redacted()`, scrolling. Makes T044 pass; existing snapshots must not change.
- [X] T053 [US1] In `main.rs`/`commands.rs` handle `Outcome::Resolved`: re-run `resolve` (authoritative), copy or print, `record_resolved_use`, report; on failure print the errors and exit 1. `CopyResolved` (Ctrl-Y) goes through `clipboard::copy` with `Place::Ui`. Makes T046 pass.
- [X] T054 [US1] Run T047; if over budget, cache the scanned template pieces in `VarPlan` (already computed once) and re-substitute only; do not add a thread.

**Checkpoint**: `quickstart.md` steps 11–13 and 15. US1 acceptance scenarios 1–7; US3 scenarios 2–4 in the TUI.

---

## Phase 7: US5 — Suggestions from earlier use (P3)

**Goal**: FR-012, US5.

**Independent test**: the tests below.

- [X] T055 [P] [US5] Tests in `tui/form.rs` and the pty file: a focused `text` field shows up to five values from `recent[name]` (most recent first, current value excluded); before Alt-↓, ↑/↓ still move between fields; Alt-↓ enters the list, ↑/↓ choose, Enter or Tab accepts into the field and does not submit, Esc or typing closes it; no suggestions on `enum`, `number` or `secret`; memory written by a copy of entry A (`env=stage`) suggests `stage` in entry B's `env` field; the 21st distinct value evicts the oldest (core test already in T009).
- [X] T056 [US5] Implement suggestions in `tui/form.rs` and `view.rs` (list under the focused field), fed by `FormSpec.suggestions`. Makes T055 pass.

---

## Phase 8: Polish and cross-cutting

- [X] T057 [P] README: sections for `kb use`, `kb vars`, `copy --raw`, the `escape` key, secrets (what is and is not stored), the form keys (Alt-C, Alt-Enter, Ctrl-Z, Ctrl-R), `vars.remember`; fix the TUI Enter/Ctrl-Y text and the key table (constitution: docs in the same change). Add `escape` to the M1 format documentation and `tests/fixtures/README.md`.
- [X] T058 Run the whole suite: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, remove the `#[ignore]` from T018 (boundary) and confirm it passes, and confirm T035 (secrets scan) and all earlier M1–M4 tests are green; list any M3/M4 tests changed on purpose.
- [X] T059 [P] Remove `ponytail:` markers only if resolved; otherwise keep the one in `escape.rs`. Run `cargo test -p kb --release --test perf -- --ignored --test-threads=1` and record the SC-002 numbers in the plan's post-implementation section.
- [ ] T060 Walk `quickstart.md` by hand on this Mac (steps 1–15) and record results. Needs a person at a terminal.
- [ ] T061 Manual matrix from `quickstart.md` (Alt-Enter and Ctrl-Z delivery in Terminal.app, iTerm2, kitty, tmux, SSH; fish round trip; masked prompt over SSH). Needs a person.
- [ ] T062 Constitution VIII gate: push the branch and confirm CI is green on macOS and Linux. Needs your push.
- [X] T063 Write the "Post-implementation re-check" section of `plan.md` (what differed from the plan, as in M4).

---

## Dependencies and order

- Phase 1 → Phase 2 → Phase 3 (US4) → Phase 4 (US2) → Phase 5 (US3) → Phase 6 (US1) → Phase 7 (US5) → Phase 8.
- Inside a phase: tests (all `[P]`) first and seen failing, then implementation in the order listed.
- T010 before T003's green; T011, T012 can run in parallel with T010 (different files); T013 needs T011 and T012; T014 needs T013; T015 is independent of T012–T014; T024 needs T014 and T023.
- US2 (T030–T034) needs T014, T015, T024. US3 needs US2. US1 needs T015, T024, T032 (shared copy path) and T038 (secret input style). US5 needs US1.

## Parallel examples

- Foundation tests: T003 T004 T005 T006 T007 T008 T009 together, then T010 T011 T016 T017 together.
- US4: T019 T020 T021 T022 together.
- US1: T041 T042 T043 T044 T045 T047 together (different files), then T048 T049 T050.

## Implementation strategy

1. **MVP = through Phase 4.** After US2, every parameterized entry works from the shell with safe escaping; stop and use it.
2. Phase 5 makes secrets safe before anyone relies on them. 3. Phase 6 is the largest change and touches M3 code (`app.rs`, `keys.rs`, `view.rs`, `worker.rs`); the existing 91+ snapshots are the safety net. 4. US5 is optional polish.

## Totals

63 tasks. Setup 2, Foundation 16 (T003–T018), US4 6, US2 10, US3 6, US1 14, US5 2, Polish 7.
