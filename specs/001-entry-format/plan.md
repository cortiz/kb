# Implementation Plan: Entry Format and Parser

**Branch**: `001-entry-format` | **Date**: 2026-10-08 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/001-entry-format/spec.md`

## Summary

M1 defines the kb entry file format and ships the code every later milestone reads entries
through: a lossless parser, a validator, and the `kb init` and `kb lint` commands.

The technical approach rests on one decision: **the original file bytes are the serialized form.**
The parser never rebuilds YAML from a tree. It keeps the source text, records the byte span of
every frontmatter value it reads, and serializes an unmodified entry by returning the source
unchanged (FR-016). A field edit splices new bytes into that one span (FR-017). This gives
byte-exact round-trips, including comments, quoting and CRLF, without a format-preserving YAML
library, which Rust does not have.

The frontmatter is read with an event-level YAML 1.2 parser that reports positions, and the body
with a CommonMark parser that reports byte offsets for fenced code blocks. Lint is a set of
independent rules over the parsed entry plus a regex-based secret scanner over the raw text.

## Technical Context

**Language/Version**: Rust stable, edition 2024, `rust-version = "1.88"` (CI tests stable and the
declared minimum).

**Primary Dependencies**:

- `kb-core`: `saphyr-parser` 0.1 (YAML events with positions), `pulldown-cmark` 0.13 without
  default features (fenced code block offsets), `regex` 1 (secret patterns), `serde` +
  `serde_json` (report and entry JSON), `walkdir` 2 (directory walk with symlink control).
- `kb`: `clap` 4 with `derive`.
- Dev only: `assert_cmd`, `tempfile`.
- Each one is justified in [research.md](research.md), along with the ones rejected.

**Storage**: plain files under the KB root. M1 creates `.kb/` but stores nothing in it yet (the
index arrives in M2).

**Testing**: `cargo test`. Unit tests next to the code, a fixture KB in `tests/fixtures/`,
round-trip tests over every fixture, CLI tests with `assert_cmd`, and an ignored-by-default
performance test run in release mode in CI.

**Target Platform**: macOS arm64 and x86_64, Linux x86_64 and aarch64.

**Project Type**: Cargo workspace with a library (`kb-core`) and a CLI binary (`kb`).

**Performance Goals**: `kb lint` on 10,000 entries in < 2 s (SC-002). CLI cold start ≤ 100 ms
(constitution VII), so no work happens at startup that `--help` or `init` does not need.

**Constraints**: no network, no telemetry. No writes to the KB root except entries and `.kb/`.
Never rewrite a file with an unsupported `kb_format`. Secret values never appear in output.

**Scale/Scope**: 7 entry types, 48 lint rules, 2 commands, KBs up to about 10,000 entries of
up to about 1 MB each.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

| # | Principle | Status | How this plan complies |
|---|-----------|--------|------------------------|
| I | Plain Files Are the Source of Truth | Pass | The format is defined in [contracts/entry-format.md](contracts/entry-format.md). The source bytes are the serialization, and `kb_format: 1` is required. `kb init` writes only example entries, `.kb/` and `.kb/.gitignore` (spec clarification). |
| II | The Index Is Disposable | Pass (n/a) | No index in M1. `.kb/` is created and self-ignored by `.kb/.gitignore`, ready for M2. |
| III | Local-Only, No Server | Pass | No network code and no new dependency that does I/O beyond the file system. |
| IV | Terminal Only, One Core | Pass | Parsing, validation, the walk, secret detection, root resolution and `init` planning all live in `kb-core`. The `kb` crate parses arguments, calls core and prints. Both commands support `--json`. |
| V | Portable by Construction | Pass | Home and config directories sit behind a `Platform` trait in `crates/kb/src/platform/` with `linux.rs` and `macos.rs`. Core takes paths as input and does no OS lookups. Paths are normalized to `/` in IDs. CI runs on both OSes. |
| VI | Secrets Are Never Stored | Pass | Rules KB060–KB065 in [contracts/lint-rules.md](contracts/lint-rules.md) scan the whole file. Findings redact the matched value. `secret` variables cannot have `default`. `write_entry`, the only save path in M1, runs the scanner and refuses unsuppressed findings, and M6 reuses it. |
| VII | Keyboard-First and Fast | Pass | CLI only in M1. Lint budget of 2 s for 10,000 entries, with a CI performance test. Regexes compile lazily so cold start isn't affected. No command runs anything. |
| VIII | Test-First Core | Pass | Tasks are ordered so failing tests come first. The fixture KB covers every type, and every fixture round-trips. CI runs `cargo fmt --check`, `clippy -D warnings` and tests on macOS and Linux. |
| IX | Simplicity First | Pass | Six runtime dependencies, each justified. No config format beyond one YAML key, no plugin points, no async, no parallelism. M1 alone is a usable validator. |

**Gate result**: pass, no exceptions. Complexity Tracking stays empty.

## Project Structure

### Documentation (this feature)

```text
specs/001-entry-format/
├── plan.md              # This file
├── research.md          # Phase 0: decisions and rejected alternatives
├── data-model.md        # Phase 1: entities, fields, validation
├── quickstart.md        # Phase 1: runnable validation scenarios
├── contracts/
│   ├── entry-format.md          # The on-disk format (kb_format 1)
│   ├── cli.md                   # kb init / kb lint: flags, output, exit codes
│   ├── lint-rules.md            # Every rule code, severity and fix
│   ├── lint-report.schema.json  # JSON Schema for kb lint --json
│   ├── init-report.schema.json  # JSON Schema for kb init --json
│   └── core-api.md              # Public kb-core API used by the CLI and later milestones
├── checklists/requirements.md
└── tasks.md             # Phase 2 (/speckit-tasks)
```

### Source Code (repository root)

```text
Cargo.toml                    # workspace: members = ["crates/*"], shared lints and versions
rust-toolchain.toml           # channel = "stable", components = ["rustfmt", "clippy"]
.github/workflows/ci.yml      # fmt, clippy -D warnings, test; ubuntu + macos; perf job

