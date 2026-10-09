---
description: "Task list for the CI and Release Pipeline"
---

# Tasks: CI and Release Pipeline

**Input**: Design documents from `specs/005-ci-release-pipeline/`
**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md),
[data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: Required for the helper scripts (constitution VIII): each script gets a failing shell test
first (`scripts/tests/test_<name>.sh`), then the script. Workflow YAML cannot run locally, so it is
checked by `actionlint` and `shellcheck` in CI and then by real runs (quickstart scenarios 3–8).
The stories are validated by those runs; each story below says which.

**Organization**: grouped by user story. US1 (the pull request gate) is the MVP and also closes M4's
open task T062 when its first run on GitHub is green. US7 (trunk-based) is small and rides on US1's
`ci.yml`. US2/US3 are the release; US6 the formula; US4/US5 cost and maintenance.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, no dependency on an incomplete task)
- **[Story]**: US1 to US7
- **[needs approval]**: changes something outside the working tree (GitHub settings, the remote, a
  push, a tag). It is done only after the maintainer says so in that conversation turn; until then
  the task stays open and the work continues around it.

## Path Conventions

Workflows in `.github/workflows/`, scripts in `scripts/`, script tests in `scripts/tests/`, policy
files at the repository root. FR/SC numbers refer to [spec.md](spec.md); R-numbers to
[research.md](research.md).

## Ground rules for every task

- **Pin every third-party action to a full commit hash** with the version in a comment, e.g.
  `uses: Swatinem/rust-cache@<40 hex>  # v2.9.2` (FR-023). Resolve a hash with
  `gh api repos/<owner>/<repo>/commits/<tag> --jq .sha` (this works for annotated tags as well).
  Versions known at planning time: `actions/checkout` v7.0.1, `actions/upload-artifact` v7.0.2,
  `actions/download-artifact` v8.0.2, `actions/attest-build-provenance` v4.2.2,
  `Swatinem/rust-cache` v2.9.2, `EmbarkStudios/cargo-deny-action` v2.1.1,
  `taiki-e/install-action` v2.87.26, `mlugg/setup-zig` v2.2.1; re-check at implementation time.
- **Every workflow starts with `permissions: contents: read`**, every job has `timeout-minutes`, and
  untrusted text (pull request titles, branch names) reaches scripts only through `env:` and quoted
  arguments, never inside `run:` expressions.
- **Scripts**: Bash, `set -euo pipefail`, errors as `error: …` on stderr, `shellcheck`-clean.
- **No secrets**: only the default `GITHUB_TOKEN` with per-job permissions.
- Do not run anything that publishes, tags, pushes or changes GitHub settings without the
  `[needs approval]` go-ahead. Commit nothing until asked.

---

## Phase 1: Setup

**Purpose**: the files every later task depends on, and the one decision that makes the first pull
request red if skipped.

- [X] T001 Add the license: `LICENSE-MIT` (MIT text, `Copyright (c) 2026 Carlos Ortiz`) and `LICENSE-APACHE` (the standard Apache-2.0 text) at the repository root; add `license = "MIT OR Apache-2.0"` to `[workspace.package]` in `Cargo.toml` and `license.workspace = true` to `crates/kb-core/Cargo.toml` and `crates/kb/Cargo.toml`. This is the plan's default (R15); if the maintainer chooses another license, change it here before anything else, because `cargo-deny` reports unlicensed crates and the first pull request would be red.
- [X] T002 Add `[profile.release]` to `Cargo.toml` (same file as T001, so after it): `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"`; leave `panic` at its default (`unwind`) because `crates/kb/src/main.rs` uses `catch_unwind`. Check `cargo build --release --locked` and `cargo test --release -p kb --test perf -- --ignored --test-threads=1` still pass.
- [X] T003 [P] Create the script test harness: `scripts/tests/run.sh` (runs every `scripts/tests/test_*.sh`, prints one line per test, exits non-zero if any fails) and `scripts/tests/lib.sh` (functions `assert_eq <want> <got> <msg>`, `assert_exit <code> <msg> -- <command…>`, `assert_contains <needle> <text> <msg>`, and `new_tmp` that makes a temp dir removed on exit).
- [X] T004 [P] Create `deny.toml` as in research R5: `[advisories]` `version = 2`, `yanked = "deny"`; `[licenses]` `version = 2`, `allow = ["MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib", "Unlicense", "Unicode-3.0", "BSL-1.0", "0BSD", "CC0-1.0", "MPL-2.0"]`, `confidence-threshold = 0.93`; `[bans]` `multiple-versions = "warn"`, `wildcards = "deny"`; `[sources]` `unknown-registry = "deny"`, `unknown-git = "deny"`, `allow-registry = ["https://github.com/rust-lang/crates.io-index"]`. If `cargo-deny` is installable locally, run `cargo deny check` and make it pass; otherwise it is first validated by the `deny` job in T012.
- [X] T005 [P] Create `cliff.toml` for git-cliff (R16): conventional commits only (`conventional_commits = true`, `filter_unconventional = true`), tag pattern `^v[0-9]+\.[0-9]+\.[0-9]+`, no header or footer, groups in this order with these parsers: `Breaking changes` (breaking = true), `Features` (`^feat`), `Fixes` (`^fix`), `Performance` (`^perf`), `Documentation` (`^docs`), `Other` (`^(refactor|build|ci|chore|test|revert)`); skip commits matching `^chore\(release\)`; `chore(deps)` commits stay, under Other. An empty result is rendered as `No user-facing changes.` by the release job (T034), not by the config.

