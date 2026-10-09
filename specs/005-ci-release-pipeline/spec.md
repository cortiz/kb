# Feature Specification: CI and Release Pipeline

**Feature Branch**: `005-ci-release-pipeline`
**Created**: 2026-10-09
**Status**: Draft
**Milestone**: infrastructure (between M4 and M5; no product behavior changes)
**Depends on**: the existing CI workflow (`.github/workflows/ci.yml`), constitution principles V, VIII and the Distribution constraint
**Input**: User description: "Build a GH CI pipeline to PR, Release release on tags vXXX use rust industry standards"

## Overview

kb has a single CI workflow that runs on pushes to `main` and on pull requests. It has never been run on GitHub: the repository's pushes have been blocked, so Linux has only been tested through pure-function tests on a Mac. This feature turns that workflow into a proper pipeline with two jobs for the maintainer:

1. **Pull requests are gated** by the checks the Rust community treats as standard, on macOS and Linux, so nothing merges that breaks formatting, lints, tests, documentation, the minimum supported Rust version, or dependency hygiene.
2. **Version tags publish a release**: pushing a tag such as `v0.2.0` builds the binary for every platform the constitution promises (macOS arm64 and x86_64, Linux x86_64 and aarch64), attaches archives and checksums to a GitHub release, and fills in release notes.

The constitution says a milestone is not done until CI is green on both macOS and Linux (principle VIII). This pipeline is what makes that statement checkable.

## Clarifications

### Session 2026-10-09

- Q: Should a release also publish the crate to crates.io? → A: No. Releases ship archives only; `cargo install` from the git repository still works for anyone who wants to build.
- Q: Should a release also update a Homebrew tap? → A: Yes if possible, served from this same GitHub repository (no separate tap repository), usable once the repository is public.

- Q: Which branching model does the project use? → A: Trunk-based: `main` is the trunk and is always releasable; work happens on short-lived branches merged through pull requests with squash merges; releases are tags on `main`; no long-lived release or development branches.

## User Scenarios & Testing *(mandatory)*

### User Story 1 — A pull request cannot merge if it breaks the standards (Priority: P1)

Carlos opens a pull request. Within minutes GitHub shows a set of checks. Each one passes or fails with a log that says what to fix. If any required check fails, the branch protection rule blocks the merge.

**Why this priority**: this is the safety net for everything else. M4's "done" depends on a green run on both operating systems, and no run has ever happened.

**Independent Test**: open a pull request from a branch with a deliberate formatting error, then a lint error, then a failing test, then a broken doc link; each must turn a specific check red and name the problem. A clean branch must turn every check green.

**Acceptance Scenarios**:

1. **Given** a pull request, **When** it is opened or updated, **Then** these checks run: formatting, lints with warnings as errors, the full test suite on macOS and on Linux, the documentation build with warnings as errors, a build on the oldest supported Rust version (1.88), and a dependency check (known vulnerabilities, disallowed licenses, duplicate or yanked crates).
2. **Given** a change that only edits documentation or other non-code files, **Then** the pipeline still reports every required check (as passed or skipped) so the merge is never blocked waiting for a check that never ran.
3. **Given** a new push to the same pull request while checks are running, **Then** the older run is cancelled.
4. **Given** the lockfile is out of date with `Cargo.toml`, **Then** the pipeline fails instead of silently updating it.
5. **Given** a failing check, **Then** its log shows the first error without scrolling, and test failures show the failing test's output.

---

### User Story 2 — A version tag publishes a release (Priority: P1)

Carlos pushes the tag `v0.2.0`. The pipeline builds kb for macOS (arm64 and x86_64) and Linux (x86_64 and aarch64), and creates a GitHub release named `v0.2.0` with one archive per platform, a checksum file and generated notes. Anyone can then download the archive for their machine and run `kb`.

**Why this priority**: the constitution promises release tarballs for these four platforms; without this there is no way to install kb except building it.

**Independent Test**: run the pipeline by hand in dry-run mode (builds, smoke-tests and packages everything, publishes nothing) and check the four archives; then push a pre-release tag such as `v0.1.0-rc.1` on `main` (after bumping the version there) and check the pre-release that appears, then delete it. Releases only ever come from `main`, so there is no throw-away branch.

**Acceptance Scenarios**:

