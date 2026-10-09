# Implementation Plan: CI and Release Pipeline

**Branch**: `005-ci-release-pipeline` | **Date**: 2026-10-09 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/005-ci-release-pipeline/spec.md` (including its 2026-10-09 clarifications)

## Summary

Replace the single `.github/workflows/ci.yml` with a small set of workflows that gate pull
requests, publish releases from version tags, keep dependencies honest, and keep the project
trunk-based. There is no Rust code to write; the deliverables are workflow files, a few helper
scripts, a dependency policy, a changelog config, a Homebrew formula template and the
contributor documentation.

Decisions (reasons and rejected alternatives in [research.md](research.md)):

1. **Two required checks.** `ci-ok` aggregates every job of `ci.yml` (and treats skipped as passed),
   so a job that is skipped or renamed can never leave a pull request waiting; `pr-title` is a
   separate small workflow so editing a title never re-runs the tests. Branch protection names
   those two. The individual jobs keep their own names for diagnosis.
2. **`ci.yml` is reusable.** It runs on pull requests, on pushes to `main`, by hand, and is called
   by the release workflow, so a release runs exactly the checks a pull request ran.
3. **No setup actions for Rust.** `rustup` is on every runner; the workflows call it directly. That
   removes one third-party dependency and the "pin a moving branch" problem of `rust-toolchain`
   actions. The few third-party actions that remain are pinned by commit hash.
4. **Linux releases are static musl binaries cross-built with Zig.** One toolchain for both
   Linux architectures, no emulated build, and the aarch64 binary is smoke-tested under QEMU
   user-mode. Apple binaries are built on one Apple-silicon runner (Intel by cross-compiling).
5. **Draft first, publish last.** A release is created as a draft, filled with assets, verified,
   and only then published, so a failed or re-run pipeline never leaves a half-published release.
6. **Homebrew formula in this repository**, updated by a pull request the release opens, never by a
   push to `main`.
7. **Trunk-based by construction.** A PR-title check (Conventional Commits) feeds squash merges,
   which feed the release notes; settings and branch protection are documented as a checklist the
   maintainer applies.

Facts found while planning that shaped it:

- The repository is private on the free plan: no branch protection or rulesets, no artifact
  attestations, 2,000 Actions minutes a month with macOS minutes counted ten times. Each of these
  has a documented fallback (R12, R16, R20); making the repository public lifts all three.
  **The minutes are the tightest: trunk-based work means each merged change costs about 380 billed
  minutes (pull request run + push to `main` + performance run), so the allowance covers roughly
  five merges a month. The maintainer chooses a lever before tasks (R12).**
- Pushes fail today because git uses SSH and the key is rejected, while `gh` is logged in over
  HTTPS. Switching the remote to HTTPS (`gh auth setup-git`) is the first step of the quickstart.
- All 185 crates in the lockfile are under permissive licenses (MIT, Apache-2.0, BSD, Zlib,
  Unicode, Unlicense, BSL-1.0, and `OR` choices that include one of them), so a strict allow-list
  policy needs no exceptions. The two workspace crates have no `license` field and the repository has
  no license file yet.
- `main.rs` relies on `catch_unwind`, so release builds keep `panic = "unwind"`.

## Technical Context

**Language/Version**: GitHub Actions workflow YAML; Bash for helper scripts (POSIX features plus
`set -euo pipefail`); Rust 2024, MSRV 1.88, stable for everything else (unchanged).

**Primary Dependencies**: no new Rust dependencies. Third-party actions, each pinned to a commit
hash: `Swatinem/rust-cache`, `EmbarkStudios/cargo-deny-action`, `taiki-e/install-action` (for
git-cliff, cargo-zigbuild, actionlint, shellcheck is preinstalled), `mlugg/setup-zig`, `docker/setup-qemu-action` (arm64 containers for the Linux smoke test),
`actions/checkout`, `actions/upload-artifact`, `actions/download-artifact`,
`actions/attest-build-provenance`. Everything else uses preinstalled tools (`gh`, `rustup`,
`tar`, `shasum`, `brew`).

**Storage**: N/A. Release assets live in GitHub Releases; the formula lives in `Formula/kb.rb`.

**Testing**: the pipeline is tested three ways: static checks in CI (`actionlint`, `shellcheck`),
script tests in `scripts/tests/` run on the Linux runner, and live runs (a dry-run dispatch, a
throw-away pre-release tag, then a real release). GitHub-hosted behavior cannot be reproduced
locally; `act` is installed but Docker is not running here, so local runs cover the scripts only.

**Target Platform**: GitHub-hosted runners `ubuntu-24.04` and `macos-15` (pinned names, not
`-latest`, for release builds); release targets `aarch64-apple-darwin`, `x86_64-apple-darwin`,
`x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`.

**Project Type**: repository infrastructure (workflows, scripts, policy files, docs).

**Performance Goals**: pull request required checks ≤ 10 min warm and ≤ 20 min cold; a release
run ≤ 40 min end to end (spec SC-001, SC-003).

**Constraints**: all jobs read-only by default; write permission only in the release and formula
jobs; no secrets needed for pull requests from forks; pushes to `main` only through pull requests;
the free-plan minute budget (macOS counted ×10) must hold up under normal use.

**Scale/Scope**: one maintainer, a few pull requests a week, a release every few weeks.

## Constitution Check

*GATE: pass before research; re-checked after design.*

| Principle | Before | After design | Notes |
|---|---|---|---|
| I. Plain files | Pass | Pass | The pipeline reads and writes only repository files and release assets; nothing touches a KB. |
| II. Index is disposable | Pass | Pass | Not involved. |
| III. Local-only, no server | Pass | Pass | CI is build infrastructure, not part of the product. The shipped binary gains no network behavior, telemetry or update check; the Homebrew formula and release archives are downloads the user chooses. |
| IV. Terminal only, one core | Pass | Pass | Not involved. |
| V. Portable by construction | Pass | Pass | The pipeline builds and tests both macOS and Linux and ships all four promised targets. |
| VI. Secrets never stored | Pass | Pass | No secret is introduced. The default `GITHUB_TOKEN` with narrow per-job permissions does everything; there is no crates.io token and no tap token. |
| VII. Keyboard-first and fast | Pass | Pass | The timing budgets keep running (weekly and on `main`), not on every pull request. |
| VIII. Test-first core | Pass | Pass | This feature is the mechanism for "CI green on macOS and Linux"; the first green run closes M4's T062. Test-first applies to the scripts (tests before scripts). |
| IX. Simplicity first | Pass | **Justified** | See Complexity Tracking: Zig cross-compilation, one aggregate job, a formula generator. No Rust dependency is added. |

Constitution Distribution constraint: macOS arm64/x86_64 and Linux x86_64/aarch64 tarballs ✓;
Homebrew from this repository ✓ (not a separate tap, spec clarification); `cargo install` from git
works as today (crates.io is out of scope by decision); "single static binary" ✓ on Linux, macOS
binaries link only the system libraries every Mac has.

Development Workflow: Conventional Commits ✓ (now enforced on pull request titles). "One branch per
milestone" is compatible with trunk-based development only while milestone branches are short; a
PATCH amendment saying so is suggested in the quickstart, not made here.

## Project Structure

### Documentation (this feature)

```text
specs/005-ci-release-pipeline/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── workflows.md       # triggers, jobs, job names, permissions, concurrency
│   ├── release.md         # tag format, assets, archive layout, notes, formula, how users verify
│   ├── repo-settings.md   # GitHub settings and branch protection checklist (gh commands)
│   └── scripts.md         # the helper scripts: arguments, outputs, exit codes
├── checklists/requirements.md
└── tasks.md               # /speckit-tasks output (not created here)
```

### Source Code (repository root)

```text
.github/
├── workflows/
│   ├── ci.yml             # REPLACE: PR + push to main + dispatch + workflow_call
│   ├── pr-title.yml       # NEW: Conventional Commit title check (re-runs on edit)