**Checkpoint**: `cargo build --workspace --locked` and `scripts/tests/run.sh` (empty suite) pass.

---

## Phase 2: Foundational (blocks the pull request run and everything after)

**Purpose**: a working remote and an empty-but-valid starting point on GitHub.

- [ ] T006 [needs approval] Make `git push` work: run `gh auth setup-git`, then `git remote set-url origin https://github.com/cortiz/kb.git` (research R21; the SSH key is rejected today). Verify with `git ls-remote origin` (it must list refs, not `Permission denied`). Do not push yet.
- [ ] T007 [needs approval] Publish the existing history so Actions has something to run: `git push origin main` (this sends all of M1–M4, about 20 commits, to the private repository `cortiz/kb`). It is outward-facing and irreversible in practice; do it only on the maintainer's explicit go-ahead.
- [ ] T008 [needs approval] Apply the repository settings for trunk-based merging with the `gh api -X PATCH repos/cortiz/kb …` call in [contracts/repo-settings.md](contracts/repo-settings.md) (squash only, PR title + commit messages, delete branch on merge, auto-merge allowed). Verify with the `gh api repos/cortiz/kb --jq …` line from the same file. Branch protection is documented there and cannot be enabled while the repository is private on the free plan.

**Checkpoint**: `gh api repos/cortiz/kb --jq .default_branch` is `main` and a branch can be pushed.

---

## Phase 3: User Story 1 — A pull request cannot merge if it breaks the standards (P1) — MVP

**Goal**: every pull request runs the standard Rust checks on macOS and Linux and ends in one
`ci-ok` status (spec US1, FR-001 to FR-010).

**Independent test**: open a pull request from a clean branch (all checks green), then break each
rule on throw-away branches (format, lint, test, doc warning, lockfile drift, an unlicensed
dependency) and see exactly the responsible check go red (SC-002).

### Tests first

- [X] T009 [P] [US1] `scripts/tests/test_ci_local.sh`: with a fake `cargo` earlier on `PATH` that records its arguments and can be told to fail a given subcommand, assert `scripts/ci-local.sh` runs, in order, `fmt --all --check`, `clippy --workspace --all-targets --locked -- -D warnings`, `test --workspace --locked`, `doc --workspace --no-deps --locked` (with `RUSTDOCFLAGS` containing `-D warnings`), skips the optional steps (`cargo deny check`, `actionlint`, `shellcheck`) with a "skipped (not installed)" line when the tool is missing, and exits 1 naming the first failing step (SC-002, FR-024). It must fail now because the script does not exist.

### Implementation

- [X] T010 [US1] `scripts/ci-local.sh` (contract: [contracts/scripts.md](contracts/scripts.md)): the steps above, each printed as `==> <name>` and `ok`/`FAILED`, so a developer runs the same commands as CI. T009 goes green.
- [X] T011 [US1] Replace `.github/workflows/ci.yml` (this and T012–T015 edit the same file, so they are sequential). Header: `name: CI`; triggers `pull_request` with `types: [opened, synchronize, reopened]` (deliberately not `edited`, R1), `push` on `main`, `workflow_dispatch`, `workflow_call`; `permissions: contents: read`; `env: CARGO_TERM_COLOR: always` and `CARGO_INCREMENTAL: 0`; `concurrency: group: ci-${{ github.workflow }}-${{ github.ref }}` with `cancel-in-progress: ${{ github.event_name == 'pull_request' }}` (FR-005). Jobs `fmt`, `clippy` (both `--locked`) and `docs` (`RUSTDOCFLAGS: -D warnings`), each on `ubuntu-24.04`, `timeout-minutes: 15`, installing Rust with `rustup toolchain install stable --profile minimal -c rustfmt,clippy` and `rustup default stable` (R2), using the pinned `Swatinem/rust-cache` with `save-if: ${{ github.ref == 'refs/heads/main' }}` (R3).
- [X] T012 [US1] Add to `ci.yml`: job `test` named `test (${{ matrix.os }})` with `matrix.os: [ubuntu-24.04, macos-15]`, `fail-fast: false`, `timeout-minutes: 30`. On Linux the first step is `command -v perl pgrep kill stty sed` printing which is missing (FR-006); then `cargo test --workspace --locked` (FR-003). Add job `msrv` named `msrv (1.88)` (`rustup toolchain install 1.88 --profile minimal`, `cargo +1.88 test --workspace --locked`) and job `deny` using the pinned `EmbarkStudios/cargo-deny-action` with `command: check` (FR-004).
- [X] T013 [US1] Add to `ci.yml`: job `workflows` (`ubuntu-24.04`, 10 minutes): install `actionlint` with the pinned `taiki-e/install-action`, run `actionlint`, then `shellcheck scripts/*.sh scripts/tests/*.sh` (preinstalled on the image) and `scripts/tests/run.sh`. Add job `ci-ok` named `ci-ok`: `needs:` every other job in the file, `if: always()`, and a step that exits 1 when `contains(needs.*.result, 'failure') || contains(needs.*.result, 'cancelled')` and 0 otherwise, so `skipped` counts as passed (R1, data-model "Check").
- [X] T014 [US1] Run `actionlint` and `shellcheck` locally if they can be installed without side effects (`brew install actionlint shellcheck` changes the machine, so ask first); otherwise rely on the `workflows` job. Fix every finding. Check by reading that every `uses:` is pinned to 40 hex characters and every job has `timeout-minutes`.
- [ ] T015 [US1] [needs approval] Open the first pull request: push a branch `ci/pipeline` to origin and `gh pr create --title "ci: replace the CI workflow with the standard pipeline"`. Watch with `gh pr checks --watch`. **Expect real failures on Linux** (it has never run; spec Assumptions): fix each in this pull request, in the code or the tests, with the failing log quoted in the commit body. Done when `ci-ok` is green; that run also satisfies M4's T062 (record the run URL there).
- [ ] T016 [US1] Verify SC-002 and FR-003 by throw-away pull requests or pushes to a scratch branch (never merged): (a) a misformatted line → `fmt` red; (b) an unused variable → `clippy` red; (c) a failing assertion → `test` red on both OSes; (d) a broken intra-doc link → `docs` red; (e) `Cargo.toml` dependency added without updating `Cargo.lock` → every `--locked` job red; (f) a dependency with a disallowed license or a git source → `deny` red. Record the result of each in the pull request description, then delete the branch. Also check a second push cancels the first run (FR-005) and that a fork-style run needs no secret (the workflow has none).