1. **Given** a tag matching `v` followed by a semantic version (`v1.2.3`, `v1.2.3-rc.1`), **When** it is pushed, **Then** the release pipeline starts. Tags that do not match (`latest`, `v1`, `release-3`) do nothing.
2. **Given** the tag's version differs from the version in `Cargo.toml`, **Then** the release fails before building anything and says which two versions disagree.
3. **Given** the release pipeline starts, **Then** it first runs the same checks as a pull request on the tagged commit; no archive is built from a commit that fails them.
4. **Given** the checks pass, **Then** one archive per platform is built: `kb-<version>-<target>.tar.gz`, each containing the `kb` binary, the README and the license files, with the executable bit set.
5. **Given** all archives are built, **Then** a `SHA256SUMS` file covering every archive is attached, and a build provenance attestation is recorded for each archive (once the repository is public) so users can verify where it came from.
6. **Given** a tag with a pre-release suffix (`-rc.1`, `-beta.2`), **Then** the GitHub release is marked as a pre-release.
7. **Given** the release is created, **Then** its notes list the changes since the previous release, grouped by Conventional Commit type (the project's commit style).
8. **Given** a release already exists for the tag (the tag was re-pushed or the run was re-run), **Then** the run does not create a second release; it either updates the same one or stops with a clear message.

---

### User Story 3 — The release binaries are the ones that were tested (Priority: P2)

A user downloads `kb` from a release. It starts on a plain machine of that platform without installing anything else, and it is the same code that passed the checks.

**Why this priority**: a release that crashes on a clean machine, or that was built differently from what was tested, defeats the pipeline.

**Independent Test**: on a clean machine or container of each platform, download and unpack the archive, run `kb --version` and `kb init` in a temporary directory, then `kb search` and `kb doctor`.

**Acceptance Scenarios**:

1. **Given** a Linux archive, **Then** the binary has no dependency on a particular system library version (it runs on any mainstream distribution of that architecture, including minimal containers).
2. **Given** each archive, **Then** the pipeline runs `kb --version` and a short smoke test (`init`, `lint`, `search`, `doctor`) on the freshly built binary before attaching it; if the smoke test fails, no release is created.
3. **Given** a release build, **Then** it is built with optimizations, with the lockfile frozen, and with debug information stripped, so the archive is small and reproducible from the same commit.
4. **Given** the same commit built twice, **Then** the two archives have identical content except for timestamps recorded by the archive format (checked on at least one platform).

---

### User Story 4 — The pipeline stays fast and cheap (Priority: P2)

Pull request feedback arrives quickly and the free CI minutes last.

**Why this priority**: a slow pipeline gets bypassed.

**Acceptance Scenarios**:

1. **Given** a pull request that changes a few lines, **When** its checks run with a warm cache, **Then** every required check finishes in 10 minutes or less.
2. **Given** several jobs need compiled dependencies, **Then** they share a build cache keyed by lockfile and toolchain.
3. **Given** the timing tests (the performance budgets), **Then** they run on pushes to `main` and on a schedule, not on every pull request, because shared runners are noisy; a failure there opens or updates an issue instead of blocking merges.

---

### User Story 5 — Dependencies and the pipeline itself stay up to date (Priority: P3)

Security advisories and new versions of dependencies and of the actions used by the pipeline are noticed without anyone remembering to look.

**Why this priority**: it keeps the supply chain healthy at no ongoing effort.

**Acceptance Scenarios**:

1. **Given** a new RustSec advisory affecting a dependency, **Then** a scheduled run (at least weekly) reports it, even when no pull request is open.
2. **Given** a newer version of a Cargo dependency or of a GitHub Action, **Then** an automated pull request proposes the update, grouped so there is not one per crate.
3. **Given** the pipeline uses third-party actions, **Then** each is pinned to a full commit hash (with the version in a comment), and the workflow's default permissions are read-only, with write access granted only to the jobs that create a release.

---

### User Story 6 — Install with Homebrew from this repository (Priority: P3)

Once the repository is public, Carlos (or anyone) runs `brew tap cortiz/kb https://github.com/cortiz/kb` and `brew install kb`, on macOS or Linux, and gets the release binary for their machine. Each release keeps the formula current.

**Why this priority**: it is the install path the constitution names, and it adds convenience, not capability: the archives already work without it.

**Independent Test**: after a release, check that the formula in the repository points at the new version's archives with the right checksums; on a machine with Homebrew and a public copy of the repository, `brew install` and `kb --version` give the new version.

**Acceptance Scenarios**:

1. **Given** a release finishes, **Then** the formula file in the repository's `Formula/` directory is updated to that version, with the download address and SHA-256 of each platform's archive, taken from the release that was just built.
2. **Given** the formula is updated, **Then** the change reaches the default branch through a pull request that the pipeline opens (so branch protection is respected), and is never pushed straight to `main`.
3. **Given** a pre-release tag, **Then** the formula is not changed: Homebrew users only see stable versions.
4. **Given** a Mac (arm64 or Intel) or a Linux machine (x86_64 or arm64), **Then** the formula selects the matching archive.
5. **Given** the formula changes, **Then** the pull request's checks include `brew audit` and `brew test` for the formula, run on macOS and Linux, so a broken formula cannot merge.
6. **Given** the repository is still private, **Then** the pipeline still produces and checks the formula; installing from it needs a GitHub token until the repository is public, and the README says so.

---

### User Story 7 — Trunk-based workflow is enforced where it can be (Priority: P2)

Carlos works on short-lived branches and merges each one to `main` as a single squashed commit. `main` is always in a state that could be released. Nothing but a pull request changes `main`.

**Why this priority**: the release pipeline assumes that any commit on `main` that passes the checks can be tagged. Squash merges give one commit per change, whose message becomes the release note line, so the pull request title has to follow the project's commit style.

**Acceptance Scenarios**:

1. **Given** a pull request, **Then** a required check verifies its title follows Conventional Commits (`feat(scope): …`, `fix: …`, `docs: …`, with `!` or a `BREAKING CHANGE` footer for breaking changes), because the squash commit takes the title.
2. **Given** the repository settings, **Then** merging is limited to squash merges (no merge commits, no rebase merges), merged branches are deleted automatically, and auto-merge is available so a pull request can merge itself when its checks pass.
3. **Given** a branch that is far behind `main`, **Then** the pipeline warns when a pull request is more than 7 days old or more than 50 commits behind, as a nudge to keep branches short-lived (a warning, not a failure).
4. **Given** the repository is private on a plan without branch protection, **Then** the documentation lists the protection to enable as soon as it is available (require a pull request, require the named checks, require branches up to date, linear history, no force pushes, no deletion of `main`), and the pipeline's checks work either way.
5. **Given** the maintainer works alone, **Then** the rules do not require another person's approval; they require the checks.

---

### Edge Cases

- **Fork pull requests**: they run with read-only permissions and no secrets; the checks must still work without any secret.
- **Tag pushed from a commit not on `main`**: the release pipeline refuses (or marks the release as a pre-release and says so) because releasing unreviewed code is a mistake.
- **A job fails on one platform only**: the other platforms' results are still shown; the run as a whole is red.
- **Flaky test**: a failing test is not retried automatically; a retry hides real races. A maintainer can re-run a job by hand.
- **Runner images change** (new macOS version, new Ubuntu): the pipeline pins operating-system versions it depends on for releases, so a new image cannot silently change release binaries.
- **Cross-compilation needs a tool that is missing**: the build fails with the tool's name in the message, not a linker error.
- **Release notes with no conventional commits** since the last tag: the notes say "No user-facing changes" instead of being empty.
- **First release ever**: there is no previous tag; the notes cover the whole history.
- **Branch protection**: the list of required checks is documented, because GitHub only enforces check names the repository owner selects.
- **Disk or time limits**: the full build of dependencies (bundled SQLite, syntax highlighting) must fit the runner's limits on every platform, including the slower emulated or cross-built Linux aarch64.

## Requirements *(mandatory)*

### Pull request pipeline

- **FR-001**: The pipeline MUST run on every pull request and on every push to `main`, and MUST support being started by hand.
- **FR-002**: It MUST run these checks, each as a separately named status so branch protection can require them: formatting (`cargo fmt --check`), lints with warnings denied (`cargo clippy --workspace --all-targets -- -D warnings`), tests (`cargo test --workspace`) on Linux and macOS, documentation with warnings denied, a test run on Rust 1.88 (the minimum supported version), and a dependency check.
- **FR-003**: All cargo commands in the pipeline MUST use the lockfile as is (`--locked`), so a drifting lockfile fails the run.
- **FR-004**: The dependency check MUST fail the run for known vulnerabilities (RustSec), disallowed licenses, yanked versions and dependencies from unknown registries or git sources, using a policy file in the repository (`deny.toml`).
- **FR-005**: A new push to a pull request MUST cancel the superseded run of the same pull request; runs on `main` and on tags MUST NOT be cancelled.
- **FR-006**: The Linux tests MUST include the pseudo-terminal tests, which need `perl`, `pgrep`, `kill` and `stty`; the job MUST verify these exist before running and name any that is missing.
- **FR-007**: Every job MUST have a timeout, so a hung test cannot hold a runner for hours.
- **FR-008**: The performance-budget tests MUST run on pushes to `main` and on a weekly schedule, not on pull requests; a failure MUST open or update a single tracking issue instead of marking a pull request red.
- **FR-009**: Build caches MUST be used and keyed by the lockfile and the toolchain version.
- **FR-010**: The pipeline MUST work for pull requests from forks without any secret.

### Release pipeline

- **FR-011**: The release pipeline MUST start when a tag matching `v<major>.<minor>.<patch>` with an optional `-<pre-release>` suffix is pushed, and for no other tag.
- **FR-012**: Before building, it MUST verify that the tag (without the `v`) equals the workspace version in `Cargo.toml`, and that the tagged commit is reachable from `main`; otherwise it fails with a message naming the problem.
- **FR-013**: It MUST run the full pull request checks on the tagged commit and MUST NOT publish anything if they fail.
- **FR-014**: It MUST build release binaries for four targets: macOS arm64 (`aarch64-apple-darwin`), macOS x86_64 (`x86_64-apple-darwin`), Linux x86_64 and Linux aarch64. Linux binaries MUST be statically linked so they run on any distribution of that architecture.
- **FR-015**: Each binary MUST be built in release mode with the lockfile frozen, then smoke-tested (`kb --version` reports the tag's version; `kb init`, `lint`, `search` and `doctor` succeed in a temporary directory) before it is packaged. A binary that cannot run on its build machine's platform (a cross-built one) MUST be smoke-tested on a machine or emulator of the target platform, or the release MUST say that platform was not smoke-tested.
- **FR-016**: Each archive MUST be named `kb-<version>-<target>.tar.gz` and contain the `kb` executable, `README.md` and the license files in a single top-level directory of the same name.
- **FR-017**: The release MUST attach all archives and a `SHA256SUMS` file, and, when the repository is public, a provenance attestation for each archive. (GitHub offers artifact attestations only for public repositories on the free, Pro and Team plans; while the repository is private the attestation step is skipped, and the run summary says so.)
- **FR-018**: The release MUST be created as a GitHub release named after the tag, marked as a pre-release when the tag has a pre-release suffix, with generated notes grouped by Conventional Commit type since the previous release tag (or the whole history for the first one).
- **FR-019**: Re-running the pipeline for the same tag MUST NOT create a second release or duplicate assets; it MUST either update the existing release or stop with a clear message.
- **FR-020**: The release job MUST be the only job with write permission to repository contents and attestations; all other jobs MUST have read-only permissions.

### Maintenance

- **FR-021**: A scheduled run (at least weekly) MUST run the dependency check against the latest advisory database, with no pull request needed.
- **FR-022**: Automated update proposals MUST exist for Cargo dependencies and for the GitHub Actions the pipeline uses, grouped so one pull request carries related updates.
- **FR-023**: Third-party actions MUST be pinned to full commit hashes, with the human-readable version in a comment.
- **FR-024**: The repository MUST document, in `README.md` or a short `CONTRIBUTING.md`, how to cut a release (bump the version, tag, push), which checks are required for branch protection, and how to run the same checks locally.

### Homebrew formula

- **FR-025**: The repository MUST contain a Homebrew formula for kb in a `Formula/` directory, so that `brew tap <owner>/kb <repository URL>` followed by `brew install kb` works without a separate tap repository.
- **FR-026**: After a stable release (a tag without a pre-release suffix), the pipeline MUST open a pull request that updates the formula to the new version: version, archive addresses for the four targets and their SHA-256 values, all taken from the release's own `SHA256SUMS`. It MUST NOT push to `main` directly.
- **FR-027**: Pre-release tags MUST NOT change the formula.
- **FR-028**: The formula MUST select the archive by operating system and CPU architecture, install the `kb` executable, and have a test that runs `kb --version` and checks it reports the formula's version.
- **FR-029**: A pull request that changes the formula MUST run `brew audit` (strict) and `brew test` for it on macOS and on Linux.

### Trunk-based development

- **FR-030**: `main` MUST be the only long-lived branch. Releases MUST be tags on `main`; the pipeline MUST NOT depend on release or development branches.
- **FR-031**: A required check MUST validate that a pull request's title is a Conventional Commit (types `feat`, `fix`, `docs`, `test`, `refactor`, `perf`, `build`, `ci`, `chore`, `revert`, optional scope, optional `!`).
- **FR-032**: The repository MUST be configured for squash merging only, with the squash commit made from the pull request title and the commit messages as its body, automatic deletion of merged branches, and auto-merge allowed. The documentation MUST list these settings, and the maintainer applies them (or the project provides a one-time script that does).
- **FR-033**: The documentation MUST list the branch protection to turn on for `main` once the host offers it: pull request required, the named checks required, branch up to date before merge, linear history, force pushes and deletion blocked, and no approval count required.
- **FR-034**: A pull request that is old or far behind `main` MUST produce a visible warning (job summary or comment), without failing.

### Key Entities

- **Check**: one named status on a pull request or a commit (format, lint, test-linux, test-macos, docs, msrv, deny, and so on).
- **Release artifact**: an archive for one target, its checksum, and its attestation.
- **Formula**: the Homebrew recipe in `Formula/kb.rb`: version, per-platform download address and checksum, install step and test.
- **Release**: the GitHub release for one version tag, with its notes and assets.
- **Trunk**: the `main` branch, always releasable; every change reaches it as one squashed commit from a short-lived branch.
- **Policy file**: the repository's dependency policy (allowed licenses, banned sources, advisory handling).

## Success Criteria *(mandatory)*

- **SC-001**: A pull request with a clean change shows every required check green within 10 minutes on a warm cache, and 20 minutes on a cold one.
- **SC-002**: A deliberate break of each rule (format, lint, test, doc warning, lockfile drift, an unlicensed dependency) is caught by exactly the check responsible for it.
- **SC-003**: Pushing a version tag produces a complete GitHub release (four archives, checksums, attestations, notes) within 40 minutes, with no manual step.
- **SC-004**: On a clean machine of each of the four platforms, an archive from a release runs `kb --version` and the smoke test successfully.
- **SC-005**: A tag that does not match the crate version, or that points at a commit failing the checks, creates no release and no assets.
- **SC-006**: A release can be cut by someone who has read only the contributor documentation (`CONTRIBUTING.md`, linked from the README): bump the version, commit, tag, push.
- **SC-007**: Over a month with normal activity, no check is skipped silently and no pull request waits on a status that never reports.
- **SC-008**: After a stable release, a pull request updating the formula is open within 15 minutes of the release being published, and merging it makes `brew install kb` (from the public repository) install that version on macOS (arm64) and Linux (x86_64).
- **SC-009**: Every commit on `main` after this feature has a Conventional Commit subject (enforced by the pull request title check), so a release's notes group correctly with no manual editing.

## Out of Scope

- Signing releases with a developer certificate or notarizing the macOS binary (Gatekeeper warnings on download are accepted for now; the archives can be verified through checksums and attestations).
- Publishing the crate to crates.io. The constitution's "`cargo install`" distribution channel is met by `cargo install --git https://github.com/<owner>/kb` (and, as ever, a local `cargo install --path crates/kb`), by the maintainer's decision.
- A separate tap repository (the formula lives in this repository).
- Windows builds (the constitution targets macOS and Linux).
- Container images, system packages (`.deb`, `.rpm`) and installer scripts.
- Deploying anything: kb has no server.
- Changing product behavior; the only repository changes are the pipeline files, the dependency policy, the contributor documentation and, if needed, a license file.

## Assumptions

- The repository is on GitHub with Actions enabled, and the maintainer will enable branch protection and select the required checks, because that setting cannot be changed by a file in the repository.
- The maintainer works alone: pull requests exist to run the checks and to leave one squashed commit per change, not to collect reviews. Branch protection and rulesets are not available on this repository while it is private on the free plan, so the required-check rules in FR-033 cannot be enforced by GitHub until it is public or the plan changes; the checks still run and report.
- Free hosted runners for Linux and macOS are used; the Linux aarch64 build is cross-compiled or built on an available arm runner, whichever the plan finds more reliable.
- The workspace version in `Cargo.toml` is the single source of truth for the release version.
- Releases follow semantic versioning and the project's Conventional Commit style, which is what makes grouped release notes possible.
- The project is dual-licensed or single-licensed under a license file in the repository root; if none exists yet, adding one is part of this work and the license choice is the maintainer's.
- The first push of this repository to GitHub is also the first run of the pipeline. Failures it exposes on Linux (never run so far) are fixed as part of this feature, and the open M4 task T062 ("CI green on macOS and Linux") is satisfied by the first green run.
- The existing `ci.yml` is replaced by the new pull request pipeline; its Intel macOS build check is kept as part of the release matrix.
- Homebrew can tap any Git repository that has a `Formula/` directory when given its URL, so no repository named `homebrew-<something>` is needed. Until the repository is public, downloading release archives (and so installing with Homebrew) needs a GitHub token; the pipeline still generates and checks the formula.
- Pull requests opened by the pipeline need the repository setting that lets Actions create pull requests; the README says to enable it. If the setting is off, the pipeline attaches the new formula to the release and says so in the run summary instead of failing the release.
- The Homebrew formula is checked with Homebrew's own audit and test commands, which are available on the hosted macOS and Linux runners.
