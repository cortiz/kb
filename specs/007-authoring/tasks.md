---
description: "Task list for M6: Authoring"
---

# Tasks: Authoring

**Input**: Design documents from `specs/007-authoring/`
**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md),
[data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: Required. Constitution VIII: `kb-core` work (detection, suggestions, render, check, file
operations, state moves) is red-green: the test task is written and seen failing before its
implementation task. Glue (CLI wiring, prompts, TUI form) is tested alongside, as in M3–M5. Unit tests
live in `#[cfg(test)] mod tests` at the bottom of the file they test.

**Organization**: Grouped by user story, but **phases run in this order**: Foundation → US2 (`kb add`,
scriptable) → US1 (TUI form) → US3 (editor template) → US4 (edit, mv, rm) → US5 (suggestions in the
form). Reason: `kb add --yes` is the cheapest end-to-end check of detection, rendering, validation and
file creation before the form exists; US1 reuses all of it. US5's engine is core-only and could be done
early, but its UI is part of the form, so it comes last.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an incomplete task)
- **[Story]**: US1 to US5

## Path Conventions

Core: `crates/kb-core/src/` (authoring in `author/`). Binary: `crates/kb/src/` (`author.rs`,
`editfile.rs`, `platform/`, `tui/add.rs`, `tui/add_view.rs`). Binary tests: `crates/kb/tests/`.
Fixtures: `tests/fixtures/authoring/`. FR/SC numbers refer to [spec.md](spec.md); R-numbers to
[research.md](research.md).

## Ground rules for every task

- **Never run `kb` or a test against `~/kb`, the real config dir, or the real clipboard.** Set
  `KB_ROOT` (temp copy of a fixture), `HOME`, `XDG_CONFIG_HOME` to temp dirs. Tests read the clipboard
  through `FakePlatform` or fake `pbpaste` / `wl-paste` / `xclip` scripts on `PATH`; editors in tests
  are scripts (`EDITOR` / `KB_EDITOR`), never `vi`.
- Tests use markers such as `S3CR3T-MARKER` and the secret patterns already in `tests/fixtures/secrets/`;
  never a real token.
- Temp files in tests go in a temp dir; assert none are left behind after each flow.
- Every change keeps `cargo fmt --check` and `cargo clippy --workspace --all-targets -- -D warnings`
  green and all existing tests passing (changed tests are listed per task).

---

## Phase 1: Setup

- [X] T001 Create the detection corpus `tests/fixtures/authoring/detect/<type>/NN-name.txt`: at least 10 real-world style snippets for each of `sql sumo cw cmd prompt link code` (at least 100 in total), including: a CloudWatch query with `| parse @message` (must not be sumo), a Sumo query with `_sourceCategory` and one with only `| timeslice 5m | count by`, `WITH` CTEs, a `kubectl`/`aws`/`git` pipeline with `$ ` prompt markers, a Markdown-fenced SQL snippet, GitHub and GitLab `/blob/<ref>/<path>#L147` and `#L10-L20` URLs, plain URLs, and multi-line natural-language prompts. Put deliberately ambiguous snippets (lone `WITH`, one-word commands, `filter`/`sort` prose) in `detect/low/<expected>/`. Document the layout in `tests/fixtures/README.md`.
- [X] T002 [P] Create `tests/fixtures/authoring/golden/`: for each of the seven types an input (`NN-<type>.in.txt` with the options in a first-line `# opts:` header, kept out of the body) and the expected file (`NN-<type>.md`) that must render byte for byte (FR-018 order: `kb_format`, `type`, `title`, `tags`, type fields, `vars`, `created`, `updated`, blank line, fence, blank line, notes). A fixed `now` of `2026-10-09T12:00:00Z` is used in the tests.
- [X] T003 [P] Create `tests/fixtures/authoring/suggest/`: pasted snippets with the expected accepted body and declarations for each FR-015 pattern (env short and long form, region, `-30m` / `24h` / `7d` / `INTERVAL '1 day'`, account in an ARN and after `account=`, UUID, long numeric id), plus negative cases (`product`, `devops`, `now-30` in prose, digits inside `{{…}}` and `\{{…}}`, an 11-digit number).
- [X] T004 [P] Create empty module files so the crates build: `crates/kb-core/src/author/{mod,detect,suggest,draft,render,fsops}.rs` (declare `pub mod author;` in `lib.rs`), `crates/kb/src/author.rs` and `crates/kb/src/editfile.rs` (declared in `lib.rs`), `crates/kb/src/tui/{add,add_view}.rs` (declared in `tui/mod.rs`).