**Checkpoint**: a clean pull request shows every job and `ci-ok` green within 10 minutes warm / 20 cold (SC-001); each deliberate break turns exactly its job red.

---

## Phase 4: User Story 7 — Trunk-based workflow is enforced where it can be (P2)

**Goal**: Conventional Commit pull request titles, stale-branch warnings, and the documented settings
(spec US7, FR-030 to FR-034).

**Independent test**: titles `feat(copy): add --method` and `fix: x` pass; `Update stuff`, `feat: ` and a 101-character title fail; an old pull request shows the warning but stays green.

### Tests first

- [X] T017 [P] [US7] `scripts/tests/test_check_pr_title.sh`: a table of accepted titles (`feat: x`, `fix(core): y`, `docs(spec)!: z`, `chore(release): 0.2.0`, `revert: a`, `feat(scope.with-dots/slash): b`) and rejected ones (`Update stuff`, `feat:` , `feat: ` (empty subject), `feat(): x`, `Feat: x`, `feat : x`, `wip: x`, a 101-character title, a title ending in `.`, an empty argument, no argument), asserting exit 0/1 and that the failure message names the expected form `type(scope): subject` with the allowed types (FR-031). Also a title containing `$(touch /tmp/pwned)` and a backtick, asserting the file is not created. Fails now.
- [X] T018 [P] [US7] `scripts/tests/test_pr_staleness.sh`: with a fake `gh` on `PATH` that answers `gh api repos/o/r/pulls/7` with a JSON `created_at` and `gh api repos/o/r/compare/main...HEAD` with `behind_by`, assert: 3 days / 10 behind → no warning and exit 0; 8 days → warning text containing "7 days"; 51 commits behind → warning containing "50 commits"; both → both; a `gh` that fails → exit 0 with a note (never blocks, FR-034); with `GITHUB_STEP_SUMMARY` set the warning is appended to that file. Fails now.

### Implementation

- [X] T019 [US7] `scripts/check-pr-title.sh <title>`: regex `^(feat|fix|docs|test|refactor|perf|build|ci|chore|revert)(\([a-z0-9._/-]+\))?!?: .+` (R7), reject titles over 100 characters or ending in `.`, print `error: expected type(scope): subject, with type one of feat, fix, docs, test, refactor, perf, build, ci, chore, revert` on failure. The title is only ever read from `$1`. T017 goes green.
- [X] T020 [P] [US7] `scripts/pr-staleness.sh <owner/repo> <pr-number>` (R8): creation time and `behind_by` through `gh api`; warn when older than 7 days or 50 or more commits behind, writing to `$GITHUB_STEP_SUMMARY` when set and printing `::warning::…`; always exit 0 (exit 2 only for wrong arguments). T018 goes green.
- [X] T021 [US7] `.github/workflows/pr-title.yml` (contract: [contracts/workflows.md](contracts/workflows.md)): name `PR title`, `on: pull_request` with `types: [opened, edited, synchronize, reopened]`, `permissions: contents: read`, one job `pr-title` (`ubuntu-24.04`, `timeout-minutes: 2`) that checks out the repository and runs `scripts/check-pr-title.sh "$TITLE"` with `env: TITLE: ${{ github.event.pull_request.title }}`.
- [X] T022 [US7] Add job `pr-staleness` to `.github/workflows/ci.yml` (after T016, same file): `if: github.event_name == 'pull_request'`, `permissions: pull-requests: read`, runs `scripts/pr-staleness.sh "$GITHUB_REPOSITORY" "$PR"` with `GH_TOKEN: ${{ github.token }}` and `PR: ${{ github.event.pull_request.number }}`; add it to `ci-ok`'s `needs`.
- [ ] T023 [US7] Verify on a real pull request: retitle `ci: x` → `Fix stuff` → `pr-title` red and only that workflow re-runs (no test jobs start); retitle back → green (FR-031, R1).

**Checkpoint**: `pr-title` is a separate status that re-runs on a title edit and nothing else does.

---

## Phase 5: User Story 2 — A version tag publishes a release (P1)

**Goal**: pushing `vX.Y.Z` runs the checks, builds the four targets, and publishes a complete GitHub release (spec US2, FR-011 to FR-020).

