# Implementation Plan: Authoring

**Branch**: `007-authoring` | **Date**: 2026-10-09 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/007-authoring/spec.md`

## Summary

M6 adds the capture path: read the clipboard, detect the type, find variables, suggest title/path/tags,
and write a valid entry, from the TUI (screen 4) and from `kb add`; plus `kb edit` (grows a
validate-and-recover loop and query lookup), `kb mv` and `kb rm`.

Six decisions carry the design (reasons and rejected alternatives in [research.md](research.md)):

1. **One pure authoring engine, two front ends.** A new `kb-core::author` module owns detection,
   suggestions, the `Draft`, canonical rendering, save-time validation, and the file operations
   (create, rename, remove). The TUI form, `kb add`, the editor-template flow and `kb edit` only collect
   input, so "form == `--yes` == editor template" (SC-003, SC-004) holds because they share `Draft::render`
   and `check`.
2. **Check what will be written, not what was typed.** `Draft::render` produces the exact bytes; the same
   bytes go through `lint::check_text` (the checks `kb lint` and the index run) before anything touches
   the disk. A secret finding blocks every path; there is no override flag.
3. **Rendering is canonical for new files, surgical for existing ones.** New entries are serialized in
   one layout (FR-018) by a small writer in `author/render.rs`; `kb edit` never re-serializes: it bumps
   `updated` with M1's `set_field`.
4. **File operations are core, process spawning is not.** `Kb::create_entry / rename_entry /
   remove_entry` do the file, state and index work; `git mv` / `git rm` and the editor are passed in as
   closures by the `kb` crate (constitution IV and V: spawning stays behind `platform`).
5. **Reading the clipboard is the mirror of copying it.** `Platform::run_read_tool` (macOS `pbpaste`,
   Wayland `wl-paste`, X11 `xclip -o` / `xsel -o`), a `clipboard::read` that returns
   `Text | Empty | Unavailable(reason)`, and a doctor check. No OSC 52 read.
6. **`Ctrl-N` moves from "next result" to "add".** The spec requires it (FR-027); `↓` and `Ctrl-J` keep
   moving down, `Ctrl-P` keeps moving up. Flagged as a user-visible change (R1).

Facts found in the code that shaped the plan:

- **`kb edit <id>` and Ctrl-E already exist** (M3/M4: `run_edit`, `edit_entry`). They open the file in
  the editor and print the first lint error. M6 keeps Ctrl-E in the list exactly as it is and changes
  `kb edit` to take `<id|query>` and run the validate / save-anyway / discard loop.
- **`write_entry` already refuses secrets and writes atomically** (`edit.rs`), but overwrites without
  checking the file changed. FR-020 adds a based-on check and a no-clobber create; `write_entry` is
  reused for the atomic part.
- **`Entry::set_field` is the format-preserving edit** (M1 FR-017): `updated` is set with it, and a
  missing `updated` is inserted before the closing `---`.
- **The usage rename-carry in M2 needs identical content** (`State::carry_renames`). `kb mv` moves the
  state records itself before the index sees the rename, so M2's rule never has to decide (FR-022).
- **`lint::check_file` takes a `WalkEntry`** (id, rel path, abs path); `check_text(id, bytes)` is a
  five-line wrapper that builds one, so there is a single definition of "has errors" (M2 rule kept).
- **The KeyMap has `Ctrl-N` as Down in Search mode and no `Ctrl-R` / `Ctrl-X`.** The duplicate-binding
  test (`has_duplicates`) and the README "every bound key is documented" test cover the change.
- **The M5 var form is held as `AppState.form: Option<Box<FormState>>` and handles its own keys.** The
  add form follows that shape (`AppState.add`), so it needs no new KeyMap mode.
- **No new dependency.** Slugs use `unicode-normalization` (already in `kb-core`); temp files use
  `std` (`create_new` + `mode(0o600)`, in `platform/unix.rs`); dates are UTC civil-from-days (~15 lines).

## Technical Context

**Language/Version**: Rust 2024, MSRV 1.88, `unsafe_code = forbid` (unchanged).

**Primary Dependencies**: existing `ratatui`, `crossterm`, `syntect`, `clap`, `regex`,
`unicode-normalization`, `serde_json`, `toml_edit`. **No new dependency.**

**Storage**: entry files (new, renamed, removed) and `.kb/state.json` (records moved or dropped). The
index is refreshed through the existing `Kb::refresh`. Nothing else is written to the KB root; temp
files live in the system temp directory.

**Testing**: `cargo test`. Red-first unit tests in `kb-core` for detection (fixture corpus, SC-002),
suggestions, rendering (byte-for-byte), `check`, create/rename/remove; `FakePlatform` for clipboard read
and editor flows; the pty harness for the add form and Ctrl-N/R/X; `secrets_scan`-style test for SC-004;
`perf.rs` unchanged budgets.

**Target Platform**: macOS (arm64, x86_64) and Linux (x86_64, Wayland and X11), unchanged.

**Project Type**: Cargo workspace, `kb-core` + `kb`.

**Performance Goals**: detection and suggestion on a 1 MB body in ≤ 50 ms (one pass per rule, no
backtracking regexes on unbounded input); the form runs both when it opens and after each Ctrl-E (the body is
read-only in the form). Save to "selected in the list" ≤ 100 ms on
a 10,000-entry KB (one `refresh`, which is already incremental).

**Constraints**: nothing is written that `kb lint` would reject with an error (except the explicit
"save anyway"); secrets are never written (VI); the editor and git are only started from `platform`
(V); the clipboard read times out at 2 s (FR-001).

**Scale/Scope**: 1 new core module directory (`author/`, 6 files) + small edits to `lint`, `usage`,
`search`, `config`, `entry_json`; 1 platform method + 1 clipboard function + 1 doctor check; 3 new
subcommands (`add`, `mv`, `rm`), 1 changed (`edit`); 1 new TUI screen (add form) + 2 list prompts.

## Constitution Check

*GATE: pass before research; re-checked after design (second column).*

| Principle | Before | After design | Notes |
|---|---|---|---|
| I. Plain files | Pass | Pass | Only entry files are created, moved, deleted. Output is a file a human could have written; `kb_format: 1`, canonical key order. |
| II. Index is disposable | Pass | Pass | Index only refreshed. Usage and remembered values live in `state.json` and are moved/dropped there. |
| III. Local-only | Pass | Pass | `git mv` / `git rm` are local git operations on the user's work tree; no network, no fetch. See Complexity Tracking. |
| IV. Terminal only, one core | Pass | Pass | All logic in `kb-core::author`. `add`, `mv`, `rm`, `edit` support `--json`. **Exception** (Complexity Tracking): per-variable settings and suggestion acceptance exist only in the form; everything else has a flag (`--title --type --tag --path --yes`, `--editor`). |
| V. Portable | Pass | Pass | Clipboard read, editor and git spawning sit in `platform`; a boundary test (like M4's) forbids `Command` elsewhere. SSH falls back, no OSC 52 read. |
| VI. Secrets never stored | Pass | Pass | `check` runs the secret scan on the final bytes on every path; secret findings have no override; `secret` variables never get a default (FR-013). |
| VII. Keyboard-first and fast | Pass | Pass | Form is keyboard-only; SC-001 counted in a pty test. Budgets in Technical Context. |
| VIII. Test-first core | Pass | Pass, one open gate | Detection corpus, suggestions, render, check, create/rename/remove: failing tests first. TUI form and wiring after, as in M3–M5. "CI green on macOS and Linux" needs your push (task gate, as in M4/M5). |
| IX. Simplicity first | Pass | Pass | No dependency, no plugin, no template engine: `path_template` is three placeholders. Detection is rules, not a model. |

**Gate result**: no violations.

## Project Structure

### Documentation (this feature)

```text
specs/007-authoring/
├── plan.md
├── research.md          # Decisions R1–R11
├── data-model.md        # Draft, Detection, VariableSuggestion, Version, ReadOutcome
├── quickstart.md        # Runnable validation scenarios
├── contracts/
│   ├── core-api.md      # kb-core::author: detect, suggest, Draft, render, check, file operations
│   ├── cli.md           # add, edit, mv, rm: arguments, flows, prompts, output, exit codes
│   ├── tui-add-form.md  # screen 4: layout, keys, effects, list prompts (Ctrl-N/R/X)
│   └── platform-and-config.md  # clipboard read, doctor check, path_template, state moves
├── checklists/requirements.md
└── tasks.md             # /speckit-tasks output (not created here)
```

### Source Code (repository root)

```text
crates/
├── kb-core/src/
│   ├── author/                  # NEW module directory
│   │   ├── mod.rs               # re-exports; Draft, DraftSource, Version, Clock
│   │   ├── detect.rs            # detect(), strip_fence(), strip_prompts(), Detection, Confidence
│   │   ├── suggest.rs           # suggest_vars(), accept(), VariableSuggestion
│   │   ├── draft.rs             # Draft: fields, set_type, body edits, literal marks, defaults (title, path, kinds)
│   │   ├── render.rs            # Draft::render (canonical layout), template(type) for --editor
│   │   └── fsops.rs             # create_entry, rename_entry, remove_entry, save_edit (no spawning)
│   ├── lint/mod.rs              # CHANGE: check_text(id, rel, bytes) -> Vec<Finding>
│   ├── usage.rs                 # CHANGE: State::move_entry(old, new), State::forget(id)
│   ├── search.rs                # CHANGE: Kb::create_entry / rename_entry / remove_entry / tag_counts
│   ├── config.rs                # CHANGE: SETTINGS gains `path_template`; Settings::path_template()
│   └── entry_json.rs            # unchanged (add --json reuses it)
└── kb/src/
    ├── author.rs                # NEW: `kb add`, `kb mv`, `kb rm`, `kb edit` loop, prompts (e/s/d, y/N)
    ├── editfile.rs              # NEW: temp file lifecycle (0600, system temp, drop guard), template flow
    ├── commands.rs              # CHANGE: run_edit delegates to author::edit
    ├── main.rs                  # CHANGE: Add, Mv, Rm subcommands; Edit takes <id|query>
    ├── doctor.rs                # CHANGE: `clipboard-read` check
    ├── platform/
    │   ├── mod.rs               # CHANGE: Platform::run_read_tool, Platform::run_git, temp-file helper
    │   ├── clipboard.rs         # CHANGE: PBPASTE/WL_PASTE/XCLIP_OUT/XSEL_OUT, read(), ReadOutcome
    │   ├── unix.rs              # CHANGE: run_read_tool (capture stdout, 2 s), run_git, private_temp_file
    │   └── fake.rs              # CHANGE: scripted reads, git calls and temp files
    └── tui/
        ├── add.rs               # NEW: AddState (pure): fields, update(), validation, suggestions
        ├── add_view.rs          # NEW: draw_add (wide and narrow), body highlight, secret highlight
        ├── app.rs               # CHANGE: AppState.add, list prompts (Rename, Delete), Effects
        ├── keys.rs              # CHANGE: Ctrl-N add (leaves Down), Ctrl-R rename, Ctrl-X delete
        ├── view.rs              # CHANGE: key bar prompts, add screen dispatch
        └── mod.rs               # CHANGE: effects ReadClipboard, EditBody, SaveDraft, Rename, Remove
