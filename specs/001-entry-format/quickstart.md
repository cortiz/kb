# Quickstart: Validate M1 (Entry Format and Parser)

Runnable checks that prove the milestone works end to end. Formats and rule codes are in
[contracts/](contracts/). These scenarios are what the CLI integration tests automate.

## Prerequisites

- Rust stable (≥ 1.88), macOS or Linux.
- From the repository root: `cargo build --release`. The binary is `target/release/kb`.
- A scratch directory: `export T=$(mktemp -d)` and `alias kb=$PWD/target/release/kb`.

## 1. Create a KB (User Story 2)

```sh
kb init "$T/kb"
ls -A "$T/kb"            # .kb  cmd  code  cw  link  prompt  sql  sumo   (no .gitignore at root)
cat "$T/kb/.kb/.gitignore"   # *
kb --root "$T/kb" lint --strict; echo "exit $?"
```

Expected: 7 example entries, then `7 entries, 0 errors, 0 warnings` and `exit 0`.

Re-run on the existing KB, and try `--no-examples` on a fresh one:

```sh
kb init "$T/kb" --json        # "created": [], "examples": "skipped-has-entries"
kb init "$T/empty" --no-examples && ls -A "$T/empty"   # .kb only
```

## 2. Lint a hand-written entry (User Story 1)

```sh
mkdir -p "$T/kb/sumo" && cat > "$T/kb/sumo/bad.md" <<'EOF'
---
kb_format: 1
type: sumologic
title: 5xx by authorizer
---

```
_sourceCategory=prod/lambda/authorizer | count
```
EOF
kb --root "$T/kb" lint; echo "exit $?"
```

Expected: `sumo/bad.md:3:7: error[KB001-unknown-type]: …`, a `fix: use "sumo"` line, and `exit 1`.

Scope to one file, from any directory:

```sh
sed -i.bak 's/sumologic/sumo/' "$T/kb/sumo/bad.md" && rm "$T/kb/sumo/bad.md.bak"
(cd /; kb --root "$T/kb" lint sumo/bad.md; echo "exit $?")   # 1 entries… exit 0
```

Warnings and `--strict`: add `vars: { region: { kind: text } }` to the frontmatter without using
it. `kb lint` exits 0 with a KB048 warning, and `kb lint --strict` exits 1.

## 3. Secrets are caught and never echoed

```sh
printf -- '---\nkb_format: 1\ntype: cmd\ntitle: t\n---\n\n```\ncurl -H "Authorization: Bearer abcdefghijklmnopqrstuvwxyz012345" x\n```\n' > "$T/kb/cmd/leak.md"
kb --root "$T/kb" lint | grep KB062     # message shows at most "abcd…"
```

Add ` # kb:allow-secret` to the end of the `curl` line: lint exits 0 and the summary ends with
`, 1 suppressed`.

## 4. JSON output (User Story 3)

```sh
kb --root "$T/kb" lint --json | jq '.summary, .findings[0] | keys'
```

Expected: one JSON document that validates against
[contracts/lint-report.schema.json](contracts/lint-report.schema.json).

## 5. Round-trip and field edits (User Story 3, SC-001)

```sh
cargo test --workspace roundtrip
```

Expected: every file under `tests/fixtures/valid/` (including CRLF, BOM, comments, flow maps and
four-backtick fences) serializes back byte-for-byte. The `set_field` tests change `code.line` and
`updated` and diff to exactly one changed value.

## 6. Usage errors

```sh
kb --root "$T/missing" lint; echo "exit $?"     # exit 2, "kb: KB root … does not exist"
KB_ROOT="$T/kb" kb lint --quiet                  # resolves the root from the environment
```

## 7. Performance (SC-002)

```sh
cargo test --release -p kb --test perf -- --ignored --nocapture
```

Expected: a generated 10,000-entry KB lints in under 2 s, and `kb --version` starts in under
100 ms.
