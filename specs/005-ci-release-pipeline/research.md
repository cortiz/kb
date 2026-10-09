# Research: CI and Release Pipeline

"Checked" means I looked it up or ran it for this repository; "from practice" means documented or
widely used behavior I could not exercise here (no GitHub runs have ever happened for this repo).

## R1. Workflow layout and the single required check

**Decision.** Five workflows, split by trigger:

| File | Triggers | Purpose |
|---|---|---|
| `ci.yml` | `pull_request`, `push` to `main`, `workflow_dispatch`, `workflow_call` | the checks every change must pass |
| `pr-title.yml` | `pull_request` (opened, edited, synchronize, reopened) | Conventional Commit check on the title, nothing else |
| `release.yml` | `push` of tags `v*`, `workflow_dispatch` (dry run) | verify, run `ci.yml`, build, package, publish, open the formula PR |
| `perf.yml` | `push` to `main`, weekly `schedule`, `workflow_dispatch` | timing budgets; tracking issue on failure |
| `audit.yml` | weekly `schedule`, `workflow_dispatch` | latest advisories; reproducibility check |

`ci.yml` ends in a job `ci-ok` that `needs:` every other job and runs `if: always()`; it fails when
any needed job's result is `failure` or `cancelled` and passes for `success` or `skipped`. Branch protection (when available) requires `ci-ok` and `pr-title`. `ci.yml` does not trigger on
`edited`: a title or body edit would otherwise re-run every job, macOS included, and skipping the
jobs on `edited` is not an option because that run's `ci-ok` would pass with the tests skipped and
could mask a red run on the same commit. The title check is its own workflow for that reason.

**Why.** GitHub requires status checks by name; a job that is skipped by a condition or renamed
leaves a required check "pending" forever. One aggregate name is the standard fix (from practice).

**Alternatives.** One giant workflow: the release and the scheduled jobs would need `if:` guards on
everything. Path filters on `pull_request`: a filtered-out workflow never reports, which is exactly
the "check that never ran" problem in spec US1.2, so every job runs on every pull request.

## R2. Toolchain installation without a setup action

**Decision.** Jobs install Rust with `rustup` (`rustup toolchain install stable --profile minimal
-c rustfmt,clippy` and `rustup toolchain install 1.88 --profile minimal`), which every hosted
runner already has. `rust-toolchain.toml` (already `channel = "stable"` with rustfmt and clippy)
keeps local and CI behavior aligned.

**Why.** It removes a third-party action and the awkwardness of pinning `dtolnay/rust-toolchain`,
whose versions are branch names (`stable`, `1.88`) rather than tags, so a commit-hash pin would
freeze the "stable" it installs. Fewer third-party actions is the simplest supply-chain story.

## R3. Caching

**Decision.** `Swatinem/rust-cache` (pinned) in every job that compiles, with the default key
(lockfile, toolchain, job name), `save-if: ${{ github.ref == 'refs/heads/main' }}` so only `main`
writes caches and pull requests read them, which avoids filling the 10 GB repository cache limit.

**Why.** It is the de-facto Rust standard, handles `target/` pruning and registry caching, and
needs no configuration. Release builds use a separate `shared-key` per target.

## R4. The set of PR checks and how each is run

| Job (name) | Command | Runner |
|---|---|---|
| `fmt` | `cargo fmt --all --check` | ubuntu-24.04 |
| `clippy` | `cargo clippy --workspace --all-targets --locked -- -D warnings` | ubuntu-24.04 |
| `test (ubuntu-24.04)` | `cargo test --workspace --locked` (after a prerequisites check) | ubuntu-24.04 |
| `test (macos-15)` | same | macos-15 |
| `docs` | `cargo doc --workspace --no-deps --locked` with `RUSTDOCFLAGS=-D warnings` | ubuntu-24.04 |
| `msrv` | `cargo +1.88 test --workspace --locked` | ubuntu-24.04 |
| `deny` | `cargo-deny-action` with `check` (all four categories) | ubuntu-24.04 |
| `workflows` | `actionlint` and `shellcheck scripts/*.sh` and the script tests | ubuntu-24.04 |
| `changes` | outputs whether the diff touches `Formula/` | ubuntu-24.04 |
| `formula` | `brew audit`/`brew test`; job-level `if` on `changes`, so it is skipped (and not billed) when `Formula/` is untouched; skipped counts as passed in `ci-ok` | ubuntu-24.04 and macos-15 |

