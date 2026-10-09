# Data Model: CI and Release Pipeline

Nothing here is stored by kb. These are the things the pipeline reasons about and the rules that
relate them.

## Check

A named status on a pull request or commit.

| Name | When it runs | Blocks merge (when protection is available) |
|---|---|---|
| `ci-ok` | every `ci.yml` run | **yes (required)** |
| `pr-title` (own workflow) | pull request opened/edited/synchronized/reopened | **yes (required)** |
| `fmt`, `clippy`, `docs`, `msrv`, `deny`, `workflows` | every run | via `ci-ok` |
| `changes`, `formula` | `formula` only when `Formula/` changed | via `ci-ok` (skipped counts as passed) |
| `test (ubuntu-24.04)`, `test (macos-15)` | every run | via `ci-ok` |
| `pr-staleness` | pull requests | never (warning only) |

Rule: `ci-ok` = failure if any needed job's result is `failure` or `cancelled`, else success
(`skipped` counts as success).

## Release version

`Version` = the workspace `version` in `Cargo.toml`. `Tag` = `v` + `Version`. `Prerelease` = the
version has a `-` suffix. Validation (research R10): tag matches semver, `Tag == "v" + Version`,
and the tagged commit is an ancestor of `origin/main`.

## Release artifact

| Field | Rule |
|---|---|
| `target` | one of `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` |
| `archive` | `kb-<Version>-<target>.tar.gz` |
| contents | one top-level directory `kb-<Version>-<target>/` holding `kb` (0755), `README.md`, `LICENSE-MIT`, `LICENSE-APACHE` (0644) |
| `sha256` | line `<hex>  <archive>` in `SHA256SUMS` |
| `attestation` | present when the repository is public; absent (and said so) when private |
| `smoke` | always `tested`; a binary that cannot be smoke-tested stops the release |

## Release

| Field | Rule |
|---|---|
| name | `Tag` |
| state | `draft` → `published`; never edited back |
| `prerelease` | equals `Prerelease` |
| assets | exactly the four archives and `SHA256SUMS` (and `kb.rb` only when the formula PR could not be opened) |
| notes | git-cliff output since the previous release tag, nothing else |

### State transitions of a release run

```
verify ──fail──▶ stop (nothing created)
  │ ok
  ▼
ci (same checks as a PR) ──fail──▶ stop (nothing created)
  │ ok
  ▼
build ×4 ──any fail──▶ stop (nothing created)
  │
  ▼
smoke ×4 ──any fail──▶ stop (nothing created)
  │
  ▼
package ──▶ draft release (created or reused) ──▶ upload (clobber) ──▶ verify checksums
                                                             │ ok
                                                             ▼
                                                        publish ──▶ formula PR (stable only)
```

Idempotency: a re-run finds the existing draft or published release, replaces its assets and
notes, and re-publishes; it never creates a second release for a tag.

## Formula

`Formula/kb.rb`, generated, never edited by hand: `version`, four `url`/`sha256` pairs selected by
OS and CPU, `bin.install "kb"`, a test asserting `kb --version` contains the version. Only stable
releases change it; each change is one pull request titled `chore(release): kb <Version> formula`.

## Policy file (`deny.toml`)

Sections `advisories`, `licenses` (allow-list), `bans`, `sources`; the values are in research R5.

## Trunk

The `main` branch. Invariants: every commit on it is one squashed pull request; its subject is a
Conventional Commit; every commit passed `ci-ok` at merge time (enforced by GitHub when branch
protection is available, by discipline until then); every tag `vX.Y.Z` points at an ancestor of it.