│   ├── release.yml        # NEW: tags v*; verify → ci → build×4 → package → draft → publish → formula PR
│   ├── perf.yml           # NEW: push to main + weekly; timing budgets; tracking issue on failure
│   └── audit.yml          # NEW: weekly; cargo-deny advisories + reproducibility check; tracking issue
├── dependabot.yml         # NEW: cargo + github-actions, grouped, weekly
└── formula.rb.tmpl        # NEW: Homebrew formula template (placeholders, see contracts/release.md)
scripts/
├── check-release-tag.sh   # NEW: tag syntax, tag == Cargo.toml version, commit on main
├── check-pr-title.sh      # NEW: Conventional Commit validation of a title
├── pr-staleness.sh        # NEW: age / commits-behind warning for the job summary
├── package.sh             # NEW: reproducible tar.gz + SHA256SUMS from built binaries
├── smoke.sh               # NEW: the kb --version / init / lint / search / doctor smoke test
├── gen-formula.sh         # NEW: fills the template from a release's SHA256SUMS
├── formula-changed.sh     # NEW: does this event's diff touch Formula/? (false on tag pushes)
├── check-pins.sh          # NEW: fails when a `uses:` is not pinned to a 40-hex hash with a version comment
├── track-issue.sh         # NEW: find-or-create the tracking issue for perf/audit failures