| `pr-staleness` (PR only) | `scripts/pr-staleness.sh` (warning only) | ubuntu-24.04 |
| `ci-ok` | aggregate | ubuntu-24.04 |

`--locked` makes cargo fail instead of updating `Cargo.lock` (FR-003). The existing Intel-macOS
build check moves into the release matrix (the cross build is exactly what release does). Clippy
and docs run on one OS only: lints and rustdoc output do not differ by platform for this code,
and the macOS minutes are the scarce resource (R12).

Homebrew is installed on the `ubuntu-24.04` image but not on `PATH`; the formula job starts with
`eval "$(/home/linuxbrew/.linuxbrew/bin/brew shellenv)"` (checked in the runner-images README).
`shellcheck` is preinstalled there too; `qemu-user` and Zig are not (R11).

Linux test prerequisites (FR-006): the first step of the Linux test job runs `command -v perl
pgrep kill stty sed` and prints which is missing. The pty tests also assume a `sed` that accepts
`-i.bak` (GNU sed does; checked in `cli_show.rs`).

**Checked locally:** the commands above are the ones that passed on this Mac for M4 (fmt, clippy,
tests twice, +1.88, doc with warnings denied).

## R5. Dependency policy (`deny.toml`)

**Decision.** `cargo-deny` with all four categories:

- `[advisories]`: `version = 2`, deny vulnerabilities, unmaintained and unsound crates (workspace
  and transitive), `yanked = "deny"`.
- `[licenses]`: an allow-list, no exceptions needed. From the lockfile (checked, 185 packages):
  `MIT`, `Apache-2.0`, `Apache-2.0 WITH LLVM-exception`, `BSD-2-Clause`, `BSD-3-Clause`, `ISC`,
  `Zlib`, `Unlicense`, `Unicode-3.0`, `BSL-1.0`, `0BSD`, `CC0-1.0`, `MPL-2.0` (not currently used,
  harmless as a file-level copyleft). Expressions with `OR` pass when one branch is allowed (one
  crate offers `LGPL-2.1-or-later` as a third choice; the MIT branch applies). `confidence-threshold
  = 0.93`. The workspace crates need a `license` field (R15) or cargo-deny reports them
  unlicensed; `private = { ignore = true }` is not used because the crates are meant to be
  published as source.
- `[bans]`: `multiple-versions = "warn"` (many transitive duplicates are normal; an error would only
  teach people to ignore the check), `wildcards = "deny"`.
- `[sources]`: `unknown-registry = "deny"`, `unknown-git = "deny"`, only crates.io allowed.

**Why.** This is the policy most Rust projects use; it covers FR-004 in one tool and one file.

**Alternative.** `cargo audit` for advisories plus separate license tooling: two tools, two
configs, and `cargo-deny` already includes the RustSec database.

## R6. Concurrency, timeouts and permissions

**Decision.**

- Top of every workflow: `permissions: contents: read`. Jobs that need more declare it locally:
  `release` (`contents: write`, `id-token: write`, `attestations: write`), `formula-pr`
  (`contents: write`, `pull-requests: write`), the failure-issue steps in `perf.yml` and `audit.yml`
  (`issues: write`).
- `ci.yml` concurrency group `ci-${{ github.workflow }}-${{ github.ref }}` with
  `cancel-in-progress: ${{ github.event_name == 'pull_request' }}`, so superseded pull request runs
  are cancelled and runs on `main` and tags never are (FR-005).
- `release.yml` concurrency group per tag, `cancel-in-progress: false`.
- `timeout-minutes` on every job: 15 for lint-type jobs, 30 for tests, 60 for release builds.

**Why.** Read-only default and narrow grants are the baseline GitHub recommends; fork pull requests
get a read-only token anyway, so nothing in `ci.yml` may need a secret (FR-010).

## R7. Pull request title check (trunk-based, squash merges)

