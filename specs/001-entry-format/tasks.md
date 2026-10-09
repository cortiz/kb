---
description: "Task list for M1: Entry Format and Parser"
---

# Tasks: Entry Format and Parser

**Input**: Design documents from `specs/001-entry-format/`
**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md),
[data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: Required. Constitution VIII makes red-green-refactor mandatory for `kb-core`: every
test task must be written and seen failing (`cargo test` red) before its implementation task
starts. Unit tests live in `#[cfg(test)] mod tests` at the bottom of the file they test.

**Organization**: Tasks are grouped by user story so each story can be implemented and tested on
its own.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependency on an incomplete task)
- **[Story]**: The user story the task belongs to (US1, US2, US3)

## Path Conventions

Cargo workspace from [plan.md](plan.md): core library in `crates/kb-core/src/`, binary in
`crates/kb/src/`, shared fixtures and core integration tests in `tests/` at the repository root,
CLI tests in `crates/kb/tests/`. Rule codes refer to
[contracts/lint-rules.md](contracts/lint-rules.md).

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Workspace, toolchain, CI and fixture layout.

- [X] T001 Create the workspace manifest `Cargo.toml` at the repository root: `members = ["crates/kb-core", "crates/kb"]`, `resolver = "3"`, `[workspace.package]` with `edition = "2024"` and `rust-version = "1.88"`, `[workspace.lints.rust] unsafe_code = "forbid"`, and `[workspace.dependencies]` pinning `saphyr-parser = "0.1"`, `pulldown-cmark = { version = "0.13", default-features = false }`, `regex = "1"`, `serde = { version = "1", features = ["derive"] }`, `serde_json = "1"`, `walkdir = "2"`, `clap = { version = "4", features = ["derive"] }`, `assert_cmd = "2"`, `tempfile = "3"`. No other dependencies (constitution IX).
- [X] T002 [P] Create `rust-toolchain.toml` with `channel = "stable"` and `components = ["rustfmt", "clippy"]`.
- [X] T003 [P] Create `crates/kb-core/Cargo.toml` (library `kb_core`, workspace deps `saphyr-parser`, `pulldown-cmark`, `regex`, `serde`, `serde_json`, `walkdir`; dev-dep `tempfile`; `[lints] workspace = true`) and `crates/kb-core/src/lib.rs` declaring the modules from plan.md (`source`, `frontmatter`, `yaml`, `body`, `entry`, `vars`, `edit`, `walk`, `root`, `init`, `lint`) as empty files so the crate builds.
- [X] T004 [P] Create `crates/kb/Cargo.toml` (`[[bin]] name = "kb"`, deps `kb-core` by path, `clap`, `serde_json`; dev-deps `assert_cmd`, `tempfile`; `[lints] workspace = true`) and a stub `crates/kb/src/main.rs` where `kb --version` and `kb --help` work.
- [X] T005 [P] Create `.github/workflows/ci.yml` per research R12: jobs on `ubuntu-latest` and `macos-latest` running `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`; on macOS also `rustup target add x86_64-apple-darwin && cargo build --workspace --target x86_64-apple-darwin`; a separate Linux job running `cargo test --workspace` with toolchain `1.88`.
- [X] T006 [P] Create `tests/fixtures/README.md` describing the fixture layout (`valid/`, `invalid/`, `secrets/`) and the `.expect` sidecar format: one finding per line as `<line>:<column> <CODE>` (for example `3:7 KB001-unknown-type`), sorted by line, then column, then code. An empty `.expect` file means "no findings". Note that kb ignores non-`.md` files, so sidecars never become entries.

**Checkpoint**: `cargo build --workspace` and `cargo clippy --workspace -- -D warnings` pass with empty modules.

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Source handling, frontmatter splitting, the spanned YAML tree, finding types, root
resolution, the platform layer and the CLI skeleton. Every story depends on these.

**⚠️ CRITICAL**: No user story work can begin until this phase is complete.

### Tests first (must fail before the matching implementation)