**Checkpoint**: `cargo build --workspace` passes; behavior unchanged.

---

## Phase 2: Foundation (blocks every story)

**Purpose**: clipboard reading, in-memory lint, state moves, the `path_template` setting, detection,
the Draft with canonical rendering and checking, and safe file creation. No command changes yet.

### Tests first (must fail)

- [X] T005 [P] Write failing tests for `lint::check_text` in `crates/kb-core/src/lint/mod.rs`: a valid entry gives no findings; a missing title, an unknown type and a fixture AWS key each give the same rule codes as `kb lint` on the same bytes written to a file (compare against `lint()` on a temp KB); `kb:allow-secret` suppresses as in `kb lint`.
- [X] T006 [P] Write failing tests for `State::move_entry` and `State::forget` in `crates/kb-core/src/usage.rs`: usage record and `vars.entries[old]` move to `new`; a stale record at `new` is overwritten; unknown top-level keys survive `save`; moving an unknown ID returns false; `forget` removes both records; `STATE_VERSION` is still 1.
- [X] T007 [P] Write failing tests for `PathTemplate` in `crates/kb-core/src/config.rs`: `{type}/{slug}` is the default (empty setting); `{tag}/{slug}` uses the first tag and `{type}` when there are none; a template without `{slug}` or with an unknown placeholder is rejected by `parse`; an invalid configured value gives a `ConfigWarning` and the default; `path_template` appears in `SETTINGS`, `config list` and `config init`, and the existing "eleven settings" assertions say twelve (update `cli_config.rs`, `config.rs` and the `config.*` snapshots listed by the failing tests).
- [X] T008 [P] Write failing tests for `detect` in `crates/kb-core/src/author/detect.rs`: the corpus test over `tests/fixtures/authoring/detect/` asserting (a) at least 95% of all snippets detect the right type (SC-002), (b) every snippet detected `High` has the right type, (c) detection is deterministic (two runs give equal results), (d) every `detect/low/<expected>/` snippet is `Low`; unit tests for: fence unwrapping and language-hint precedence (`sql`, `bash`, `sh`, `zsh`, `shell`), `$ ` and `% ` stripped only when every non-empty line has one and the type is `cmd`, GitHub/GitLab URL fields (`repo`, `ref`, `path`, `line`, `end_line`), plain URL → link with `url`, CRLF normalization, empty/whitespace → `cmd` with `Low`, and rule 3 vs 4 (a text with `@message` is never sumo by operator; `_sourceCategory` always is).
- [X] T009 [P] Write failing tests for the title and slug rules in `crates/kb-core/src/author/draft.rs`: title from `--title`, then first comment line within the first 3 lines (`-- `, `# `, `// `, shebang ignored), then link host + last segment, then code symbol or `file:line`, else empty; slug is lowercase ASCII, accents removed, words joined by `-`, at most 60 characters, cut on a `-` boundary; an empty slug is `DraftError::NeedsPath`; clashes append `-2`, `-3` against the `taken` callback; case-insensitive clash counts.
- [X] T010 Write failing tests for variables in a draft in `crates/kb-core/src/author/draft.rs`: every `{{name}}` is declared once in first-appearance order with `kind: text`; `minutes limit days count port` default to `number`; `token password secret key api_key` and names ending `_token _password _secret _key` default to `secret` with no default; `set_var_default` on a secret → `DraftError::SecretDefault`; an enum default not in `options` → `DraftError::DefaultNotInOptions`; `mark_literal` rewrites `{{x}}` to `\{{x}}` and drops the declaration; `set_body` keeps decisions by name, drops declarations whose variable is gone.
- [X] T011 [P] Write failing golden tests for `Draft::render` and `Draft::check` in `crates/kb-core/src/author/render.rs`: every `golden/NN-<type>.in.txt` renders to exactly `NN-<type>.md`; `parse_entry(render(d)).to_bytes() == render(d)`; the result passes `check_text` with no findings (SC-003); `created == updated` on creation; the fence is one backtick longer than the longest backtick run in the body (minimum 3); titles needing quotes are quoted with the same rule as `set_field`; `link` and `code` render no fence; an empty notes section renders no trailing blank line; a rendered file over 1,048,576 bytes makes `Check.too_large` true; a fixture secret in body, title or notes gives secret findings and `writable(true)` is false (SC-004).
- [X] T012 [P] Write failing tests for the file operations in `crates/kb-core/src/author/fsops.rs` with a temp KB: `create_entry` writes atomically and creates folders, refuses an existing path (`Exists`) without touching it (also with a different case), re-scans for secrets itself and refuses a `Check` that was marked writable by mistake, leaves no temp file after success or failure, and the next `search` finds the entry (FR-021); `rename_entry` refuses an existing target, moves state, creates folders, removes empty source folders but never the root or `.kb`, calls the `mover` and leaves everything unchanged when the mover fails; `remove_entry` forgets state; `save_edit` returns `Conflict` when the bytes differ from `based_on`; `restore` writes the snapshot back atomically; `tag_counts` orders by count then name and falls back to a file scan when the index is stale; `id_taken` is case-insensitive.
- [X] T013 [P] Write failing tests for the clipboard read in `crates/kb/src/platform/clipboard.rs` using `FakePlatform`: tool choice per environment (macOS `pbpaste`; Wayland `wl-paste`; X11 `xclip -selection clipboard -out`, `xsel --clipboard --output`; XWayland tries `wl-paste` first); SSH → `Unavailable(OverSsh)` and no tool run; a missing tool falls through to the next; a timeout (2 s) falls through; no tool → `Unavailable(NoTool)` with the install hint; invalid UTF-8 or NUL → `Empty`; CRLF and CR → LF; whitespace-only → `Empty`; output over 2 MiB → `Unavailable(TooLarge)`; the last tool exiting non-zero with empty output → `Empty`.