**Independent test**: a dry-run dispatch produces the five release files as a workflow artifact and publishes nothing; then a release candidate tag on `main` produces a pre-release with five assets and notes (quickstart 4 and 5).

### Tests first

- [X] T024 [P] [US2] `scripts/tests/test_check_release_tag.sh`: against a temp git repository with a fake workspace (`Cargo.toml` with `[workspace.package] version = "0.2.0"`, `crates/x/Cargo.toml` inheriting it) and a `main` branch: `v0.2.0` on `main` → exit 0, prints `tag=v0.2.0`, `version=0.2.0`, `prerelease=false`; `v0.2.0-rc.1` with the manifest at `0.2.0-rc.1` → `prerelease=true`; `v0.3.0` → exit 1 and a message containing both `v0.3.0` and `0.2.0`; syntax rejects `latest`, `v1`, `release-3`, `v1.2.3abc`, `v01.2.3`, `1.2.3`; a tag on a commit not reachable from `origin/main` → exit 1 "not on main"; `--dry-run` with no tag reads the version from `Cargo.toml`, prints `tag=v0.2.0`, and skips the main check (contract: scripts.md). Fails now.
- [X] T025 [P] [US2] `scripts/tests/test_package.sh`: given a fake `kb` file, a temp directory holding `README.md`, `LICENSE-MIT`, `LICENSE-APACHE`, and `SOURCE_DATE_EPOCH=1700000000`: `scripts/package.sh 0.2.0 x86_64-unknown-linux-musl <kb> <out>` creates `<out>/kb-0.2.0-x86_64-unknown-linux-musl.tar.gz` whose listing is exactly one top-level directory `kb-0.2.0-x86_64-unknown-linux-musl/` with `kb` (mode 0755), `README.md`, `LICENSE-MIT`, `LICENSE-APACHE` (0644), owner 0:0, mtime 1700000000; running it twice gives byte-identical archives (compare `sha256sum`); `<out>/SHA256SUMS` has one `<64 hex>  <file>` line per archive, sorted by file name, and `sha256sum -c` accepts it after two targets were packaged; a missing license file or README makes it exit 1 naming the file (FR-016). Fails now.
- [X] T026 [P] [US2] `scripts/tests/test_smoke.sh`: with a fake `kb` shell script that implements `--version`, `init`, `lint`, `search`, `doctor --json` (and one variant that fails a chosen step): `scripts/smoke.sh <kb> 0.2.0` exits 0 and prints `smoke: ok`; a wrong version, a failing `init`, a `search` that prints no ID, and a `doctor` whose JSON has `"ok": false` each exit 1 and name the failing step; a runner prefix argument (`env`) is applied to every invocation; the script uses a fresh temp `KB_ROOT` and `HOME` (assert the fake saw them) (FR-015). Fails now.

### Implementation