**Decision.** `scripts/check-pr-title.sh "$TITLE"` implements a regex for
`^(feat|fix|docs|test|refactor|perf|build|ci|chore|revert)(\([a-z0-9._/-]+\))?!?: .+` and rejects
titles over 100 characters or ending in a period. The job passes the title through an environment
variable (never interpolated into the shell line, so a hostile title cannot inject commands). It is its own workflow (`pr-title.yml`, one 2-minute job) on `pull_request` types `opened`,
`edited`, `synchronize`, `reopened`: retitling a pull request re-runs only this check. It is the
second required status, next to `ci-ok`.

**Why.** With squash merging and the setting "default to pull request title and commit details",
the title becomes the commit subject on `main` and from there the release note. A regex in a script
is smaller and more auditable than a third-party action, and is unit-testable.

**Alternative.** `amannn/action-semantic-pull-request`: well known, but one more pinned action for
a 6-line regex.

## R8. Staleness warning

**Decision.** `scripts/pr-staleness.sh` reads the pull request's `created_at` and the compare
result `main...head` (`behind_by`) from the REST API through `gh api`, and writes a warning to
`$GITHUB_STEP_SUMMARY` (and an `::warning::` annotation) when the PR is older than 7 days or more
than 50 commits behind. It always exits 0.

## R9. Performance budgets

**Decision.** `perf.yml` runs `cargo test --release -p kb --test perf -- --ignored --nocapture
--test-threads=1` on `ubuntu-24.04` and `macos-15`, on pushes to `main` and every Monday. When a
job fails, a step uses `gh issue list --label perf-regression --state open` to find the tracking
issue and either comments on it or creates it. It does not run on pull requests.

**Why.** Hosted runners are noisy; the M3 and M4 notes already show budgets within 10% of the
limit failing occasionally. A red pull request caused by noise teaches people to retry blindly; a
tracking issue gets looked at.

## R10. Release trigger and validation

**Decision.** `on: push: tags: ['v[0-9]*.[0-9]*.[0-9]*']` (GitHub glob; covers `v1.2.3` and
`v1.2.3-rc.1`). The first job runs `scripts/check-release-tag.sh "$GITHUB_REF_NAME"` which:

1. matches the tag against the full semver regex `^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?$` (the glob is too loose: it admits `v1.2.3abc`);
2. reads the workspace version with `cargo metadata --no-deps --format-version 1` and compares;
3. with `fetch-depth: 0`, checks `git merge-base --is-ancestor "$GITHUB_SHA" origin/main`;
4. prints `version=…`, `prerelease=true|false` to `$GITHUB_OUTPUT`.

Any failure stops the run with an `::error::` naming both values. A `workflow_dispatch` run has a
branch in `GITHUB_REF_NAME`, not a tag, so a dry run calls `check-release-tag.sh --dry-run`: it
reads the version from `Cargo.toml`, derives the tag `v<version>`, applies the same syntax rule and
skips step 3; the publish jobs do not run. (An early draft passed the branch name to the same check
and would have failed it before anything built.)

## R11. Targets, runners and how each binary is built

| Target | Runner | Build | Smoke test |
|---|---|---|---|
| `aarch64-apple-darwin` | macos-15 (arm64) | `cargo build --release --locked --target …` | native |
| `x86_64-apple-darwin` | macos-15 (arm64) | same, with `rustup target add` | `arch -x86_64 ./kb …` (Rosetta); if Rosetta is missing the step records "not smoke-tested" in the release notes (FR-015) |
| `x86_64-unknown-linux-musl` | ubuntu-24.04 | `cargo zigbuild --release --locked --target …` | native |
| `aarch64-unknown-linux-musl` | ubuntu-24.04 | `cargo zigbuild …` | `qemu-aarch64 ./kb …` (apt `qemu-user`) |

Zig comes from `mlugg/setup-zig` (pinned) and `cargo-zigbuild` from `taiki-e/install-action`
(pinned). The `[profile.release]` section gains `lto = "thin"`, `codegen-units = 1` and `strip =
"symbols"` (R14). `panic` stays `unwind` because `main` uses `catch_unwind`.