- [X] T007 [P] Write unit tests in `crates/kb-core/src/source.rs` for `Source::from_bytes`: UTF-8 BOM is detected, stripped from `text` and restored by `to_bytes`; line endings classify as `Lf`, `CrLf` or `Mixed`; `line_starts` is correct for LF and CRLF; byte offset → (line, column) with columns counting Unicode characters and both 1-based (check `"héllo"`); char index → byte offset conversion for non-ASCII text, and identity for ASCII without building the table; invalid UTF-8 returns an error carrying the first bad byte offset.
- [X] T008 [P] Write unit tests in `crates/kb-core/src/frontmatter.rs` for `split`: first line (after BOM) not `---` → KB002 at 1:1; opening `---` without a closing `---` line → KB003 at 1:1; empty file or only whitespace → KB004; trailing spaces or tabs after either `---` accepted; CRLF delimiters accepted; empty body accepted; frontmatter and body spans exact in bytes.
- [X] T009 [P] Write unit tests in `crates/kb-core/src/yaml.rs` for `parse_mapping`: for plain, single-quoted, double-quoted, literal, folded and flow-collection scalars, `&source[node.span]` equals exactly the scalar's source text, including with non-ASCII before the value; nested block and flow maps and sequences; keys carry their own spans; anchor, alias or explicit tag → KB008; duplicate key → KB009 at the second occurrence; root that is not a mapping → KB007; YAML syntax error → KB006 with the parser's line and column.
- [X] T010 [P] Write unit tests in `crates/kb-core/src/lint/mod.rs` for the finding model: `RuleCode::UnknownType` displays as `KB001-unknown-type`, and every code in contracts/lint-rules.md round-trips through its display form; `Severity` serializes as `"error"`, `"warning"` and `"info"`; findings sort by path, line, column, then code.
- [X] T011 [P] Write unit tests in `crates/kb-core/src/lint/suggest.rs`: `suggest("sumologic", TYPES) == Some("sumo")` (prefix match wins), `suggest("cdm", TYPES) == Some("cmd")`, `suggest("tilte", KEYS) == Some("title")`, `suggest("zzzz", TYPES) == None` (Levenshtein distance must be ≤ 2).
- [X] T012 [P] Write unit tests in `crates/kb-core/src/root.rs` for `resolve_root`: the order is "`--root` flag, the `KB_ROOT` environment variable, the config file, then `~/kb`" (FR-022); a config `root: ~/notes` expands `~/` to `home`; a missing config file is fine; a config file that exists but can't be read or parsed is a `RootError` naming the file; unknown config keys are ignored; no `home` and nothing else set → `RootError`.

### Implementation

- [X] T013 Implement the finding model in `crates/kb-core/src/lint/mod.rs`: `Severity`, `RuleCode` with one variant for every code in contracts/lint-rules.md (number + slug, `Display` and `Serialize` as `KB001-unknown-type`), its default severity, `Finding { path, id, line, column, severity, code, message, fix }` and `Suppression { path, line, code }` as in data-model.md. Makes T010 pass.
- [X] T014 [P] Implement `Source` in `crates/kb-core/src/source.rs` (fields `text`, `bom`, `line_ending`, `line_starts`, lazily built `char_to_byte`; `to_bytes()`; `position(byte) -> (line, column)`). Makes T007 pass.
- [X] T015 Implement `split` in `crates/kb-core/src/frontmatter.rs` returning frontmatter and body spans or a KB002/KB003/KB004 finding. Depends on T014. Makes T008 pass.
- [X] T016 Implement `crates/kb-core/src/yaml.rs`: drive `saphyr_parser::Parser::new_from_str` over the frontmatter text, convert the parser's character offsets to byte offsets through `Source` (research R2), and build `Node::{Scalar{value, style, span}, Seq{items, flow, span}, Map{entries, flow, span}}` with key spans. Reject anchors, aliases and tags (KB008) and duplicate keys (KB009). Depends on T014, T015. Makes T009 pass.
- [X] T017 [P] Implement `suggest` (prefix match first, then Levenshtein ≤ 2, about 15 lines, no dependency) in `crates/kb-core/src/lint/suggest.rs`. Makes T011 pass.
- [X] T018 Implement `RootInputs` and `resolve_root` in `crates/kb-core/src/root.rs`, reading the config's `root` key through `yaml.rs`. Core never reads env vars or OS directories itself: all inputs come in `RootInputs`. Depends on T016. Makes T012 pass.
- [X] T019 [P] Implement the platform layer in `crates/kb/src/platform/mod.rs` (`trait Platform { fn home_dir(&self) -> Option<PathBuf>; fn config_file(&self) -> Option<PathBuf>; }` and `fn current() -> impl Platform`), `crates/kb/src/platform/linux.rs` (`$XDG_CONFIG_HOME/kb/config.yml`, falling back to `~/.config/kb/config.yml`) and `crates/kb/src/platform/macos.rs` (`~/Library/Application Support/kb/config.yml`), selected with `#[cfg(target_os = ...)]`; home from `std::env::home_dir()`.
- [X] T020 Build the CLI skeleton in `crates/kb/src/main.rs` and `crates/kb/src/output.rs`: clap derive with global `--root <path>`, subcommands `init [path] [--no-examples] [--json]` and `lint [paths...] [--strict] [--quiet] [--json]` exactly as in contracts/cli.md; root resolution via `kb_core::resolve_root` with `KB_ROOT` and `Platform`; exit codes `0`/`1`/`2`; usage, config and I/O errors print `kb: <message>` to stderr and, with `--json`, `{"error": {"kind": ..., "message": ...}}` to stdout; no color when stdout is not a terminal or `NO_COLOR` is set. Subcommands may return "not implemented" until their story lands. Depends on T018, T019.