### Implementation

- [X] T014 Implement `lint::check_text(id, bytes) -> Vec<Finding>` in `crates/kb-core/src/lint/mod.rs` by building a `WalkEntry` (id, `<id>.md`, a dummy `abs_path`) and calling `check_file`; no second definition of "has errors". Makes T005 pass.
- [X] T015 Implement `State::move_entry` and `State::forget` in `crates/kb-core/src/usage.rs` (R7). Makes T006 pass.
- [X] T016 Implement `PathTemplate` and the `path_template` setting (`Kind::Text`, default empty, env `KB_PATH_TEMPLATE`, doc as in `contracts/platform-and-config.md`) in `crates/kb-core/src/config.rs`; `Settings::path_template()`; update `config init` output and the changed snapshots/tests named by T007. Makes T007 pass.
- [X] T017 Implement `detect`, `normalize_clipboard`, `strip_fence`, `strip_prompts`, `Detection`, `Confidence` in `crates/kb-core/src/author/detect.rs` per R2 and `contracts/core-api.md`. Iterate rules against the corpus until T008 passes; every rule change is driven by a corpus miss, not by adding special cases.
- [X] T018 Implement `Draft` (`from_text`, `empty`, `set_type`, `set_body`, variable setters, `mark_literal`, `target_id`) and the title, slug and variable-kind rules in `crates/kb-core/src/author/draft.rs`, plus `DraftOptions`, `DraftSource`, `DraftFields`, `VarDecl`, `PathChoice`, `DraftError`. Constraints verbatim from data-model.md: title 1–120 characters; tags lowercase kebab, duplicates dropped; secret variables never have a default; enum default must be in `options`; changing the type drops type fields that do not apply and keeps title, tags, body, notes, vars. Makes T009 and T010 pass.
- [X] T018a Add `Clock` / `SystemClock` in `crates/kb-core/src/author/mod.rs`; `Draft::render`, `check` and `Kb::save_edit` take `&dyn Clock`. `kb` passes `SystemClock`, except that when `KB_TEST_NOW` (RFC 3339) is set in a debug build it passes a fixed clock; release builds ignore the variable. Test: with it set, `kb add --yes` writes that `created` and `updated`; in a release build it is ignored.
- [X] T019 Implement `Draft::render` (canonical layout, fence length, YAML quoting via the `edit.rs` quoting helpers made `pub(crate)`, UTC `YYYY-MM-DDTHH:MM:SSZ` from `SystemTime` with a civil-from-days function, no new dependency), `Draft::check`, `Check` (`secrets`, `errors`, `writable`, `too_large`) in `crates/kb-core/src/author/render.rs`. Makes T011 pass.
- [X] T020 Implement `Kb::create_entry`, `rename_entry`, `remove_entry`, `save_edit`, `restore`, `version`, `tag_counts`, `id_taken`, `Version`, `FsError` in `crates/kb-core/src/author/fsops.rs` and `crates/kb-core/src/search.rs` (atomic no-clobber create via `hard_link` with a `rename` fallback, per R7; `refresh` before returning). Makes T012 pass.
- [X] T021 Add the platform pieces in `crates/kb/src/platform/mod.rs`, `unix.rs`, `fake.rs`: `Platform::run_read_tool`, `run_git`, `private_temp_file` / `TempFile` (system temp dir, `0600`, `create_new`, delete on drop), the four read tool constants and `read_tools_for` in `clipboard.rs`; `FakePlatform::with_read`, `with_git` and an in-memory temp store. Extend `crates/kb/tests/platform_boundary.rs`: `Command::new` and temp-file creation appear only under `platform/`, and `TempFile` never lands under the KB root.
- [X] T022 Implement `clipboard::read` and `ReadOutcome` / `Unavailable` in `crates/kb/src/platform/clipboard.rs` (R10). Makes T013 pass.
- [X] T023 [P] Add the `clipboard-read` check to `crates/kb/src/doctor.rs` (tool lookup only, never reads the clipboard; pass/warn with install hint; same JSON shape) with tests in the existing doctor test module and `crates/kb/tests/cli_doctor.rs` (macOS, Wayland, X11, SSH, nothing installed).

