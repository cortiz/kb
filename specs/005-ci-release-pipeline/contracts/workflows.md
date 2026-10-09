# Contract: workflows

Common to all: `permissions: contents: read` at the top; every job has `timeout-minutes`;
third-party actions are pinned `uses: owner/repo@<40-hex>  # vX.Y.Z`; no job in `ci.yml` needs a
secret (fork pull requests must pass).

## `ci.yml` — name `CI`

Triggers: `pull_request` (types `opened`, `synchronize`, `reopened`; deliberately not `edited`, so
retitling a pull request does not re-run the expensive jobs), `push` to `main`, `workflow_dispatch`
`workflow_call`.
Concurrency: group `ci-${{ github.workflow }}-${{ github.ref }}`; cancel in progress only for
`pull_request`.

| Job id | Display name | Runs on | Needs | Notes |
|---|---|---|---|---|
| `fmt` | `fmt` | ubuntu-24.04 | | |
| `clippy` | `clippy` | ubuntu-24.04 | | `-D warnings`, `--locked` |
| `test` | `test (${{ matrix.os }})` | ubuntu-24.04, macos-15 | | prerequisites check first on Linux |
| `docs` | `docs` | ubuntu-24.04 | | `RUSTDOCFLAGS=-D warnings` |
| `msrv` | `msrv (1.88)` | ubuntu-24.04 | | |
| `deny` | `deny` | ubuntu-24.04 | | all four categories |
| `workflows` | `workflows` | ubuntu-24.04 | | actionlint, shellcheck, script tests |
| `changes` | `changes` | ubuntu-24.04 | | outputs `formula=true` when the diff touches `Formula/` |
| `formula` | `formula (${{ matrix.os }})` | ubuntu-24.04, macos-15 | `changes` | job-level `if: needs.changes.outputs.formula == 'true'`; skipped (not run, not billed) otherwise. On Linux Homebrew needs `eval "$(/home/linuxbrew/.linuxbrew/bin/brew shellenv)"` first |
| `pr-staleness` | `pr-staleness` | ubuntu-24.04 | | `pull_request` only; never fails |
| `ci-ok` | `ci-ok` | ubuntu-24.04 | all of the above | `if: always()`; fails on any `failure`/`cancelled` need; `skipped` counts as passed |

## `pr-title.yml` — name `PR title`

Triggers: `pull_request` types `opened`, `edited`, `synchronize`, `reopened`. One job `pr-title`
(ubuntu-24.04, 2 minutes, read-only): `scripts/check-pr-title.sh` on the title passed through an
environment variable. It is its own workflow so editing a title re-runs only this check, never the
tests. Branch protection requires two names: `ci-ok` and `pr-title`.

## `release.yml` — name `Release`

Triggers: `push` tags `v[0-9]*.[0-9]*.[0-9]*`; `workflow_dispatch` with input `dry_run` (boolean,
default `true`). Concurrency: group `release-${{ github.ref }}`, never cancel.

| Job | Needs | Runs on | Permissions | Output |
|---|---|---|---|---|
| `verify` | | ubuntu-24.04 | read | `version`, `prerelease` |
| `ci` | `verify` | (calls `ci.yml`) | read | |
| `build` (matrix of 4 targets) | `ci` | macos-15 / ubuntu-24.04 | read | artifact `bin-<target>` |
| `smoke` (matrix of 4) | `build` | per target | read | `smoke-<target>` result: tested / not tested |
| `package` | `smoke` | ubuntu-24.04 | read | artifact `dist` (archives + `SHA256SUMS`) |
| `release` | `package`, not on dry run | ubuntu-24.04 | `contents: write`, `id-token: write`, `attestations: write` | release URL |
| `formula-pr` | `release`, stable tags only | ubuntu-24.04 (then macOS for the brew test) | `contents: write`, `pull-requests: write`, `actions: write` | PR URL, or the formula uploaded to the release |

**Why `formula-pr` also checks the formula itself.** A pull request opened with the default
`GITHUB_TOKEN` does not start `pull_request` workflows (GitHub's rule against token-triggered
recursion), so the `formula` job of `ci.yml` would never run on it. The job therefore (1) runs
`brew audit --strict` and `brew test` on the generated formula *before* opening the pull request and
states the result in the PR body, and (2) dispatches `ci.yml` on the formula branch
(`gh workflow run ci.yml --ref chore/formula-<version>`, which a token may do) so the checks exist
on that commit. Whether check runs from a dispatched run satisfy a required `ci-ok` on the pull
request is to be confirmed on the first formula PR; if not, the documented fallback is to close and
reopen the pull request (a human event), which starts the normal run.

Dry run (`workflow_dispatch`, where `GITHUB_REF_NAME` is a branch, not a tag): `verify` calls `check-release-tag.sh --dry-run`, which reads the version from `Cargo.toml`, derives `v<version>` as the tag and skips the "on main" check; `release` and `formula-pr` are skipped; `package`
uploads `dist` as a workflow artifact kept 7 days.

## `perf.yml` — name `Performance`

Triggers: `push` to `main`, `schedule` (Mondays 06:00 UTC), `workflow_dispatch`. Matrix
`ubuntu-24.04`, `macos-15`. On failure: find or create the open issue labelled `perf-regression`
(`issues: write` on that step's job) and comment with the run URL and the failing test output.

## `audit.yml` — name `Audit`

Triggers: `schedule` (Mondays 05:00 UTC), `workflow_dispatch`. Jobs: `advisories` (cargo-deny
`check advisories`, tracking issue labelled `security` on failure), `reproducible` (builds
`x86_64-unknown-linux-musl` twice and compares SHA-256; tracking issue labelled `reproducibility` on
mismatch).

## Required check names for branch protection

`ci-ok` and `pr-title`. (Listed again in [repo-settings.md](repo-settings.md).)
