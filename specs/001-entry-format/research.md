# Research: Entry Format and Parser

Phase 0 for `001-entry-format`. Technical Context had no open NEEDS CLARIFICATION items. This file
records the decisions behind it, and a short spike verified the YAML and Markdown parser behavior
described below (`saphyr-parser` 0.1.0, `pulldown-cmark` 0.13.4, Rust 1.97).

## R1. Byte-exact round-trip (FR-016, FR-017)

- **Decision**: keep the original text as the serialized form. Parse it into a spanned value tree
  for reading, and implement edits as span splices followed by a re-parse.
- **Rationale**: no maintained Rust YAML library preserves comments, quoting and key order when
  serializing. Splicing guarantees FR-016 by construction (unmodified → identical bytes) and
  FR-017 by construction (only the value's span changes). The re-parse catches any splice that
  would produce invalid YAML.
- **Alternatives considered**: `serde_yaml` is archived and loses comments. `serde_yml` has
  soundness and maintenance concerns and also loses comments. A home-grown concrete syntax tree
  YAML parser is far more code than M1 warrants.

## R2. YAML parser

- **Decision**: `saphyr-parser` 0.1 (event API).
- **Rationale**: it is a YAML 1.2 parser (so `no` and `on` are strings, not booleans, which
  matters for enum options). Every event carries a span with start and end markers including line
  and column. The spike confirmed exact value spans for plain, double-quoted and flow-style scalars,
  and that CRLF input works. **Caveat found in the spike**: `Marker::index()` counts characters,
  not bytes (`"héllo wörld"` gave a 13-wide span for 15 bytes). `Source` converts with a lazily
  built char→byte table, skipped for pure-ASCII files.
- **Alternatives considered**: `yaml-rust2` 0.13 has the same lineage and API but saphyr is where
  maintenance is moving. The high-level `saphyr` crate discards spans. `serde` deserialization
  directly gives no positions for findings.

## R3. Locating the payload (FR-007)

- **Decision**: `pulldown-cmark` 0.13 with `default-features = false`, using `into_offset_iter()`
  to find the first `Start(CodeBlock(Fenced(_)))` and its byte range.
- **Rationale**: it handles every CommonMark fence rule: backtick and tilde fences, fences of four
  or more characters, info strings, fences inside block quotes and lists, and indented code blocks,
  which are not fences. The spike confirmed a ```` ```` ```` block containing a ```` ``` ```` block
  comes back as one block. Writing these rules by hand is the kind of complexity a dependency
  should remove (constitution IX). The payload is the block's content lines. Notes are the body
  with the block's range removed.
- **Alternatives considered**: a hand-written fence scanner is small but gets nested containers
  and indentation wrong. `comrak` is heavier and builds a full AST.

## R4. Secret detection (FR-013)

- **Decision**: `regex` 1 with patterns compiled once on first use (`std::sync::LazyLock`), plus
  Shannon entropy written by hand.

  | Rule | Pattern (case-insensitive where marked) |
  |------|------------------------------------------|
  | AWS access key ID | `\b(AKIA\|ASIA\|ABIA\|ACCA)[A-Z0-9]{16}\b` |
  | AWS secret key in assignment | `(?i)aws_?secret_?(access_?)?key\s*[:=]\s*["']?[A-Za-z0-9/+=]{40}` |
  | Bearer token | `(?i)\bbearer\s+[A-Za-z0-9._~+/-]{20,}=*` |
  | JWT | `\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}` |
  | Private key header | `-----BEGIN ([A-Z0-9]+ )*PRIVATE KEY( BLOCK)?-----` |
  | High-entropy assignment | `(?i)\b\w*(password\|passwd\|pwd\|token\|secret\|api_?key\|access_?key)\w*\s*[:=]\s*["']?([^\s"'{}$]{12,})` with entropy ≥ 3.5 bits per character |

- **Rationale**: these patterns follow the well-known ones used by gitleaks and trufflehog, cut
  down to the list in FR-013. Excluding `{`, `}` and `$` from captured values means `{{token}}`
  references and `$TOKEN` or `${TOKEN}` environment variables never trigger a finding, which keeps
  SC-005's "no false positives on clean fixtures" achievable. The entropy threshold of 3.5 separates
  random 12+ character tokens from words like `changeme_please`, and the fixture set pins it.
- **Suppression**: a line is suppressed when it ends with `kb:allow-secret` after optional
  whitespace, whatever comment syntax comes before it (spec Assumptions). Suppressed matches are
  counted and listed in the report but do not count as errors.
- **Redaction**: messages show at most the first 4 characters of the match, then `…`.
- **Alternatives considered**: running gitleaks is an external binary (constitution III and IX).
  Entropy-only scanning is too noisy on hashes in SQL.

## R5. Directory walk

- **Decision**: `walkdir` 2 with `follow_links(false)`. Symlinks are handled by hand: canonicalize
  the target and follow it only if it is under the canonicalized root, otherwise report KB052
  (warning) and skip it. Loops are prevented by a visited set of canonical directories.
- **Hidden entries**: any path component *below the root* that starts with `.` is skipped, and that
  covers hidden files (`.draft.md`) as well as hidden directories. A root that is itself hidden
  (`~/.kb-data`) still works.
- **Rationale**: `walkdir` handles traversal errors and ordering across platforms, and
  `sort_by_file_name` gives deterministic output, so tests are stable on macOS and Linux.
- **Alternatives considered**: `ignore` (ripgrep's walker) reads `.gitignore` files, which is the
  wrong behavior here, and is larger. Plain `std::fs::read_dir` recursion would need hand-written
  loop and error handling.

## R6. Case-only ID collisions

- **Decision**: group IDs by their lowercase form across the set of files being linted, and report
  KB053 on every member of a group larger than one, on every platform. When `kb lint` gets
  explicit paths, collisions are checked only among those files.
- **Rationale**: the spec assumes a KB that lints clean on Linux must also work on a
  case-insensitive macOS checkout.

## R7. Configuration file and KB root

- **Decision**: the config file is YAML at `config.yml`, with one key read in M1: `root` (a string,
  and a leading `~/` expands to the home directory). Locations, provided by the `Platform` trait:
  - Linux: `$XDG_CONFIG_HOME/kb/config.yml`, falling back to `~/.config/kb/config.yml`.
  - macOS: `~/Library/Application Support/kb/config.yml`.

  Resolution order (FR-022): `--root`, then `KB_ROOT`, then config `root`, then `~/kb`. A config
  file that exists but cannot be read or parsed is a usage error (exit 2) naming the file. Unknown
  config keys are ignored in M1.
- **Rationale**: reusing the YAML parser the format already needs avoids a TOML dependency
  (constitution IX). The home directory comes from `std::env::home_dir()`, which is correct on both
  targets since Rust 1.86 and passes `clippy -D warnings` (confirmed in the spike).
- **Alternatives considered**: `directories` or `etcetera` crates would replace about 20 lines.
  TOML would be more conventional for Rust tools but means a second parser.

## R8. Type suggestions ("did you mean")

- **Decision**: suggest a known value when it is a prefix of the input (`sumologic` → `sumo`) or
  within edit distance 2 (`cdm` → `cmd`), preferring the prefix match. The distance is optimal
  string alignment (Levenshtein plus swaps of two adjacent characters), so a swapped pair costs 1
  and `cdm` picks `cmd` over `cw`. The same helper is used for unknown keys (`tilte` → `title`) and
  for `shell` values. The distance function is about 20 lines.
- **Alternatives considered**: `strsim` is not worth a dependency for one function.

## R9. Dates (FR-005)

- **Decision**: validate by hand: `YYYY-MM-DD`, or an RFC 3339 date-time (`YYYY-MM-DDTHH:MM[:SS[.f]]`
  followed by `Z` or `±HH:MM`), with day-of-month and leap-year checks. Values are kept as strings.
  M1 never computes with dates.
- **Alternatives considered**: `jiff` or `chrono` are heavy dependencies for a format check.

## R10. Paths given to `kb lint`

- **Decision**: absolute paths are used as given. A relative path is resolved against the current
  directory if it exists there, otherwise against the KB root, so `kb lint sumo/x.md` works from
  anywhere (User Story 1, scenario 4). A path outside the KB root, or one that doesn't exist, is a
  usage error (exit 2). Reported paths are always relative to the KB root.

## R11. Cold start (constitution VII)

- **Decision**: no global initialization. Regexes compile on the first secret scan, and the
  examples are `include_str!` constants. `kb --help` touches no files.
- **Measurement**: the CI performance job times `kb --version` and `kb lint --quiet` on the
  generated 10,000-entry KB in release mode and fails above the budgets (100 ms and 2 s).

## R12. CI matrix

- **Decision**: GitHub Actions jobs on `ubuntu-latest` and `macos-latest` (arm64): `cargo fmt
  --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Add
  a `cargo build --target x86_64-apple-darwin` step on macOS so Intel builds can't break silently,
  plus the release-mode performance job on both OSes. The `rust-version` 1.88 minimum is tested in
  a separate Linux job.
- **Rationale**: the constitution makes CI green on macOS and Linux part of "done".