**Checkpoint**: `cargo test --workspace` passes; no command behavior changed except `kb doctor` gaining one
check and `config` listing twelve settings.

---

## Phase 3: User Story 2 — Capture from the shell (P1)

**Goal**: `kb add` with the body from stdin, the clipboard or nothing, saved non-interactively with
`--yes`, printing the ID.

**Independent test**: `echo 'kubectl rollout restart deploy/api' | kb add -t "Restart api" --tag k8s --yes` prints `cmd/restart-api` and the file passes `kb lint`.

### Tests first (must fail)

- [X] T024 [US2] Write failing tests in `crates/kb/tests/cli_add.rs` (temp KB, `FakePlatform` or fake clipboard scripts): piped stdin becomes the body and wins over the clipboard; `--clipboard`, `--stdin`, `--empty` force the source; two of them together exit 2; `--yes` with title saves and prints only the ID on stdout (everything else on stderr); the written file equals the expected bytes for a fixed clock (use `KB_TEST_NOW` (T018a); `--yes` without a title exits 1 with `a title is required (use --title)`; low confidence without `--type` exits 1 with `could not tell the type (use --type)`; `--type` overrides detection; repeated and comma-separated `--tag`; `--path` creates folders; a clashing path gets `-2`; a configured `path_template` of `{tag}/{slug}` with `--tag ecs` gives ID `ecs/<slug>`; an empty slug without `--path` exits 1; secret content exits 1, writes nothing, names the rule and the three ways out; an entry over 1,048,576 bytes exits 1 with the limit; `--json` prints the `kb show --json` shape; `--yes --editor` exits 2; no terminal and no `--yes` exits 2; the clipboard over SSH prints one explanation line and opens an empty draft (with `--yes` and a title and `--type`, it saves an empty-body entry only if lint allows, else exit 1); the new entry shows up in `kb search` immediately (FR-021).
- [X] T025 [P] [US2] Write failing tests that stdin reading is capped at 1 MB + 1 bytes in `crates/kb/src/author.rs` (a reader of unbounded zeros stops) and that the cap yields the size refusal rather than a hang.

### Implementation

- [X] T026 [US2] Implement `crates/kb/src/author.rs`: `AddArgs` handling, body source selection (R12), `Draft::from_text` / `empty`, the non-interactive save path with every refusal above, output formats, exit codes (0/1/2/130 per FR-030). Load `Settings::path_template()` and pass it in `DraftOptions`. Wire `Add` into `crates/kb/src/main.rs` (clap `conflicts_with_all`, `--tag` repeatable with comma splitting, `-t`, `-y`) and `crates/kb/src/commands.rs`. Makes T024 and T025 pass.
- [X] T027 [US2] Document `kb add` (flags, sources, `--yes`, exit codes, pipe example, what is never auto-applied) in `README.md`.

**Checkpoint**: shell capture works end to end; `quickstart.md` scenario 2 passes.

---

## Phase 4: User Story 1 — Capture from the clipboard in the TUI (P1) 🎯 MVP

**Goal**: Ctrl-N → add form (screen 4) → Ctrl-S → saved, indexed, selected; `kb add` with no flags opens the same form.

**Independent test**: with the fake clipboard holding a CloudWatch query, Ctrl-N, type a title, Ctrl-S: the written file equals the golden file byte for byte and the entry is selected with `Saved cw/authorizer-p99-latency` on the key bar.

### Tests first (must fail)

- [ ] T028 [P] [US1] Write failing pure tests for `AddState` in `crates/kb/src/tui/add.rs`: initial focus is Title when text and a `High`/`Medium` type were found, Type when the body is empty, the clipboard was unavailable, or confidence is `Low`; the header message explains `Unavailable` once; Tab/Shift-Tab order `Type → Title → Tags → Variables → Path → Notes`; `←` `→` change the type, drop inapplicable fields and keep focus; typing in Title/Tags/Path/Notes edits them; tag completion lists up to 5 prefix matches by frequency and Tab accepts the first when in a partial tag; invalid tags show an error style; clearing Path returns to auto; `changed` is false on open and true after any edit; Esc on unchanged closes, on changed asks `Discard this entry? (y/N)` and only `y` closes; submit with an empty title focuses Title with `a title is required`; submit with a failing check focuses the offending field and writes nothing; no "save anyway" exists in the form; a secret finding focuses the body view and records the range to highlight.
- [ ] T029 [P] [US1] Write failing tests for the list keys in `crates/kb/src/tui/keys.rs` and `app.rs`: `Ctrl-N` → `Command::Add`, `Ctrl-R` → `Rename`, `Ctrl-X` → `Delete` in Search mode; `Ctrl-N` is gone from `Down` while `↓` and `Ctrl-J` remain; `has_duplicates(BINDINGS)` is false; the README "every bound key is documented" test and the six `help.*` snapshots fail until updated; `Ctrl-N` with the clipboard `Text` opens the form via `Effect::ReadClipboard` → `Action::ClipboardRead`; a failed save keeps the form open with the message; `Action::Saved(id)` closes the form, re-runs the query and selects the id.
- [ ] T030 [P] [US1] Write failing snapshot tests in `crates/kb/tests/tui_snapshots.rs` (new snapshots under the existing snapshot dir): the form wide, narrow, `link` (field inputs instead of body), with a secret highlight and message, with the preview toggled (Ctrl-P), and with the dirty-confirm bar; `NO_COLOR` and 16-color variants for the wide one.
- [ ] T031 [P] [US1] Write failing pty tests in `crates/kb/tests/tui_add_pty.rs`: fake clipboard with the CloudWatch fixture → `Ctrl-N`, title text, `Ctrl-S` → file equals `golden/` bytes, entry selected, bar shows `Saved cw/authorizer-p99-latency`, and the key presses other than title text are at most 5 (SC-001, count asserted); empty clipboard → focus on Type; Esc with changes asks and `n` keeps the form; `Ctrl-E` with a scripted editor that rewrites the body → body replaced, detection and variable list re-run, `changed` set (US1.5); an editor exiting non-zero or leaving the file unchanged leaves the body alone; terminal restored after the editor; no temp file left; `kb add` with no flags in a pty opens the form on the clipboard text and prints the ID after the UI closes, exit 0; Esc in it exits 130 and prints nothing.

### Implementation

- [ ] T032 [US1] Implement `AddState` (fields, focus, `handle_key`, `update`, dirty tracking, tag completion data, submit validation using `Draft::target_id` and `Draft::check`) in `crates/kb/src/tui/add.rs`. Makes T028 pass.
- [ ] T033 [US1] Implement `draw_add` (wide and narrow layouts per `contracts/tui-add-form.md`, type chips in the list's type colors, body through the existing highlighter, secret range highlighting, completions, Variables and Saves-to sections, key bar text) in `crates/kb/src/tui/add_view.rs`; dispatch from `view.rs`; Ctrl-P preview renders `Draft::render` through the existing preview renderer. Makes T030 pass (review and accept new snapshots by hand).
- [ ] T034 [US1] Wire the form into the app: `AppState.add`, `Effect::{ReadClipboard, EditBody, SaveDraft}`, `Action::{ClipboardRead, BodyEdited, Saved, SaveFailed}`, `Outcome::Added { id }`, key routing while the form is open, the `Saved <id>` message and selection by id after refresh, in `crates/kb/src/tui/app.rs`, `mod.rs`, `worker.rs` as needed. Implement `EditBody` in `crates/kb/src/tui/mod.rs` on the model of `edit_entry` (suspend, `private_temp_file` with an extension from the type — `.sql`, `.sh`, `.txt`, `.md` for others —, run the editor, resume and resize, drop the temp file, handle signals). Load `Settings::path_template()` for the form's draft. An entry over 1,048,576 bytes from the editor is rejected with the limit message.
- [ ] T035 [US1] Add the keys in `crates/kb/src/tui/keys.rs`: `Ctrl-N` Add (removed from `Down`), `Ctrl-R` Rename, `Ctrl-X` Delete (the latter two are wired in T048); help overlay rows; update README key table; update the six `help.*` snapshots. Makes T029 pass.
- [ ] T036 [US1] Make `kb add` without `--yes`/`--editor` open the TUI on the prepared draft (`crates/kb/src/author.rs`, `crates/kb/src/commands.rs::run_ui`): needs a terminal on stdout (else exit 2 as in T024), prints the ID on success after the terminal is restored, exit 130 on cancel. Makes T031 pass.
- [ ] T037 [US1] Document the add form, the key table and the `Ctrl-N` change in `README.md`; note that `Ctrl-N` no longer selects the next result.

**Checkpoint**: Quickstart scenario 1 passes; SC-001 asserted in a test. **MVP: Foundation + US2 + US1.**

---

## Phase 5: User Story 3 — Write an entry in my editor with a template (P2)

**Goal**: `kb add --editor` opens a type template and validates it afterwards.

**Independent test**: with a scripted editor appending `SELECT 1;` to the template, `kb add --editor --type sql --path sql/demo` creates `sql/demo` and prints it.

### Tests first (must fail)

- [ ] T038 [US3] Write failing tests for `template(type, opts)` in `crates/kb-core/src/author/render.rs`: for each type the template has every required field, type-specific optional fields as `# key: value` comments, `created`/`updated`, an empty fence (none for `link` and `code`, which show `url:` / `code:` lines), parses far enough to reach the lint step, and a given title, tags and body are already in place.
- [ ] T039 [US3] Write failing tests in `crates/kb/tests/cli_add.rs` (editor flow, scripted editors, scripted tty answers via the pty harness `support/pty.rs`): a valid result is created at the derived or `--path` ID and the ID printed; an unchanged template exits 130 and writes nothing; editor exit ≠ 0 exits 130 and removes the temp file; lint errors are listed in `path:line: error[KBxxx]: message` form and the `[e]dit again  [s]ave anyway  [d]iscard` prompt appears; `e` reopens with the content kept; `s` writes and warns; `d`, EOF and Ctrl-C delete the temp file and exit 130; with a fixture secret `s` is not offered and typing it re-prompts; no terminal exits 2; the temp file is `0600`, inside the system temp dir, never under the KB root, and gone afterwards in every branch.

### Implementation

- [ ] T040 [US3] Implement `template` in `crates/kb-core/src/author/render.rs`. Makes T038 pass.
- [ ] T041 [US3] Implement the editor flow in `crates/kb/src/editfile.rs` (temp file lifecycle with drop guard and the signal path, edit loop, the e/s/d prompt reading from the tty, read-back of `type` and `title` to derive the ID) and call it from `crates/kb/src/author.rs`. Makes T039 pass.
- [ ] T042 [US3] Document `kb add --editor`, the prompt, and the "use `code --wait`" caveat for editors that return immediately in `README.md`.

**Checkpoint**: Quickstart scenario 3 passes.

---

## Phase 6: User Story 4 — Edit, rename and delete entries (P2)

**Goal**: `kb edit <id|query>` with validation, `kb mv`, `kb rm`, and the TUI Ctrl-R / Ctrl-X.

**Independent test**: `kb mv cmd/a cmd/b` keeps the entry's position in an empty-query search (SC-005) and carries its remembered values.

### Tests first (must fail)

- [ ] T043 [P] [US4] Write failing tests in `crates/kb/tests/cli_edit.rs` (scripted editors): an exact ID wins over a query; one query match is opened; several matches exit 1 listing up to 5 IDs and open nothing; no match exits 1; unchanged bytes exit 0, no write, no `updated` change; a clean change bumps only `updated` (every other byte equal, via `set_field`) and refreshes the index; a file without `updated` gets one inserted; errors show the e/s/d prompt (`d` restores the snapshot byte for byte and exits 130, `s` keeps the file and bumps `updated` only if it parses, `e` reopens); secret introduced by the editor: no `s`, `d` restores (SC-004); editor exit ≠ 0 restores a changed file and exits 130; no terminal → errors printed, file left as written, exit 1; a file changed on disk between the post-editor read and the write → `Conflict` message, exit 1; `--json` prints `{"id","status","findings"}` and never prompts.
- [ ] T044 [P] [US4] Write failing tests in `crates/kb/tests/cli_mv_rm.rs`: `mv` renames, creates folders, removes the emptied source folder, carries usage and remembered variable values even when the content was edited first (FR-022); the empty-query search order is identical before and after (SC-005); target exists (also with different case) → exit 1, nothing changed; same ID → exit 0; unknown source → exit 1; bad ID → exit 1; inside a git work tree with the file tracked it runs `git mv` (assert via `git status --porcelain` showing a rename and the output saying `(git mv)`), with an untracked file or no git it does a plain rename (`(rename)`); a failing `git` falls back; `rm` asks `Delete cw/foo? (y/N)` on the tty and only `y` deletes, anything else exits 130; `--yes` skips it; no terminal and no `--yes` exits 2; usage and remembered values removed (FR-023); `git rm` is used for tracked files; `--json` shapes `{"id","from","to","method"}` and `{"id","method"}`; both refresh the index.
- [ ] T045 [P] [US4] Write failing pty tests in `crates/kb/tests/tui_add_pty.rs` (or a new `tui_manage_pty.rs`): `Ctrl-R` shows `Rename to: <id>▌` pre-filled with the cursor at the end, Enter renames and selects the new ID with `Renamed to cw/bar`, an existing target shows the error and keeps the prompt, Esc cancels; `Ctrl-X` shows `Delete cw/foo? (y/N)`, `y` deletes and selects the next row (previous at the end) with `Deleted cw/foo`, anything else cancels; both are no-ops with an empty list; list `Ctrl-E` is unchanged from M3 (no `updated` bump, only the first lint error shown) — assert it explicitly.

### Implementation

- [ ] T046 [US4] Implement `kb edit` in `crates/kb/src/author.rs` (resolution, snapshot, editor, check, `save_edit`, `restore`, e/s/d loop shared with T041, exit codes, `--json`), replace `run_edit` in `crates/kb/src/commands.rs`, change `EditArgs` in `main.rs` to `<id|query>`. Makes T043 pass. Keep the TUI's own `edit_entry` untouched.
- [ ] T047 [US4] Implement `kb mv` and `kb rm` in `crates/kb/src/author.rs` and `main.rs` (git detection via `run_git`: `rev-parse --is-inside-work-tree`, `ls-files --error-unmatch`, then `git mv` / `git rm -q`, fallback to plain operations; messages naming the method; confirmation prompt; exit codes). Makes T044 pass.
- [ ] T048 [US4] Implement the list prompts and effects (`Effect::{RenameEntry, RemoveEntry}`, `Action::{Renamed, Removed, OpFailed}`, key-bar prompt state in `app.rs`, drawing in `view.rs`) using the same functions as T047. Makes T045 pass; update the affected help snapshots.
- [ ] T049 [US4] Document `kb edit`, `kb mv`, `kb rm`, git behavior, the Ctrl-R / Ctrl-X keys and the Ctrl-E difference in `README.md`.

**Checkpoint**: Quickstart scenario 4 passes.

---

## Phase 7: User Story 5 — kb turns literal values into variables (P3)

**Goal**: suggestions listed in the form, accepted with Space, never automatic.

**Independent test**: accepting every suggestion on the Sumo fixture and resolving with defaults reproduces the pasted text exactly (SC-006).

### Tests first (must fail)

- [ ] T050 [P] [US5] Write failing tests for `suggest_vars` and `Draft::accept` in `crates/kb-core/src/author/suggest.rs` over `tests/fixtures/authoring/suggest/`: exactly the FR-015 patterns; `-30m` → `-{{minutes}}m` (digits only), `24h` → hours, `7d` → days, `INTERVAL '1 day'` → days; env as `enum` with options `[dev, qa, stage, prod]` (short) or `[dev, qa, staging, production]` (long) and the original as default (so lint KB045 passes); one suggestion per distinct value with all positions, accepting replaces every occurrence; names unique against declared variables and each other (`env`, `env_2`); nothing inside `{{…}}` or `\{{…}}`; negative cases produce nothing; every suggested value matches `[A-Za-z0-9._/-]+`; SC-006 table test: accept all, resolve with M5's `varres` using defaults, in `cmd` (bare, `"…"`, `'…'`), `sql` and `url` contexts, equals the pasted text; accepted suggestions are removed and the rest keep their positions (re-run).
- [ ] T051 [P] [US5] Write failing tests in `crates/kb/src/tui/add.rs` and a pty case in `tui_add_pty.rs`: suggestion rows appear under Variables showing value, proposed name and kind; Space on a row accepts it (body and declarations change, `changed` set); without Space nothing changes (US5.3); `kb add --yes` never applies any (also in `cli_add.rs`); variable rows: `k` cycles kind, `d` edits the default (refused on secret, with a message), `o` edits options, `l` marks literal; snapshot with suggestions (extends T030).

### Implementation

- [ ] T052 [US5] Implement `suggest_vars`, `VariableSuggestion`, `SuggestPattern` and `Draft::accept` in `crates/kb-core/src/author/suggest.rs` (R4). Makes T050 pass.
- [ ] T053 [US5] Implement suggestion and variable rows in `AddState` / `draw_add` (`crates/kb/src/tui/add.rs`, `add_view.rs`). Makes T051 pass.
- [ ] T054 [US5] Document the five suggestion patterns and "never automatic" in `README.md`.

**Checkpoint**: Quickstart scenario 5 passes.

---

## Phase 8: Polish & cross-cutting

- [ ] T055 [P] Write `crates/kb/tests/authoring_secrets.rs` (SC-004): the fixture secrets from `tests/fixtures/secrets/` pasted through every path — TUI form, `kb add --yes`, `kb add --editor`, `kb edit` — never end up in a file under the KB (scan the whole temp KB afterwards) or in `state.json`; `kb:allow-secret` is honored on the same paths; secrets as `{{variable}}` of kind `secret` are accepted.
- [ ] T056 [P] Extend `crates/kb/tests/perf.rs`: on a 10,000-entry KB, `Draft::from_text` (detect + variables + suggestions) on a 1 MB body ≤ 50 ms, `create_entry` + refresh ≤ 100 ms (release only, like the other budgets); `kb add --yes` cold start ≤ 100 ms.
- [X] T057 [P] Round-trip property check in `crates/kb-core/src/author/render.rs` tests: for every corpus snippet, `Draft::from_text` with a fixed title renders a file that passes `check_text` with no errors, parses, and re-serializes to identical bytes (SC-003 over the whole corpus).
- [ ] T058 Concurrency and cleanup tests in `crates/kb/tests/concurrency.rs`: two `kb add --yes` racing for the same derived ID produce `-2` for one and never overwrite; `kb mv` racing a search keeps the index consistent; SIGTERM during the editor flow leaves no temp file; temp files are never created inside the KB root in any flow.
- [ ] T059 Update `tests/fixtures/README.md`, `CONTRIBUTING.md` (new test layout, corpus rules: a detection change must come with a corpus case) and the `README.md` configuration section for `path_template` and `kb doctor`'s `clipboard-read` check; make sure the "every bound key is documented" test and the config-key docs test pass.
- [ ] T060 Run the full gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and `cargo test --release -p kb --test perf`. Fix anything that fails.
- [ ] T061 **Person task**: walk `quickstart.md` by hand on a real KB with the real clipboard (scenarios 1–5); confirm the screen-4 layout against the design image and adjust `add_view.rs` if it differs.
- [ ] T062 **Person task**: terminal matrix for Ctrl-N / Ctrl-R / Ctrl-X and editor suspend/resume: Terminal.app, iTerm2, tmux, an SSH session (expect: add form opens empty with the "not readable over SSH" line); also `code --wait` as the editor.
- [ ] T063 **Person task**: push the branch and confirm CI is green on macOS and Linux (constitution VIII); the milestone is not done before.

---

## Dependencies & execution order

```text
Phase 1 Setup ─▶ Phase 2 Foundation ─▶ Phase 3 US2 ─▶ Phase 4 US1 (MVP) ─┬▶ Phase 5 US3 ─┐
                                                                         ├▶ Phase 6 US4 ─┼▶ Phase 8 Polish
                                                                         └▶ Phase 7 US5 ─┘
```

- **Foundation internal order**: T014 → T018/T019 (render calls `check_text`); T015, T016, T017 are
  independent of each other; T018 → T019 → T020; T021 → T022 → T023.
- **US2 needs** T017–T022. **US1 needs** US2 (`author.rs`, source selection) and T021 (`EditBody` temp file).
- **US3 needs** US2 and T021. **US4** needs Foundation and the e/s/d loop from US3 (T041) for `kb edit`;
  T048 needs the US1 key plumbing (T035). **US5** core (T052) needs only Foundation; its UI (T053) needs US1.
- Within a story: tests first and seen failing, then implementation, then docs.

## Parallel opportunities

- Foundation tests T005–T013 are all different files: run together. Fixtures T001–T003 are independent.
- After Foundation: US3's T038/T040 (core template) and US5's T050/T052 (core suggestions) can proceed
  while the TUI work of US1 is in progress, since they touch `author/render.rs` and `author/suggest.rs`
  only (T038 and T011 share `render.rs`: do T019 first).
- US4's T043, T044 and T045 tests are in separate files.

## Implementation strategy

1. **MVP = Foundation + US2 + US1.** Capture works from both the shell and the TUI; stop and use it.
2. Add US3, then US4 (the management commands are what keeps the KB tidy once capture is easy).
3. US5 last: pure convenience, and the format does not change when it is added.
4. Build the detection corpus (T001) before any detector code; judge SC-002 on the corpus, not on
   hand-picked examples.