crates/kb/tests/
├── cli_add.rs  cli_edit.rs  cli_mv_rm.rs   # NEW
├── tui_add_pty.rs                          # NEW (screen 4 end to end, SC-001 key count)
├── authoring_secrets.rs                    # NEW (SC-004: every save path)
└── platform_boundary.rs                    # extend: no `Command`/`std::fs::OpenOptions` temp files outside platform
tests/fixtures/authoring/                   # NEW
├── detect/<type>/NN-name.txt               # ≥ 10 per type, ≥ 100 total (SC-002); low/ for ambiguous
├── suggest/                                # pasted snippet + expected accepted result
└── golden/                                 # expected files for the byte-for-byte tests
```

**Structure Decision**: authoring logic in `kb-core` (constitution IV); input, rendering, editors, git
and the terminal in `kb`. `author/` is a directory because detection, suggestions and rendering are
separately testable; it is not split further.

## Complexity Tracking

| Item | Why needed | Simpler alternative rejected because |
|---|---|---|
| Spawning `git` (`mv`, `rm`, `ls-files`) from the `kb` crate | FR-024: history is kept for renames and deletes | Plain `rename`/`unlink` shows in git as delete + add; fine for content but loses nothing technically. Spec asks for `git mv`; it is local, behind `platform`, with a plain fallback. Not "sync" in the sense of principle III. |
| Moving `Ctrl-N` off "next result" | FR-027 says Ctrl-N adds | Using another key breaks the spec text and SC-001's key count; `↓`, `Ctrl-J` and `Ctrl-P` remain. Recorded in R1 and the README. |
| Form-only actions (set variable kind/default/options, mark literal, accept suggestion) | Spec clarification Q4 keeps suggestions interactive; per-variable editing is a form convenience | A flag per action (`--var`, `--literal`, `--accept`) adds 3+ flags and a mini-language. The same result is reachable by `kb add --editor`, or by writing `{{x}}` and a `vars:` block, which `kb add --yes` declares as `text`. Applies to this milestone only (constitution: exceptions are per milestone). |
| Two renderers for entries (M1's byte-preserving editor, and the new canonical writer) | New files have no original text to preserve; existing files must not be re-serialized (FR-018, FR-019) | One writer for both would re-serialize hand-written files, which M1 forbids. The canonical writer is ~120 lines and is round-trip tested against the parser. |

## Behavior changes and documentation

- **`kb edit` takes `<id|query>`** and, when the file is invalid after the editor closes, asks
  e / s / d instead of just printing the first error. Exit code is 0 after a valid or accepted save,
  1 when the user discards or the file stays invalid in a non-terminal run.
- **`Ctrl-N` no longer selects the next result** in the list. README key table and the help overlay
  (six `help.*` snapshots) are updated; the "every bound key is documented" test covers the three new keys.
- **`kb edit --json`** changes from an error object to `{id,status,findings}`.
- **New setting `path_template`** (twelfth setting): `config init` template, `config list`, snapshots.
- **README**: new sections for `add`, `mv`, `rm`, `edit`, the add form, clipboard reading and the
  suggestion patterns; same change as the code (constitution).
- **`kb doctor`**: one more check, `clipboard-read`.

## Post-design re-check

Re-read after the data model and contracts: the table above stands. Risks that are real:

- **Detection accuracy (SC-002) is an empirical target.** Rules are deterministic, but the corpus
  decides if 95% holds. The corpus is built first (task gate); rule changes are driven by misses.
  Rule 3 vs 4 ordering is refined in R2 (a `@field` blocks the Sumo-operator route) to keep CloudWatch
  `| parse @message …` queries from being called Sumo.
- **Hand editors differ on temp files.** Some editors need a file extension to highlight and some
  (`code`, `subl`) return immediately unless told to wait; `kb edit` cannot detect that. Documented in
  the README ("use `code --wait`"); an editor that returns at once reads as "unchanged" and cancels.
- **`git mv` on a file with local changes or untracked state.** Handled by checking
  `git ls-files --error-unmatch` first and falling back to plain rename; the failure of `git` after that
  check also falls back, and `kb mv` says which it used.
- **`updated` as a datetime in UTC.** It changes on every save (FR-009) even twice on one day, but two
  saves in the same second keep one value. Accepted; nothing orders by it.
- **Constitution IV exception** is recorded in Complexity Tracking; revisit in M7 if scripting variable settings is wanted.