`qemu-user` is installed with `apt-get install qemu-user` (static musl binaries need no
sysroot). Rosetta is not documented on the `macos-15` image; the Intel smoke test first runs
`softwareupdate --install-rosetta --agree-to-license` and, if that fails, the `macos-15-intel`
runner is the alternative (Intel runners exist; they cost the same ×10), otherwise the platform is
listed as not smoke-tested.

**Why musl for Linux.** Spec: the binary runs on any distribution including minimal containers
(US3.1). A glibc build would need a floor (the runner's glibc) and breaks on older systems.

**Risk, stated plainly.** Nothing here has been run. Compiling bundled SQLite and the workspace for
musl with Zig is common and expected to work; the dry-run dispatch is the validation (R19).
Fallbacks in order: `cross`, then gnu targets built on `ubuntu-22.04` with the glibc floor
documented.

**Why not native arm64 Linux runners.** Free for public repositories only; this one is private.
When the repository is public a native `ubuntu-24.04-arm` job could replace QEMU (a later
simplification, not needed now).

## R12. Cost on a private free-plan repository

2,000 included minutes a month; Linux counts 1×, macOS 10×. Trunk-based development makes every
squash merge a push to `main`, which runs `ci.yml` again and `perf.yml` on both operating systems.
Estimates for a warm cache (cold is about double):

| Run | Linux min | macOS min | Billed |
|---|---|---|---|
| a pull request run (`ci.yml`) | ≈ 25 | ≈ 10 | ≈ 125 |
| the push to `main` after merging it (`ci.yml`) | ≈ 25 | ≈ 10 | ≈ 125 |
| `perf.yml` on that push (release build + timing tests) | ≈ 8 | ≈ 12 | ≈ 130 |
| **one merged change** | | | **≈ 380** |
| a release (two macOS builds, Linux builds, `ci.yml` again) | ≈ 60 | ≈ 25 | ≈ 310 |

So 2,000 minutes cover roughly **five merged changes a month**, with nothing left for a release.
(An earlier draft counted only the pull request run, about 16 runs a month; that was wrong for
trunk-based work.)

Levers, none applied yet because they change the spec or the safety net: (1) make the repository
public: public repositories get unlimited minutes and the problem disappears (the maintainer
intends this); (2) run `perf.yml` weekly only, not on every push to `main` (changes FR-008);
(3) skip the macOS leg of `ci.yml` on pushes to `main`, since the same commit just passed it as a
pull request; (4) accept a handful of merges a month. The plan implements the spec as written and
asks the maintainer to choose before `/speckit-tasks`; each lever is a one-line change in a
workflow file.

## R13. Packaging (reproducible archives)

**Decision.** A `package` job on ubuntu-24.04 downloads the four built binaries and runs
`scripts/package.sh <version> <target> <binary>` for each. The script:

- sets `SOURCE_DATE_EPOCH` to the tagged commit's time;
- stages `kb-<version>-<target>/{kb,README.md,LICENSE-MIT,LICENSE-APACHE}` with mode 0755 / 0644
  and zeroed owner;
- `scripts/package.sh` builds the archive with Python's `tarfile` and `gzip` (fixed member order,
  owner 0, modification time `SOURCE_DATE_EPOCH`, no name or time in the gzip header), so the same
  script gives the same bytes on macOS and Linux (neither `bsdtar` nor GNU `tar` is needed);
- writes `SHA256SUMS` with `sha256sum` in the `<hash>  <file>` format `shasum -c` also reads.

Packaging on one Linux runner, instead of on each build runner, is what makes the archives
byte-identical for identical binaries (macOS `bsdtar` has no `--sort`, hence Python).

**Reproducibility check (US3.4).** `audit.yml` (weekly) builds `x86_64-unknown-linux-musl` twice
from the same commit in two directories with `--remap-path-prefix` and compares SHA-256. The
check is a weekly canary, not part of each release, to keep a release under 40 minutes.

## R14. Release profile

**Decision.** In the root `Cargo.toml`: `[profile.release] lto = "thin"`, `codegen-units = 1`,
`strip = "symbols"`. `opt-level` stays at the default 3. Debug info is not shipped.

