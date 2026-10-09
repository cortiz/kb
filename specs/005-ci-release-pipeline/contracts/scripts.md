# Contract: helper scripts

All scripts: Bash, `set -euo pipefail`, pass `shellcheck`, take arguments (never read the pull
request title or any untrusted text from the environment into a shell expression), print one-line
errors prefixed `error:` on stderr, and have a test in `scripts/tests/`.

| Script | Arguments | Success | Failure |
|---|---|---|---|
| `check-release-tag.sh` | `<tag>`, or `--dry-run` with no tag (the version is read from `Cargo.toml` and the tag derived as `v<version>`; the "on main" check is skipped) | exit 0; prints `tag=<v…>`, `version=<v>` and `prerelease=<true\|false>` | exit 1 + message naming both mismatching values, or the bad syntax, or "not on main" |
| `check-pr-title.sh` | `<title>` | exit 0 | exit 1 + the expected format |
| `pr-staleness.sh` | `<owner/repo> <pr-number>` | always exit 0; appends a warning to `$GITHUB_STEP_SUMMARY` and prints `::warning::` when older than 7 days or 50+ commits behind | exit 2 only on a usage error |
| `smoke.sh` | `<path-to-kb> <expected-version> [<runner-prefix>]` (the prefix is e.g. `qemu-aarch64` or `arch -x86_64`) | exit 0; prints `smoke: ok` | exit 1 + the failing step |
| `package.sh` | `<version> <target> <path-to-kb> <outdir>` ; env `SOURCE_DATE_EPOCH` | writes the archive, appends to `<outdir>/SHA256SUMS` | exit 1 if a license file or README is missing |
| `gen-formula.sh` | `<tag> <SHA256SUMS> <out-file>` | writes the formula | exit 1 if a target's checksum is missing |
| `formula-changed.sh` | `<event-name> <base-ref> <before-sha> <after-sha>` | prints `formula=true\|false`; never fails on a zero `before` (new branch, tag push) | exit 2 only on a usage error |
| `check-pins.sh` | `[<workflows-dir>]` (default `.github/workflows`) | exit 0 | exit 1 listing each `file:line` whose `uses:` is not `owner/repo@<40 hex>` plus a `# vX.Y.Z` comment (local `./` and `docker://` references are allowed) |
| `track-issue.sh` | `<label> <title> <body-file>` | comments on the open issue with the label, or creates one | exit 1 with the `gh` message |
| `ci-local.sh` | none | runs fmt, clippy, tests, docs, and deny/actionlint/shellcheck when installed | exit 1 at the first failing step, naming it |

`scripts/tests/run.sh` runs every `test_*.sh` and exits non-zero if any fails.