crates/kb-core/
├── Cargo.toml
└── src/
    ├── lib.rs                # public API re-exports
    ├── source.rs             # Source: bytes, BOM, line endings, line index, char→byte map
    ├── frontmatter.rs        # split ---/---, YAML events → spanned value tree
    ├── yaml.rs               # Spanned node types (Scalar/Seq/Map with byte spans)
    ├── body.rs               # payload + notes via pulldown-cmark offsets
    ├── entry.rs              # Entry, types, fields, CodeRef; rules KB001, KB010–KB036
    ├── vars.rs               # VarDef, references, \{{ escape; rules KB040–KB049
    ├── edit.rs               # set_field: span splice, value formatting
    ├── walk.rs               # KB walk, hidden filter, symlinks, IDs; rules KB052–KB053
    ├── root.rs               # KB root resolution order (pure; platform paths passed in)
    ├── init.rs               # plan + apply kb init (create-only)
    ├── examples/             # one example .md per type, embedded with include_str!
    └── lint/
        ├── mod.rs            # lint(), LintReport, Finding, RuleCode, Severity
        ├── rules.rs          # file-level rules KB050–KB051
        ├── secrets.rs        # KB060–KB065, entropy, kb:allow-secret
        └── suggest.rs        # "did you mean" (prefix + Levenshtein ≤ 2)

crates/kb/
├── Cargo.toml                # [[bin]] name = "kb"
└── src/
    ├── main.rs               # clap definitions, dispatch, exit codes
    ├── output.rs             # human and JSON rendering
    └── platform/
        ├── mod.rs            # trait Platform { home_dir, config_file }
        ├── linux.rs          # XDG_CONFIG_HOME or ~/.config/kb/config.yml
        └── macos.rs          # ~/Library/Application Support/kb/config.yml

tests/
├── fixtures/
│   ├── valid/                # one or more valid entries per type, CRLF, BOM, long fences
│   ├── invalid/              # one file per rule; <name>.expect lists "line:col CODE"
│   └── secrets/              # positive and negative secret samples
├── roundtrip.rs              # every fixture: parse → serialize == bytes
└── lint_fixtures.rs          # every invalid fixture produces exactly its .expect findings

crates/kb/tests/
├── cli_init.rs               # assert_cmd scenarios for User Story 2
├── cli_lint.rs               # assert_cmd scenarios for User Story 1 and JSON output
└── perf.rs                   # #[ignore] 10,000-entry generated KB, release mode, cold start
```

The root `tests/` holds the shared fixture KB the constitution requires (`tests/fixtures/`) and
the core integration tests, attached to `kb-core` through `[[test]]` entries with
`path = "../../tests/..."`. The CLI tests in `crates/kb/tests/` need the built binary, so they
live with the `kb` crate and read the same fixtures.

**Structure Decision**: a two-crate Cargo workspace under `crates/`, as the constitution's
technical constraints require. The `platform` module lives in the `kb` binary because it is
platform glue. Core receives already-resolved directories, which keeps it pure and testable on
every OS.

## Key Design Points

1. **Lossless model.** `Entry` holds a `Source` (the original `String`, BOM flag and line-ending
   style) and a spanned YAML tree. `Entry::to_bytes()` returns the BOM plus the source. Edits go
   through `Entry::set_field(path, value)`, which re-quotes the value only when needed, splices it
   into the span and re-parses to confirm the result is valid ([contracts/core-api.md](contracts/core-api.md)).
2. **Positions.** `saphyr-parser` reports character offsets, not byte offsets (checked in a spike;
   see research R2). `Source` builds a char→byte table only for files with non-ASCII text, and
   ASCII files use offsets directly. Columns in findings count Unicode characters, starting at 1.
3. **Strict YAML subset.** Anchors, aliases, explicit tags, duplicate keys and multi-line flow
   collections spanning the closing `---` are rejected. This keeps every value at exactly one
   span, so edits are unambiguous and files stay friendly to hand editing.
4. **Lint never stops at the first error.** Every file yields all its findings. A file that fails
   structurally (no frontmatter, invalid UTF-8) yields that one error, and other files carry on.
5. **Secrets are never echoed.** Messages show at most the first 4 characters followed by `…`,
   in both human and JSON output.
6. **Exit codes** come from the report (`LintReport::exit_code(strict)`), so the CLI holds no
   policy.

## Complexity Tracking

No constitution violations to justify.