**Why thin LTO.** Full LTO of a workspace with SQLite, syntect and ratatui adds many minutes per
target for a few percent; thin keeps most of the benefit. The timing tests already pass in release
mode without these settings, so they are not a performance fix but a size and consistency one.

## R15. License files

The repository has no license and the crates have no `license` field. Archives must contain license
files (FR-016) and `cargo-deny` needs a license on the workspace crates. **Default proposed:**
`MIT OR Apache-2.0`, the Rust ecosystem's usual dual license (and what 90% of the dependencies
use), as `LICENSE-MIT` and `LICENSE-APACHE` plus `license = "MIT OR Apache-2.0"` in
`[workspace.package]` inherited by both crates. **This is the maintainer's decision**, and it is needed *before the first pull request*, not on
release day: `deny` is part of `ci-ok`, and without a `license` field on the workspace crates it
reports them unlicensed, so the very first run would be red. The tasks add the files and the field
first; the release script also fails clearly if a license file is missing.

## R16. Release creation, notes and attestations

**Decision.** A `release` job (needs `package`):

1. `gh release view "$TAG"` to see whether a release exists; if not, `gh release create "$TAG"
   --draft --verify-tag --title "$TAG" [--prerelease] --notes-file notes.md`; if it exists as a
   draft or published release, `gh release edit` the notes and upload with `--clobber` (FR-019).
2. Uploads the four archives and `SHA256SUMS`.
3. If `github.event.repository.private == false`, runs `actions/attest-build-provenance` (pinned)
   on the archives; otherwise writes "attestations skipped: repository is private" to the run
   summary. (**Checked from practice:** GitHub artifact attestations require a public repository on
   the free, Pro and Team plans.)
4. Re-downloads the draft's assets and runs `sha256sum -c SHA256SUMS`, then `gh release edit
   --draft=false`. Pre-release tags are published with `--prerelease`.

**Notes** come from `git-cliff --latest --strip header -c cliff.toml` (`taiki-e/install-action`
pinned): sections Features, Fixes, Performance, Documentation, Other, grouped by Conventional
Commit type, breaking changes first. With no previous tag it covers the whole history; with no
user-facing commits it prints "No user-facing changes." (FR-018, edge cases).

**Alternatives.** `softprops/action-gh-release`: a third-party action for what the preinstalled
`gh` does in four commands. GitHub's own `--generate-notes`: groups by label, not by commit type.

## R17. Homebrew formula in this repository

**Decision.** `.github/formula.rb.tmpl` has placeholders for the version and for each archive's
URL and SHA-256. `scripts/gen-formula.sh <tag> SHA256SUMS` renders `Formula/kb.rb`:

```ruby
class Kb < Formula
  desc "Terminal knowledge base for operational snippets"
  homepage "https://github.com/cortiz/kb"
  version "<version>"
  license "MIT OR Apache-2.0"

  on_macos do
    on_arm do   url ".../kb-<version>-aarch64-apple-darwin.tar.gz";      sha256 "…" end
    on_intel do url ".../kb-<version>-x86_64-apple-darwin.tar.gz";       sha256 "…" end
  end
  on_linux do
    on_arm do   url ".../kb-<version>-aarch64-unknown-linux-musl.tar.gz"; sha256 "…" end
    on_intel do url ".../kb-<version>-x86_64-unknown-linux-musl.tar.gz";  sha256 "…" end
  end

  def install
    bin.install "kb"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/kb --version")
  end