├── ci-local.sh            # NEW: the PR checks, runnable on a developer machine
└── tests/                 # NEW: shell tests for the scripts above
deny.toml                  # NEW: dependency policy
cliff.toml                 # NEW: release-notes grouping by Conventional Commit type
Formula/kb.rb              # created by the first stable release's formula PR
Cargo.toml                 # CHANGE: [profile.release] lto/codegen-units/strip; [workspace.package] license
LICENSE-MIT, LICENSE-APACHE# NEW (default choice; the maintainer confirms before the first release)
CONTRIBUTING.md            # NEW: trunk-based workflow, how to cut a release, required checks, local checks
README.md                  # CHANGE: install section (archives, Homebrew), status line
```

**Structure Decision**: workflow files stay thin (orchestration only); anything with logic is a
script in `scripts/` with its own tests, so it can be run and tested without GitHub. Five workflow files, split by trigger and by who needs the result (PR author, release, owner), not by tool.

## Complexity Tracking

| Item | Why needed | Simpler alternative rejected because |
|---|---|---|
| Zig (`cargo-zigbuild`) for the Linux builds | Static musl binaries for two architectures from one x86_64 runner, including bundled SQLite's C code | Native arm64 runners are free only for public repositories; `cross` needs Docker images per target and is slower; installing `musl-tools` plus an aarch64 sysroot by hand is brittle |
| `ci-ok` aggregate job | One stable required-check name; skipped jobs count as passed | Requiring each job by name breaks whenever one is renamed or conditionally skipped, which blocks merges forever |
| Formula generator script + template | The release must produce a correct formula from the real checksums | Hand-editing the formula after each release is the step people forget |
| git-cliff | Release notes grouped by Conventional Commit type | GitHub's generated notes group by labels and PR author, not by commit type; a hand-written grouping script is more code than a config file |
| A dry-run dispatch mode in `release.yml` | Test the whole build and packaging path without tagging or publishing | Testing only with real tags leaves tags and releases to clean up |

## Post-design re-check

Re-read after `research.md`, the contracts and `quickstart.md` were written. The constitution table
stands. Risks that remain real, in order of likelihood:

1. **The musl + Zig build of the whole workspace is unverified.** Bundled SQLite (C), syntect with a
   pure-Rust regex engine, signal-hook and crossterm are all expected to work on musl, but nothing
   has been compiled for it yet. The dry-run dispatch is the first thing to run; the fallback is
   `cross`, then the gnu targets with a documented glibc floor.
2. **Linux has never run the test suite.** The first push will show failures that are real bugs in
   the platform-dependent tests; fixing them is part of this feature (spec Assumptions).
3. **Rosetta on the macOS arm64 runner** may be missing, in which case the Intel binary is
   smoke-tested on the `macos-15-intel` runner instead, or marked not smoke-tested (FR-015).
4. **Minutes.** See R12: about 380 billed minutes per merged change on a private repository, far
   under what trunk-based work needs. The spec as written is implemented unless the maintainer
   picks a lever (public repository, weekly-only performance runs, no macOS on `main` pushes).
5. **Formula pull requests do not start workflows by themselves** (token rule). Handled by testing
   the formula inside the release job and dispatching `ci.yml`; unverified until the first stable
   release (R17).
6. **License first.** `deny` is red on the first pull request until the workspace crates have a
   `license`; the license choice (default MIT OR Apache-2.0) is the maintainer's and is the first
   task.

Analysis follow-ups folded in: the Linux clean-machine check runs inside the release workflow
(containers on the hosted runner, so it does not depend on a Docker daemon on the maintainer's
machine); the formula `changes` check is a tested script that never errors on tag pushes; the
Intel Mac smoke test falls back to the `macos-15-intel` runner before giving up. Two requirements
are met by design and have no dedicated task: SC-007 (no silently skipped check: every job always
runs and `ci-ok` counts skipped as passed) and FR-025 (the formula exists only after the first
stable release's formula pull request is merged).

## Post-implementation re-check

Written on the branch before any push; **no workflow has run on GitHub yet**.

**Verified locally**
- Script tests (10 files) green on bash 5 and macOS `/bin/bash` 3.2, each seen red first; shellcheck,
  actionlint 1.7.12 and `scripts/check-pins.sh` clean; `cargo deny check` passes; git-cliff renders
  notes from the repository history; `scripts/smoke.sh` passes against the real release binary
  (`kb 0.1.0`); the 12 release-mode performance budgets pass with the new release profile.

**Changed against the plan**
- `scripts/package.sh` uses Python's `tarfile`/`gzip`, not GNU tar (same bytes on macOS and Linux).
- The Intel Mac binary is smoke-tested natively on `macos-15-intel`, not under Rosetta, and there is
  no "not smoke-tested" fallback: if that runner is unavailable the release stops (safe, not silent).
- Homebrew is put on the Linux `PATH` by appending its `bin` directory, not by `brew shellenv`.
- The formula uses `license any_of: ["MIT", "Apache-2.0"]` (Homebrew rejects the SPDX expression
  `MIT OR Apache-2.0`, checked with `brew ruby`); `cliff.toml` ignores pre-release tags so a stable
  release's notes cover everything since the last stable one.
- Zig 0.14.1 with cargo-zigbuild 0.23.4 for the musl builds (versions pinned in the workflows).

**Not verified (needs a push, a public repository, or both)**
- The musl + Zig build, QEMU/container smoke, Intel runner, release creation and re-run idempotency,
  the formula pull request and its dispatched CI, attestations, branch protection, Homebrew install,
  and the Linux test suite (never run on Linux). CI times and billed minutes (T045).

**Still open**: T006, T007, T008, T015, T016, T023, T036, T037, T040, T044, T045,
T050, T055, T056, T060, T061.