- [X] T027 [US2] `scripts/check-release-tag.sh [--dry-run | <tag>]` (R10): semver regex `^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?$`; workspace version via `cargo metadata --no-deps --format-version 1` and `jq`-free parsing (use `python3 -c` or `sed`; `jq` is not guaranteed locally) and compare; `git merge-base --is-ancestor "$GITHUB_SHA" origin/main` unless dry-run; print `tag=`, `version=`, `prerelease=` lines (so `>> "$GITHUB_OUTPUT"` works). T024 green.
- [X] T028 [P] [US2] `scripts/package.sh <version> <target> <path-to-kb> <outdir>` (R13): stage the directory with `install -m 0755` / `-m 0644`, `tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="@${SOURCE_DATE_EPOCH}" -cf - <dir> | gzip -n -9 > <archive>`, append the checksum with `sha256sum` and keep `SHA256SUMS` sorted. T025 green.
- [X] T029 [P] [US2] `scripts/smoke.sh <kb> <expected-version> [<runner-prefix…>]` with the five steps of [contracts/release.md](contracts/release.md) ("Smoke test"). T026 green.
- [X] T030 [US2] Create `.github/workflows/release.yml` (this and T031–T036 edit it; sequential): name `Release`; `on: push: tags: ['v[0-9]*.[0-9]*.[0-9]*']` and `workflow_dispatch` with boolean input `dry_run` (default `true`); `permissions: contents: read`; `concurrency: group: release-${{ github.ref }}`, `cancel-in-progress: false`. Job `verify` (`ubuntu-24.04`, checkout with `fetch-depth: 0`): runs `scripts/check-release-tag.sh "$GITHUB_REF_NAME"` on a tag event or `--dry-run` on dispatch and exposes `tag`, `version`, `prerelease` as job outputs (FR-011, FR-012). Job `ci`: `needs: verify`, `uses: ./.github/workflows/ci.yml` (FR-013).
- [X] T031 [US2] Add the build matrix to `release.yml` — job `build`, `needs: ci`, `timeout-minutes: 60`, `fail-fast: false`, four entries: `aarch64-apple-darwin` on `macos-15`; `x86_64-apple-darwin` on `macos-15` (`rustup target add`); `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` on `ubuntu-24.04` using the pinned `mlugg/setup-zig` and `cargo-zigbuild` installed with the pinned `taiki-e/install-action` (a prebuilt binary, not `cargo install`) (R11). Build with `cargo build --release --locked --target <t>` (Apple) or `cargo zigbuild --release --locked --target <t>` (Linux), `RUSTFLAGS="--remap-path-prefix=$PWD=/src"`, then upload `target/<t>/release/kb` as artifact `bin-<t>` with the pinned `actions/upload-artifact`. The first run of this job decides whether the musl + Zig plan holds (R11 fallbacks: `cross`, then gnu targets); if it fails, fix or fall back here and note it in the plan's re-check.
- [X] T032 [US2] Add job `smoke` (matrix over the same four targets, `needs: build`, runs on the build's OS): download the artifact, `chmod +x`, run `scripts/smoke.sh ./kb "$VERSION" <prefix>` with prefix empty (native), `arch -x86_64` after `softwareupdate --install-rosetta --agree-to-license || true` (Intel Mac binary), or `qemu-aarch64` after `sudo apt-get install -y qemu-user` (aarch64 Linux). For the Intel Mac binary, if Rosetta cannot be installed or the binary will not run, run the same smoke test on the `macos-15-intel` runner instead (add that runner as the matrix entry for this target); only if that also cannot run, do not fail: write `x86_64-apple-darwin: not smoke-tested (<reason>)` to a `smoke-<target>` artifact that the release job appends to the notes (FR-015, spec edge case). For the two Linux binaries, after the native/QEMU test, also run the binary inside minimal containers on the hosted runner (Docker is preinstalled there): `docker run --rm -v "$PWD:/w:ro" alpine:latest /w/kb --version` and the same with `debian:stable-slim`, with `--platform linux/arm64` for the aarch64 binary (register QEMU with the pinned `docker/setup-qemu-action`), then the full `scripts/smoke.sh` inside one of them (SC-004, US3.1: this automates the clean-machine check that cannot run on a machine without Docker).
- [X] T033 [US2] Add job `package` (`needs: smoke`, `ubuntu-24.04`): checkout, download the four `bin-*` artifacts, set `SOURCE_DATE_EPOCH="$(git log -1 --format=%ct)"`, run `scripts/package.sh` for each target, upload artifact `dist` (archives + `SHA256SUMS`; `retention-days: 7` on a dry run).
- [X] T034 [US2] Add job `release` (`needs: [verify, package]`, `if: github.event_name == 'push'`, `permissions: contents: write, id-token: write, attestations: write`, `timeout-minutes: 15`; R16): download `dist`; build `notes.md` with `git-cliff --latest --strip header -c cliff.toml` (installed through the pinned `taiki-e/install-action`), write `No user-facing changes.` when it is empty, append any `smoke-*` "not smoke-tested" lines; create the release as a draft with `gh release create "$TAG" --draft --verify-tag --title "$TAG" --notes-file notes.md [--prerelease]` or, when `gh release view "$TAG"` succeeds, `gh release edit … --notes-file` (FR-018, FR-019); `gh release upload "$TAG" dist/* --clobber`; if `github.event.repository.private == false` run the pinned `actions/attest-build-provenance` on `dist/*.tar.gz`, otherwise write "attestations skipped: repository is private" to `$GITHUB_STEP_SUMMARY` (FR-017); re-download the assets with `gh release download` and run `sha256sum -c SHA256SUMS`; finally `gh release edit "$TAG" --draft=false`.
- [X] T035 [US2] Run `actionlint`/`shellcheck` over the new workflow and scripts (T014's method), then review the permission blocks against FR-020: only `release` (and later `formula-pr`) may write; `verify`, `build`, `smoke`, `package` are read-only.
- [ ] T036 [US2] [needs approval] Dry run: a workflow can be dispatched only once its file exists on the default branch, so merge the pull request that adds `release.yml` first (its own pull request run covers `workflows`/actionlint), then `gh workflow run release.yml --ref main -f dry_run=true`. Download the `dist` artifact with `gh run download`, check `sha256sum -c SHA256SUMS`, list each archive against the layout in [contracts/release.md](contracts/release.md), and confirm `verify` did not fail on the branch ref (R10) and `release` was skipped. Record the run URL.
- [ ] T037 [US2] [needs approval] Release candidate: in one pull request bump `[workspace.package] version` to `0.1.0-rc.1` (title `chore(release): 0.1.0-rc.1`), merge it, then tag that merge commit `v0.1.0-rc.1` and push the tag. Verify (quickstart 5): the release is a **pre-release** with exactly the five assets; its notes are grouped by commit type; a second run (`gh run rerun`) leaves one release, not two (FR-019); a tag whose version disagrees with `Cargo.toml` (`v0.9.9`, pushed from a scratch clone) stops at `verify` naming both versions and creates nothing (SC-005). Delete the candidate afterwards: `gh release delete v0.1.0-rc.1 --cleanup-tag --yes`.

**Checkpoint**: the dry run and the release candidate both behave as in quickstart 4–5.

---

## Phase 6: User Story 3 — The release binaries are the ones that were tested (P2)

**Goal**: the Linux binary is truly static, runs on a clean machine, and builds reproducibly (spec US3, FR-014, FR-015).

**Independent test**: on a minimal container of each Linux architecture and on a clean Mac, the archive runs `kb --version` and the smoke test.

- [X] T038 [US3] Extend `scripts/smoke.sh` (test first: add a case to `scripts/tests/test_smoke.sh` using a fake `file` command) with a static-link check for Linux targets selected by a `--static` flag: `file "$kb"` must contain `statically linked` (or `static-pie linked`) and must not contain `dynamically linked`; the release `smoke` job passes `--static` for the two musl targets (US3.1).
- [X] T039 [US3] In `.github/workflows/audit.yml` (created in T049; if that task has not run, create the file with only this job and let T049 extend it) add job `reproducible`: build `x86_64-unknown-linux-musl` twice from the same commit into two target directories with `--remap-path-prefix`, `SOURCE_DATE_EPOCH` and `--locked`, and fail when the two binaries' `sha256sum` differ, printing both (US3.4, R13). Run weekly and on dispatch, not per release.
- [ ] T040 [US3] [needs approval] Clean-machine check of the downloaded dry-run (or candidate) archives, the part the pipeline cannot do: on this Mac unpack the arm64 archive and run the five-step smoke test for real, and the x86_64 one under Rosetta (`arch -x86_64`). The Linux containers are checked inside the pipeline (T032); repeat them by hand only if a Docker daemon is running (`docker run --rm -v "$PWD:/w:ro" alpine:latest /w/kb --version`). Record what could not be checked.

**Checkpoint**: a Linux archive is reported statically linked by the pipeline itself, and the reproducibility canary has run once.

---

## Phase 7: User Story 4 — The pipeline stays fast and cheap (P2)

**Goal**: timing budgets leave the pull request path; costs are bounded and visible (spec US4, FR-007 to FR-009, research R9, R12).

**Independent test**: a pull request finishes within 10 minutes warm; a failing performance run produces one tracking issue, not one per failure.

- [X] T041 [P] [US4] `scripts/tests/test_track_issue.sh`: with a fake `gh` that records calls and answers `gh issue list --label L --state open --json number --jq …` from a variable: when no open issue exists, `scripts/track-issue.sh <label> <title> <body-file>` calls `gh issue create` once with the label; when one exists (number 12), it calls `gh issue comment 12` and never `create`; a failing `gh` makes the script exit 1 with its message. Fails now.
- [X] T042 [US4] `scripts/track-issue.sh <label> <title> <body-file>` (find the open issue by label, comment on it, or create it) so `perf.yml` and `audit.yml` share one implementation. T041 green.
- [X] T043 [US4] `.github/workflows/perf.yml` (contract: workflows.md): name `Performance`; `on: push: branches: [main]`, `schedule: - cron: '0 6 * * 1'`, `workflow_dispatch`; matrix `ubuntu-24.04` and `macos-15`; `timeout-minutes: 30`; runs `cargo test --release --locked -p kb --test perf -- --ignored --test-threads=1 --nocapture` with the same Linux prerequisites check; on failure a step with `permissions: issues: write` calls `scripts/track-issue.sh perf-regression "Performance budget failed" <log excerpt file>` with the run URL (FR-008). Remove the old `perf` job from `ci.yml` (it was dropped when T011 replaced the file; confirm it is gone).
- [ ] T044 [US4] Review the cost levers with the maintainer (research R12) and apply the one chosen, as a one-line workflow change each: public repository (nothing to change), `perf.yml` weekly only (delete the `push` trigger; this edits FR-008, so also update the spec line), or `ci.yml` skipping the macOS `test` leg on pushes to `main` (`if: github.event_name != 'push' || matrix.os != 'macos-15'`). Record the decision in the plan's re-check. If nothing is chosen, leave the spec's behavior and say so.
- [ ] T045 [US4] After the first week of real use, read the run times with `gh run list --json name,conclusion,startedAt,updatedAt` and compare to SC-001 (10 min warm / 20 cold) and the estimates in R12; put the measured numbers and the billed-minutes estimate in the plan's re-check. Adjust cache keys or splits if a required check exceeds its budget.

**Checkpoint**: the performance tests no longer run on pull requests, and one issue exists per failing check kind.

---

## Phase 8: User Story 5 — Dependencies and the pipeline stay up to date (P3)

**Goal**: weekly advisory scans and grouped update pull requests (spec US5, FR-021 to FR-023).

**Independent test**: a manual `audit` dispatch passes (or opens one issue); Dependabot opens grouped pull requests whose titles pass `pr-title`.

- [X] T046 [P] [US5] `scripts/tests/test_check_pins.sh`: given temp workflow files, assert `scripts/check-pins.sh <dir>` exits 0 when every `uses:` that is not local (`./`) or a `docker://` reference is `owner/repo[/path]@<40 hex>` followed by a `# vX.Y.Z` comment, and exits 1 naming `file:line` for `@v4`, `@main`, a 39- or 41-character hash, and a missing comment; reusable-workflow `uses: ./.github/workflows/ci.yml` is allowed (FR-023). Fails now.
- [X] T047 [US5] `scripts/check-pins.sh [dir]` (default `.github/workflows`), and call it from the `workflows` job in `ci.yml` (same file as earlier tasks; do it after T022). T046 green; the repository's own workflows must pass it.
- [X] T048 [P] [US5] `.github/dependabot.yml` (R18): `version: 2`; ecosystems `cargo` and `github-actions`, `directory: "/"`, `schedule: interval: weekly, day: monday`, `open-pull-requests-limit: 5`, one `groups:` entry per ecosystem with `patterns: ["*"]` and `update-types: [minor, patch]`, `commit-message: prefix: "chore(deps)"` (so squash titles pass the title check).
- [X] T049 [US5] `.github/workflows/audit.yml`: name `Audit`; `on: schedule: - cron: '0 5 * * 1'`, `workflow_dispatch`; job `advisories` (pinned `EmbarkStudios/cargo-deny-action`, `command: check advisories`, `permissions: issues: write` for the failure step calling `scripts/track-issue.sh security …`) (FR-021). Keep the `reproducible` job from T039 in the same file.
- [ ] T050 [US5] [needs approval] Run `audit` once by dispatch and confirm it passes or opens exactly one issue (a second failing run comments instead of duplicating, quickstart 8); confirm Dependabot's first pull request appears with a title that passes `pr-title`.

**Checkpoint**: `scripts/check-pins.sh` passes in CI and the weekly jobs have run once.

---

## Phase 9: User Story 6 — Install with Homebrew from this repository (P3)

**Goal**: each stable release leaves a correct formula and a pull request to merge it (spec US6, FR-025 to FR-029).

**Independent test**: generate a formula from a canned `SHA256SUMS`, audit it, and (on a public repository) install from it.

### Tests first

- [X] T051 [P] [US6] `scripts/tests/test_gen_formula.sh`: with a canned `SHA256SUMS` holding the four archives for 0.2.0, `scripts/gen-formula.sh v0.2.0 SHA256SUMS <out>` writes a formula whose `version` is `0.2.0`, with exactly four `url` lines of the form `https://github.com/cortiz/kb/releases/download/v0.2.0/kb-0.2.0-<target>.tar.gz` and four matching `sha256` values, inside `on_macos`/`on_linux` and `on_arm`/`on_intel` blocks that pair `aarch64-apple-darwin` with macOS arm, `x86_64-apple-darwin` with macOS Intel, and the two musl targets with Linux arm and Intel; contains `bin.install "kb"` and a `test do` block asserting `kb --version`; a missing checksum for any target exits 1 naming the target; a pre-release version (`0.2.0-rc.1`) exits 1 ("formulas are for stable releases only", FR-027); and `ruby -c <out>` passes when `ruby` is installed (macOS ships it). Also `scripts/tests/test_formula_changed.sh` for `scripts/formula-changed.sh <event> <base-ref> <before> <after>` (prints `formula=true|false`), in a temp git repository: `pull_request` with `Formula/kb.rb` changed against `main` → true, unchanged → false; `push` with a real `before`..`after` range touching `Formula/` → true; `push` with `before` = 40 zeros (a new branch or **a tag push**) → false and no git error; `workflow_dispatch` compares against `origin/main`; an unknown event → false. Fails now.

### Implementation

- [X] T052 [US6] `.github/formula.rb.tmpl` and `scripts/gen-formula.sh <tag> <SHA256SUMS> <out>` (R17, contract: release.md "Formula") and `scripts/formula-changed.sh <event> <base-ref> <before> <after>` (the cases in T051; it never lets a missing or zero `before` fail the job): substitute `@VERSION@` and `@SHA_<TARGET>@` placeholders using `awk`/`sed` over the sums file; set `license "MIT OR Apache-2.0"` (matching T001). T051 green.
- [X] T053 [US6] Add jobs `changes` and `formula` to `ci.yml` (after T047): `changes` runs `scripts/formula-changed.sh` with the event name, `github.base_ref`, `github.event.before` and `github.sha` (checkout `fetch-depth: 0`) and outputs its `formula=` line, so a tag push (the release's `ci` call) yields `false` instead of a git error; `formula` is a matrix over `ubuntu-24.04` and `macos-15` with `needs: changes` and `if: needs.changes.outputs.formula == 'true'`; on Linux it first runs `eval "$(/home/linuxbrew/.linuxbrew/bin/brew shellenv)"`; steps `brew tap cortiz/kb "$GITHUB_WORKSPACE"`, `brew audit --strict kb`, then `brew install kb && brew test kb` only when `github.event.repository.private == false` (otherwise print why and stop after the audit) (FR-029, R17). Add both to `ci-ok`'s `needs`.
- [X] T054 [US6] Add job `formula-pr` to `release.yml` (`needs: release`, `if: needs.verify.outputs.prerelease == 'false' && github.event_name == 'push'`, `permissions: contents: write, pull-requests: write, actions: write`, macOS runner for the brew test): download `SHA256SUMS` with `gh release download`, run `scripts/gen-formula.sh`; if `Formula/kb.rb` is unchanged stop; run `brew audit --strict` and `brew test` on the generated file (Linux needs the `brew shellenv` line); commit on branch `chore/formula-<version>` (identity `github-actions[bot]`), push it, `gh pr create --title "chore(release): kb <version> formula" --body "<audit/test results>"`, then `gh workflow run ci.yml --ref chore/formula-<version>` (R17). If `gh pr create` fails because Actions may not create pull requests, upload the formula to the release as `kb.rb` and write the reason to `$GITHUB_STEP_SUMMARY` without failing the job (spec assumption). Never push to `main` (FR-026).
- [ ] T055 [US6] [needs approval] Allow Actions to open pull requests when the first stable release is about to happen: `gh api -X PUT repos/cortiz/kb/actions/permissions/workflow -f default_workflow_permissions=read -F can_approve_pull_request_reviews=true` (contract: repo-settings.md). Skip it to test the fallback path first (formula attached to the release).
- [ ] T056 [US6] [needs approval] On the first stable release (T060), confirm: the formula pull request exists within 15 minutes (SC-008), its checks ran or the close-and-reopen fallback was needed (record which in the plan's R17), and after merging it `brew tap cortiz/kb https://github.com/cortiz/kb && brew install kb` installs the version (only once the repository is public; until then record "audit only").

**Checkpoint**: a canned formula passes `ruby -c` and `brew audit --strict`; the release job opens a pull request or attaches `kb.rb`.

---

## Phase 10: Polish and cross-cutting

- [X] T057 [P] Write `CONTRIBUTING.md`: the trunk-based workflow (branch from `main`, short-lived, one squashed pull request, titles as Conventional Commits with the allowed types, auto-merge when `ci-ok` is green), the required checks (`ci-ok`, `pr-title`) and the branch protection to switch on when GitHub allows (the table in repo-settings.md), how to run the checks locally (`scripts/ci-local.sh`, `scripts/tests/run.sh`), how to cut a release (bump `version` in a `chore(release): X.Y.Z` pull request, merge, `git tag vX.Y.Z <merge commit>`, `git push origin vX.Y.Z`; what appears; how to retry; how to delete a bad release), and the formula pull request including the close-and-reopen fallback (FR-024, SC-006).
- [X] T058 [P] Update `README.md`: an "Install" section (download an archive, verify with `shasum -a 256 -c --ignore-missing SHA256SUMS`, the Gatekeeper note and `xattr -d com.apple.quarantine`, Homebrew after the repository is public, `cargo install --git` for building), a CI badge, and the status line; link to `CONTRIBUTING.md`.
- [X] T059 Re-run the whole verification once the workflows exist: `scripts/ci-local.sh`, `scripts/tests/run.sh`, `scripts/check-pins.sh`, `actionlint`, and read each workflow against its contract in [contracts/workflows.md](contracts/workflows.md) (job names, permissions, triggers, needs). Fix drift in the workflow or the contract, whichever was wrong.
- [ ] T060 [needs approval] The first stable release: pull request `chore(release): 0.1.0` bumping the version (and nothing else), merge, tag `v0.1.0` on the merge commit, push the tag, and walk quickstart 6 and 7 against the result. Record the run URL, durations (SC-003 target 40 minutes), and anything that failed in the plan's re-check.
- [ ] T061 Close the loop in the earlier milestone: tick M4's `T062` in `specs/004-platform-clipboard/tasks.md` with the green `ci-ok` run URL from T015, and, if the maintainer agrees, note in the constitution's Development Workflow (as a PATCH amendment in its own commit) that "one branch per milestone" means a short-lived branch merged by squash.
- [X] T062 Update this feature's plan with a "Post-implementation re-check" section like the earlier milestones: what changed against the plan (the musl/Zig outcome, whether the dispatched-CI trick satisfied required checks, the cost lever chosen, measured times), what was not verified (branch protection, attestations, Homebrew install while private), and which `[needs approval]` tasks are still open.

---

## Dependencies and execution order

- **Phase 1** first (T001 before T002; both edit `Cargo.toml`). T003–T005 are independent files.
- **Phase 2** needs the maintainer; T006 before T007; T008 is independent of both but is best done before the first pull request merges.
- **US1** needs Phases 1–2: tests (T009) → script (T010) → `ci.yml` (T011–T013 in order) → static check (T014) → first pull request (T015) → rule-by-rule verification (T016). Everything that edits `ci.yml` is sequential: T011, T012, T013, T022, T047, T053.
- **US7** needs `ci.yml` to exist (T013): T017/T018 in parallel, then T019 and T020 in parallel, T021, then T022 (after T016), T023.
- **US2** needs US1's `ci.yml` (it is called by `release.yml`). Tests T024–T026 in parallel → scripts T027–T029 (T028 and T029 in parallel) → workflow T030–T034 in order → T035 → T036 → T037.
- **US3** needs US2's workflow (T038 extends `smoke.sh`; T039 and T049 share `audit.yml`, so whichever runs first creates the file).
- **US4** is independent of the release: T041 → T042 → T043; T044 needs the maintainer; T045 needs a week of data.
- **US5**: T046 → T047; T048 any time; T049 after T039/T042; T050 last.
- **US6** needs US2's release job: T051 → T052 → T053 (after T047) → T054 (after T034) → T055/T056 with the first stable release.
- **Polish** last; T060 needs US2, US3, US6 done, the license decided and the maintainer's go-ahead.
- Same-file serialization summary: `Cargo.toml` (T001, T002); `ci.yml` (T011, T012, T013, T022, T047, T053); `release.yml` (T030–T034, T054); `audit.yml` (T039, T049); `scripts/smoke.sh` (T029, T038).

## Parallel opportunities

- Phase 1: T003 ∥ T004 ∥ T005 after T001/T002.
- Script tests are all independent files and can be written together: T009, T017, T018, T024, T025, T026, T041, T046, T051.
- Scripts that follow them: T019 ∥ T020; T028 ∥ T029; T042; T052 (each in its own file).
- T048 (Dependabot) and T057/T058 (docs) can be done at any point.

## Implementation strategy

- **MVP**: Phases 1–3 (T001–T016): license, profile, script harness, policy files, working remote, and a pull request pipeline that goes green on both operating systems. That alone satisfies the constitution's "CI green on both" gate and M4's T062.
- Then US7 (cheap, shares `ci.yml`), then the release (US2, US3), then cost and maintenance (US4, US5), then Homebrew (US6), then docs and the first release.
- **Honest limits**: the workflows cannot be run before a push; every workflow task is "written and linted", and only the `[needs approval]` runs prove it. The musl + Zig build, the Rosetta smoke test, the dispatched-CI trick for formula pull requests, attestations and branch protection are unverified until then, and the latter two need a public repository.
- **Open decisions** for the maintainer, with the default used until told otherwise: license `MIT OR Apache-2.0` (T001); the minutes lever (T044; default: spec as written).