end
```

(the real file formats each block over several lines). After a **stable** release, the
`formula-pr` job (skipped for pre-releases, FR-027) checks out `main`, runs the generator, and if
the file changed:

1. runs `brew audit --strict` and `brew test` on the generated formula (Homebrew on the macOS
   runner; on Linux after `eval "$(/home/linuxbrew/.linuxbrew/bin/brew shellenv)"`), so a broken
   formula never gets a pull request;
2. commits to branch `chore/formula-<version>` and opens a pull request titled
   `chore(release): kb <version> formula` with `gh pr create`, stating the audit and test results
   in the body;
3. dispatches `ci.yml` on that branch with `gh workflow run`.

Why steps 1 and 3: a pull request opened with the default `GITHUB_TOKEN` does **not** start
`pull_request` workflows (GitHub's guard against token-triggered recursion), so the `formula` job
in `ci.yml` and `ci-ok` would never run on it, and a required `ci-ok` could never turn green. Step 1
makes the pull request safe without them; step 3 supplies the checks, to be confirmed on the first
formula PR (whether a check run from a dispatched workflow satisfies a required status on the pull
request's head commit). If it does not, the fallback is to close and reopen the pull request, which
is a human event and starts the normal run; `CONTRIBUTING.md` says so.

If the repository setting "Allow GitHub Actions to create and approve pull requests" is off,
`gh pr create` fails with a recognizable message; the job then uploads the generated formula to the
release as `kb.rb` and writes the reason in the run summary, and does not fail the release (spec
assumption).

**CI for the formula.** When a pull request touches `Formula/` (a human edit, or the bot's pull
request after reopening), the `formula` job of `ci.yml` runs on ubuntu and macOS: `brew tap
cortiz/kb "$GITHUB_WORKSPACE"` (a tap from a local path), `brew audit --strict kb`, `brew install
kb`, `brew test kb`. While the repository is private the install step cannot download the release
assets; the job then runs `audit` only and prints the reason.

**Taps from any repository.** Homebrew accepts `brew tap <user>/<name> <url>` for any Git
repository containing a `Formula/` directory, so `cortiz/kb` needs no `homebrew-` prefix (from
practice; the installation line goes into the README after the repository is public).

## R18. Dependabot

`.github/dependabot.yml`: ecosystems `cargo` and `github-actions`, `directory: "/"`, weekly on
Monday, `open-pull-requests-limit: 5`, `groups` with a single group per ecosystem (`patterns:
["*"]`, update types minor and patch), commit-message prefix `chore(deps)` so squash titles pass
the title check. Dependabot also keeps pinned action hashes current when the version comment is in
the standard `# v4.1.1` form.

## R19. Testing the pipeline

1. **Static:** `actionlint` (via `taiki-e/install-action`) and `shellcheck` in the `workflows` job.
2. **Scripts:** `scripts/tests/*.sh` run in the same job (tests first, per principle VIII): a table
   of valid and invalid tags and titles, a fake `cargo metadata` for the version comparison, a
   package-twice-compare for reproducibility, and formula generation against a canned `SHA256SUMS`
   followed by a Ruby syntax check (`ruby -c`) when Ruby is present.
3. **Dry run:** `workflow_dispatch` on `release.yml` with `dry_run: true` builds, smoke-tests and
   packages all four targets and uploads them as workflow artifacts without creating anything.
4. **Throw-away tag:** `v0.0.0-test.1` on `main` exercises the real publish path as a pre-release,
   then `gh release delete --cleanup-tag`.
5. **Local:** `scripts/ci-local.sh` runs fmt, clippy, tests, docs, and (if installed) deny,
   actionlint and shellcheck. `act` would run Linux jobs in Docker but the daemon is not running
   on this machine and macOS jobs cannot run in it, so it is not part of the plan.

## R20. Trunk-based development: settings and branch protection

Spec FR-032/FR-033. The repository settings (squash only, PR-title-and-commits message, delete
branch on merge, auto-merge allowed) are applied with one `gh api` call listed in
[contracts/repo-settings.md](contracts/repo-settings.md); the maintainer runs it (or confirms it).
Branch protection and rulesets are refused by GitHub for private free-plan repositories (checked:
`gh api …/branches/main/protection` returns 403 "Upgrade to GitHub Pro or make this repository
public"), so the same document gives the ruleset to apply when that changes. Until then the
pipeline's checks run and report but do not block merging; the maintainer's discipline (merge only
when `ci-ok` is green) stands in, and `CONTRIBUTING.md` says so.

## R21. Git remote

`git push` fails today (`Permission denied (publickey)`) while `gh` is authenticated over HTTPS
(checked). The first quickstart step is `gh auth setup-git` and
`git remote set-url origin https://github.com/cortiz/kb.git`. The alternative is adding an SSH key
to the GitHub account. Neither changes the pipeline.