**Checkpoint**: `cargo test -p kb-core` is green for Phase 2 tests, and `kb --root /tmp lint` reaches the lint stub.

---

## Phase 3: User Story 1 - Write an entry by hand and know it is valid (Priority: P1) 🎯 MVP

**Goal**: `kb lint` checks a KB (or chosen files) and reports every problem with file, line,
column, rule code and suggested fix, with the right exit code.

**Independent Test**: lint `tests/fixtures/` (one valid and one invalid entry per type at least)
and check every finding and the exit code. That's T024 plus the CLI scenarios in T029.

### Fixtures and tests for User Story 1 (write first, confirm they fail)

- [X] T021 [P] [US1] Create valid fixtures in `tests/fixtures/valid/`, each producing **no findings at all** (not even info): `sql/active-connections.md` (`dialect: postgres`), `sumo/5xx-by-authorizer.md` (the spec's Sumo example verbatim), `cw/lambda-errors.md` (`log_groups` with a `{{env}}` reference, `region`), `cmd/tail-logs.md` (`shell: bash`, `cwd`), `prompt/summarize-incident.md` (a ```` ```` ```` fence containing ```` ``` ```` blocks), `link/runbook.md` (empty body), `code/authorizer-handler.md` (the spec's code example verbatim), `format/crlf.md` (CRLF line endings), `format/bom.md` (UTF-8 BOM), `format/comments-and-flow.md` (YAML comments, flow maps, single and double quotes), `format/escaped-braces.md` (payload with `\{{not_a_var}}`), `format/tilde-fence.md` (`~~~` payload) and `format/dates.md` (`created: 2026-10-08`, `updated: 2026-10-08T14:30:00Z`).
- [X] T022 [P] [US1] Create invalid fixtures in `tests/fixtures/invalid/`, one `.md` file per rule KB001–KB051 (except KB052 and KB053, see T028) named after the slug (for example `unknown-type.md` with `type: sumologic`), each with a `.expect` sidecar listing exactly its findings. Fixtures that can't be committed as text are generated at test time inside `tests/lint_fixtures.rs` instead: invalid UTF-8 (KB005, expected message names the first bad byte offset), a file larger than 1,048,576 bytes (KB051) and mixed LF/CRLF (KB050). Include `unsupported-kb-format.md` with `kb_format: 2` (KB012), and both `type`-specific errors from data-model.md for `code` (`line: 0`, `end_line` < `line`, `path: ../x`).
- [X] T023 [P] [US1] Create secret fixtures in `tests/fixtures/secrets/`: one positive file per KB060–KB065 with `.expect` (use public documentation sample values such as AWS's `AKIAIOSFODNN7EXAMPLE` and `wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY`, and build the JWT and private-key header from obviously fake parts); `clean.md` that must produce **no** secret finding (`{{token}}`, `$TOKEN`, `${API_KEY}`, `Bearer {{token}}`, `password: changeme_please`, a 40-char git SHA in a SQL `WHERE`); `suppressed.md` with a bearer token on a line ending in `-- kb:allow-secret` and another ending in `# kb:allow-secret` (expected: no errors, 2 suppressions).
- [X] T024 [US1] Write `tests/lint_fixtures.rs` (registered in `crates/kb-core/Cargo.toml` as `[[test]] name = "lint_fixtures", path = "../../tests/lint_fixtures.rs"`): every file in `valid/` lints with zero findings; for every `.md` in `invalid/` and `secrets/`, `kb_core::lint(root, &[file])` yields exactly the `line:col CODE` set in its `.expect`; generated fixtures from T022 are written to a `tempfile` KB and checked the same way; `secrets/suppressed.md` reports 2 suppressions. Depends on T021–T023.
- [X] T025 [P] [US1] Write unit tests in `crates/kb-core/src/entry.rs` for field rules: missing `kb_format` (KB010), `kb_format: "1"` or `0` (KB011), `kb_format: 2` (KB012), missing `type` (KB013), `type: sumologic` → KB001 whose message lists `sql, sumo, cw, cmd, prompt, link, code` and whose fix suggests `sumo`, missing `title` (KB014), title that is blank or "at most 120 characters" exceeded (KB015), `tags: lambda` (KB018), tags `["#Auth", "auth"]` normalize to `["auth"]` with KB021, a non-string tag (KB020), `created: 2026-02-30` (KB022), `url` on a `sql` entry (KB017), `link` without `url` (KB030), `url: ftp://x` (KB031), `code` entry without the `code` map (KB032), each KB033 sub-case, `shell: pwsh` (KB034), payload type with no fence (KB035) and an empty fence (KB036).
- [X] T026 [P] [US1] Write unit tests in `crates/kb-core/src/vars.rs`: `{{name}}` is a reference only when the name matches `[a-z][a-z0-9_]*`; `\{{x}}` is a literal and appears as `{{x}}` in the parsed text; `{{ Name }}` and `{{a-b}}` give KB049 (info) and stay literal; references are found in the payload and in `range`, `url`, `cwd` and every `log_groups` item; undeclared reference → `kind: text`, no default, KB047 (info); declared but unused → KB048 (warning); KB040 for `vars: { Env: … }`; KB041 missing `kind`; KB042 `kind: string` (fix suggests `text`); KB043 `default` on `secret`; KB044 `enum` without `options` or with `options: []`; KB045 enum `default` not in `options`; KB046 `number` default `"abc"`.
- [X] T027 [P] [US1] Write unit tests in `crates/kb-core/src/lint/secrets.rs` for `scan_secrets`: each pattern from research R4 matches its sample and not near-misses; the high-entropy rule needs a value of at least 12 characters with entropy ≥ 3.5 bits per character and ignores values containing `{`, `}` or `$`; messages contain at most the first 4 characters of the match followed by `…` (assert the full secret never appears in `message` or `fix`); a line ending in `kb:allow-secret` after optional whitespace becomes a `Suppression`, not a finding; the private-key rule reports the header line.
- [X] T028 [P] [US1] Write unit tests in `crates/kb-core/src/walk.rs` using `tempfile`: only `*.md` files are returned; any path component below the root starting with `.` is skipped (`.git/`, `.kb/`, `notes/.draft.md`), while a root that is itself named `.kb-data` still works; IDs use `/` and drop `.md`; output is sorted; a symlink to a file or directory inside the root is followed; a symlink to outside the root gives KB052 (warning) and is skipped; a symlink loop terminates; `SQL/a.md` and `sql/a.md` (create only where the file system allows it, otherwise test the grouping function directly) give KB053 on each file.
- [X] T029 [P] [US1] Write `crates/kb/tests/cli_lint.rs` with `assert_cmd` against a `tempfile` copy of `tests/fixtures/valid/`: (1) clean KB prints exactly `13 entries, 0 errors, 0 warnings` and exits 0; (2) adding `type: sumologic` prints `sumo/bad.md:3:7: error[KB001-unknown-type]: …` followed by `  fix: use "sumo"` and exits 1; (3) a warning-only KB exits 0, and 1 with `--strict`; (4) `kb lint sumo/5xx-by-authorizer.md` run from an unrelated working directory checks only that file (`1 entry, …`); `--quiet` prints only the summary line; a missing root exits 2 with `kb: ` on stderr; singular and plural forms; `, N info` and `, N suppressed` appear only when non-zero; linting `secrets/suppressed.md` prints two `suppressed[KB062-secret-bearer-token]` lines and the summary ends with `, 2 suppressed`, and `--quiet` omits those lines.

- [X] T030 [P] [US1] Write unit tests in `crates/kb-core/src/body.rs`: the first fenced block is the payload when there are several; a ```` ```` ```` fence containing ```` ``` ```` blocks is one payload; `~~~` fences work; the info string (`sql`) goes to `info`, not `text`; `\{{x}}` appears as `{{x}}` in `text` while `raw_span` still covers the backslash; `notes` equal the body with `fence_span` removed (text before and after kept); a fence inside a block quote or list is still found; an indented (4-space) code block is not a payload; for `link` and `code` the whole body is notes and `payload` is `None`; `fence_span` and `raw_span` slice the source exactly.
### Implementation for User Story 1

- [X] T031 [US1] Implement `crates/kb-core/src/body.rs`: find the first `Start(CodeBlock(Fenced(info)))` with `pulldown_cmark::Parser::new(body).into_offset_iter()` and build `Payload { text, raw_span, fence_span, info }`, where `text` has `\{{` turned into `{{`; `notes` is the body with `fence_span` removed. For `link` and `code` the whole body is notes. Makes T030 pass.
- [X] T032 [US1] Implement `crates/kb-core/src/entry.rs`: `Entry`, `EntryType` (7 variants, payload yes for `sql`, `sumo`, `cw`, `cmd`, `prompt`), `TypeFields`, `CodeRef`, and `parse_entry(id, path, bytes) -> ParseOutcome` applying KB001, KB010–KB018, KB020–KB022 (dates per research R9: `YYYY-MM-DD` or RFC 3339 date-time, with day-of-month and leap-year checks), and KB030–KB036. Quote the data-model constraints exactly: title "non-blank, ≤ 120 characters"; `code.repo` "`owner/name`, or a URL starting with `https://`, `ssh://`, `git@` or `file://`"; `code.path` "relative, no `..` component"; `code.line` "≥ 1"; `code.end_line` "≥ `line`"; `shell` "`sh` (default), `bash`, `zsh`, `fish`". `Entry` is returned whenever the frontmatter parses as a mapping. Unknown top-level keys are kept in `unknown_keys` and reported as KB016 with a suggestion. Depends on T016, T017, T031. Makes T025 pass.
- [X] T033 [US1] Implement `crates/kb-core/src/vars.rs`: `Variable`, `VarKind`, `VarRef`, definition validation (KB040–KB046), the reference scanner (`\{{` literal, KB049 for invalid `{{…}}` on one line) over the payload and `range`, `url`, `cwd` and `log_groups[i]`, and KB047 and KB048. Called from `parse_entry`. Depends on T032. Makes T026 pass.
- [X] T034 [P] [US1] Implement `crates/kb-core/src/lint/secrets.rs`: the six patterns from research R4 compiled once with `std::sync::LazyLock<regex::Regex>`, Shannon entropy, redaction (first 4 characters plus `…`), `kb:allow-secret` suppression, and `pub fn scan_secrets(text: &str) -> SecretScan`, run over the whole file (frontmatter and body). Every finding carries the fix "use a `secret` variable with `env`" (KB064: "remove the key from the entry"). Makes T027 pass.
- [X] T035 [P] [US1] Implement `crates/kb-core/src/walk.rs` with `walkdir` (`follow_links(false)`, `sort_by_file_name`): hidden-component filter below the root, manual symlink following inside the canonical root with a visited set of canonical directories, KB052, ID derivation, and KB053 grouping by lowercase ID. Makes T028 pass.
- [X] T036 [US1] Implement file-level rules in `crates/kb-core/src/lint/rules.rs`: KB050 (mixed line endings) and KB051 (more than 1,048,576 bytes; the file is still parsed). Depends on T014.
- [X] T037 [US1] Implement `lint(root, scope)` and `LintReport` in `crates/kb-core/src/lint/mod.rs`: resolve scope paths per research R10 (absolute as given; relative tried against the current directory, then the root; missing or outside the root → error); walk; read each file; run `parse_entry`, `rules`, `scan_secrets`; collision check among scanned files; sort findings; counts; `exit_code(strict)` returns 1 if there is any error, or any warning under strict, else 0; `summary_line()` as `<n> entries, <e> errors, <w> warnings`, appending `, <i> info` and `, <s> suppressed` only when non-zero, singular for 1. Depends on T032–T036. Makes T024 pass.
- [X] T038 [US1] Wire `kb lint` in `crates/kb/src/main.rs` and `crates/kb/src/output.rs`: human output `path:line:col: severity[CODE]: message`, then `  fix: …` when present, then one `path:line: suppressed[CODE]` line per suppression, a blank line, then the summary; `--quiet` prints only the summary; exit with `report.exit_code(strict)`. Depends on T020, T037. Makes T029 pass.
- [X] T039 [US1] Document the entry format (from contracts/entry-format.md), `kb lint` usage, exit codes and the rule list (link to the codes) in `README.md` (constitution: user-visible behavior is documented in the same change).

**Checkpoint**: `kb lint` is a usable validator for hand-written entries (MVP). `cargo test --workspace` is green.

---

## Phase 4: User Story 2 - Start a new KB (Priority: P1)

**Goal**: `kb init` creates or completes a KB without ever writing to the KB root except example
entries and `.kb/`.

**Independent Test**: run `kb init` in an empty temporary directory and check the files (T042).
Checking that the examples pass lint uses the US1 linter, so build US1 first, or check them with
`parse_entry` until it lands.

### Tests for User Story 2 (write first, confirm they fail)

- [X] T040 [P] [US2] Write unit tests in `crates/kb-core/src/init.rs` with `tempfile`: `plan_init` on a nonexistent path creates the root, `.kb/`, `.kb/.gitignore` and 7 examples; on an empty directory, the same; on a directory that already holds entries, only missing `.kb/` and `.kb/.gitignore`, with `examples = SkippedHasEntries`; with `examples = false`, `SkippedFlag`; a path that is a file → `InitError`; an existing `.kb/.gitignore` with other content is left byte-identical; `apply_init` never creates `<root>/.gitignore`; a file created between plan and apply is not overwritten (exclusive create).
- [X] T041 [P] [US2] Write a test in `crates/kb-core/src/init.rs` that every embedded example lints with **zero findings of any severity** (so `kb lint --strict` passes and no info appears) and that there is exactly one example per `EntryType`.
- [X] T042 [P] [US2] Write `crates/kb/tests/cli_init.rs`: scenario 1 (`kb init <tmp>/kb` creates `.kb/`, `.kb/.gitignore` containing exactly `*\n`, and the 7 files named in contracts/cli.md, and `kb --root <tmp>/kb lint --strict` exits 0 with `7 entries, 0 errors, 0 warnings`); scenario 2 (KB with an existing entry and root `.gitignore`: only `.kb/` items are created, every pre-existing file is byte-identical afterward, output lists `exists` lines, and a second run prints `KB at … is up to date`); scenario 3 (`--no-examples`); `--json` has exactly the keys in contracts/init-report.schema.json; the hint appears when the path differs from the resolved root; a path that is a file exits 2.

### Implementation for User Story 2

- [X] T043 [P] [US2] Write the 7 example entries in `crates/kb-core/src/examples/`: `sql/example-active-connections.md`, `sumo/example-5xx-by-authorizer.md`, `cw/example-lambda-errors.md`, `cmd/example-tail-pod-logs.md`, `prompt/example-summarize-incident.md`, `link/example-runbook.md`, `code/example-request-handler.md`. Each uses its type's fields and short notes that teach the format (SC-004). Every declared variable is referenced and there are no undeclared references. No real company names, URLs or repos: use `example.com` and `example-org/example-service`.
- [X] T044 [US2] Implement `plan_init` and `apply_init` in `crates/kb-core/src/init.rs` per data-model.md (`InitPlan`, `InitAction`, `InitReport`): `create_dir_all` for the root, `.kb/`, `.kb/.gitignore` with content `*\n`, examples embedded with `include_str!`, and every file created with `OpenOptions::new().write(true).create_new(true)`. "Has entries" means the walk from T035 finds at least one `.md`. Depends on T035, T043. Makes T040 and T041 pass.
- [X] T045 [US2] Wire `kb init` in `crates/kb/src/main.rs` and `crates/kb/src/output.rs`: positional `path`, otherwise the resolved root; `--no-examples`; human output per contracts/cli.md (`Initialized KB at …`, `created` and `exists` lines, `up to date`, hint naming `KB_ROOT` and the config `root` when the path differs from the resolved root); `--json` per contracts/init-report.schema.json; exit 2 on any `InitError`. Depends on T020, T044. Makes T042 pass.
- [X] T046 [US2] Add a "Getting started" section with `kb init`, the KB layout and the `.kb/` directory to `README.md`.

**Checkpoint**: a new user can `kb init`, copy an example and lint it clean (SC-004).

---

## Phase 5: User Story 3 - Scripts and later milestones consume entries reliably (Priority: P2)

**Goal**: byte-exact round-trips, single-value edits through the core API, and a stable JSON
report.

**Independent Test**: parse and serialize every fixture and compare bytes (T047); edit one field
and diff (T048); validate `kb lint --json` (T051).

### Tests for User Story 3 (write first, confirm they fail)

- [X] T047 [P] [US3] Write `tests/roundtrip.rs` (registered as `[[test]]` in `crates/kb-core/Cargo.toml` with `path = "../../tests/roundtrip.rs"`): for every `.md` under `tests/fixtures/` whose frontmatter parses as a mapping (valid, secrets and the parseable invalid ones), `parse_entry(...).entry.unwrap().to_bytes()` equals the file bytes exactly, including BOM, CRLF and comments (SC-001).
- [X] T048 [P] [US3] Write unit tests in `crates/kb-core/src/edit.rs` for `Entry::set_field`: `["code","line"]` 147 → 150 changes only those bytes (compare the prefix and suffix around the span); `["updated"]` on an existing value; inserting a missing top-level `updated` before the closing `---` keeps CRLF in a CRLF file; inserting a missing nested key in a block map goes after the last sibling at the same indentation; inserting into a flow map → `EditError::Unsupported`; a single-quoted value stays single-quoted when possible, otherwise it becomes double-quoted; a value that would make the YAML invalid → `EditError::Invalid` and the entry is unchanged; `kb_format: 2` → `EditError::UnsupportedFormat` and nothing changes (FR-018); unknown keys and their comments survive any edit (FR-015).
- [X] T049 [P] [US3] Write tests in `crates/kb-core/src/edit.rs` for `write_entry`: it writes through a temporary file in the same directory followed by a rename; it refuses an entry whose `kb_format` is greater than `MAX_KB_FORMAT` or invalid, leaving the file untouched (compare bytes and mtime); an entry containing an unsuppressed secret (for example `AKIAIOSFODNN7EXAMPLE`) returns `WriteError::Secret` and leaves the file untouched, while the same line ending in `kb:allow-secret` is written (constitution VI).
- [X] T050 [P] [US3] Write a no-panic test in `tests/roundtrip.rs`: feed `parse_entry` 10,000 inputs from a seeded xorshift generator (random bytes, plus every prefix of each fixture file) and assert it returns without panicking. No new dependency.
- [X] T051 [P] [US3] Add JSON tests to `crates/kb/tests/cli_lint.rs`: `kb lint --json` prints exactly one JSON document; its top-level, `summary`, `findings[]` and `suppressions[]` objects have exactly the required keys of contracts/lint-report.schema.json (no extra keys); `version == 1`; `severity` is one of `error`, `warning`, `info`; `code` matches `^KB[0-9]{3}-[a-z0-9-]+$`; an unknown frontmatter key yields a KB016 `info` finding and the file is unchanged on disk; `--json` with a missing root prints `{"error": {...}}` and exits 2.

### Implementation for User Story 3

- [X] T052 [US3] Implement `Entry::to_bytes()` in `crates/kb-core/src/entry.rs` (BOM if present, then `source.text`). Makes T047 pass.
- [X] T053 [US3] Implement `Entry::set_field(path, FieldValue)` in `crates/kb-core/src/edit.rs` per contracts/core-api.md: span splice for existing scalars, preserving the quoting style when possible; top-level insert before the closing `---` with the file's line ending; nested insert in block maps; `Unsupported` for flow maps; re-parse and roll back on `Invalid`; refuse `UnsupportedFormat`. Depends on T032, T052. Makes T048 pass.
- [X] T054 [US3] Implement `write_entry(path, entry)` in `crates/kb-core/src/edit.rs` (temp file in the same directory, `sync_all`, rename; refuse unsupported `kb_format`; run `scan_secrets` on the full bytes first and refuse any unsuppressed finding with `WriteError::Secret`). Depends on T034. Makes T049 pass.
- [X] T055 [US3] Implement JSON output: `Serialize` for `LintReport` in `crates/kb-core/src/lint/mod.rs` matching contracts/lint-report.schema.json (`version: 1`, absolute `root`, `summary` including `failed` for the active `--strict`, `id: null` for non-entry findings, `fix: null` when absent), and `--json` rendering plus the error document in `crates/kb/src/output.rs`. Depends on T038. Makes T051 pass.
- [X] T056 [US3] Document `--json` for both commands, with a `jq` example, in `README.md`.

**Checkpoint**: all three stories work independently, and `cargo test --workspace` is green.

---

## Phase 6: Polish & Cross-Cutting Concerns

- [X] T057 Write `crates/kb/tests/perf.rs` (`#[ignore]`): generate a 10,000-entry KB in a `tempfile` directory (mixed types, realistic sizes, with variables); assert that `kb lint --quiet` finishes in under 2 s (SC-002) and that the median of 5 `kb --version` runs is under 100 ms (constitution VII). Run with `cargo test --release -p kb --test perf -- --ignored`.
- [X] T058 Add a `perf` job on `ubuntu-latest` and `macos-latest` to `.github/workflows/ci.yml` running T057 in release mode.
- [X] T059 [P] Add a test in `crates/kb-core/src/lint/mod.rs` asserting that every error-severity `RuleCode` produces a `fix` except `KB006-yaml-syntax`, which keeps SC-003 (≥ 80% of error codes have a fix) from regressing.
- [X] T060 [P] Add rustdoc to every public item in `crates/kb-core/src/lib.rs` re-exports, and make `cargo doc --workspace --no-deps` warning-free.
- [X] T061 Run every scenario in `specs/001-entry-format/quickstart.md` on Linux (and macOS when available), then fix the code or update the quickstart where outputs differ.
- [X] T062 Constitution re-check: confirm with `cargo tree -e normal` that only the dependencies listed in T001 are direct dependencies and none does network I/O (III, IX); confirm by test (T042) that `kb init` writes nothing to the root except entries and `.kb/` (I); record the result in `specs/001-entry-format/plan.md` under Constitution Check.

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: no dependencies.
- **Foundational (Phase 2)**: depends on Setup. Blocks every story.
- **US1 (Phase 3)**: depends on Foundational.
- **US2 (Phase 4)**: depends on Foundational, plus T035 (walk) for "has entries". Its examples
  check (T041, T042) uses the US1 linter, so in practice it runs after US1.
- **US3 (Phase 5)**: depends on Foundational and T032 (entry parsing). Its JSON task (T055)
  depends on T038.
- **Polish (Phase 6)**: depends on all stories.

### Within Each Story

- Tests and fixtures first, confirmed failing, then implementation (constitution VIII).
- `body` → `entry` → `vars` → `lint` orchestration → CLI wiring → README.

### Key Task Dependencies

- T013 → everything that emits findings.
- T014 → T015 → T016 → T018, T032.
- T031, T016, T017 → T032 → T033 → T037.
- T034, T035, T036 → T037 → T038.
- T035, T043 → T044 → T045.
- T032 → T052 → T053 → T054. T034 → T054. T038 → T055.

### Parallel Opportunities

- Setup: T002–T006 together after T001.
- Foundational tests: T007–T012 together. Then T014, T017 and T019 in parallel with the T015 →
  T016 → T018 chain.
- US1: T021–T023 and T025–T030 together, then T034, T035 and T036 in parallel with T031 → T032 →
  T033.
- US2: T040–T043 together.
- US3: T047–T051 together.
- Polish: T059 and T060 together.

---

## Parallel Example: User Story 1

```bash
# Fixtures and tests together:
Task: "Create valid fixtures in tests/fixtures/valid/"
Task: "Create invalid fixtures in tests/fixtures/invalid/"
Task: "Create secret fixtures in tests/fixtures/secrets/"
Task: "Unit tests for field rules in crates/kb-core/src/entry.rs"
Task: "Unit tests for variables in crates/kb-core/src/vars.rs"
Task: "Unit tests for secrets in crates/kb-core/src/lint/secrets.rs"
Task: "Unit tests for the walk in crates/kb-core/src/walk.rs"
Task: "CLI tests in crates/kb/tests/cli_lint.rs"

# Independent implementations together:
Task: "Implement scan_secrets in crates/kb-core/src/lint/secrets.rs"
Task: "Implement the walk in crates/kb-core/src/walk.rs"
Task: "Implement file-level rules in crates/kb-core/src/lint/rules.rs"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Phase 1 Setup, then Phase 2 Foundational.
2. Phase 3 (US1): `kb lint` validates hand-written entries.
3. **Stop and validate**: quickstart sections 2, 3 and 6. This alone is a usable tool
   (constitution IX).

### Incremental Delivery

1. Setup + Foundational → foundation ready.
2. US1 → `kb lint` (MVP).
3. US2 → `kb init` with examples.
4. US3 → round-trip guarantees, `set_field`, `--json` for scripts and later milestones.
5. Polish → performance gate in CI, documentation, quickstart run.

Commit after each task or logical group using Conventional Commits (`feat(core): …`,
`test(core): …`, `ci: …`). The milestone is done when the acceptance scenarios pass and CI is
green on macOS and Linux.

---

## Notes

- [P] tasks touch different files and don't depend on an incomplete task.
- Never weaken a fixture's `.expect` to make a test pass: fix the code, or fix the contract and
  the spec together.
- Secret fixtures use public documentation sample values only. If GitHub push protection flags
  one, replace it with a different fake value that still matches the pattern; never bypass the
  protection.
