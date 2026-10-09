# Quickstart: validating the pipeline

Each scenario names what to do and what you should see. Scenarios 1–2 need only this machine;
3–7 need GitHub (and so need the push to work).

## 0. Prerequisites (once)

Decide the license first: without a `license` on the workspace crates `deny` is red on the very
first pull request (research R15).

```sh
gh auth setup-git
git remote set-url origin https://github.com/cortiz/kb.git
gh api -X PATCH repos/cortiz/kb -f allow_squash_merge=true -F allow_merge_commit=false \
  -F allow_rebase_merge=false -F delete_branch_on_merge=true -F allow_auto_merge=true \
  -f squash_merge_commit_title=PR_TITLE -f squash_merge_commit_message=COMMIT_MESSAGES
```

(The settings call is in [contracts/repo-settings.md](contracts/repo-settings.md); it changes the
repository and needs your go-ahead.)

## 1. Local: the same checks as a pull request

```sh
scripts/ci-local.sh          # fmt, clippy, tests, docs, then deny/actionlint/shellcheck if installed
scripts/tests/run.sh         # tests of the helper scripts
```

Expect every step `ok`. Break each rule on purpose and see exactly that step fail (SC-002).

## 2. Local: the release helpers

```sh
scripts/check-release-tag.sh v0.1.0            # version=0.1.0 prerelease=false (on main)
scripts/check-release-tag.sh v9.9.9            # error: tag v9.9.9 but Cargo.toml says 0.1.0
scripts/check-pr-title.sh "feat(copy): add --method"      # ok
scripts/check-pr-title.sh "Update stuff"                  # error: expected type(scope): subject
```

## 3. A pull request (first push)

Push a branch, open a pull request titled `ci: replace the CI workflow` and watch the checks.
Expected: all jobs run, Linux tests may reveal real failures (Linux has never run; fix them on the
same branch), `ci-ok` turns green, `pr-title` fails if you retitle it badly and passes when fixed
(retitling re-runs it), and a second push cancels the first run.

## 4. Dry-run release

Actions → Release → Run workflow → `dry_run: true` (a workflow can be dispatched only after its file is on `main`, so merge the pull request that adds `release.yml` first). Expected: `verify`, `ci`, four builds, four
smoke tests, `package` produce the artifact `dist` (four archives + `SHA256SUMS`); `release` and
`formula-pr` are skipped. Download `dist` and check the checksums and that each archive unpacks to
the layout in [contracts/release.md](contracts/release.md). This is where the musl + Zig build is
proven (or the fallback in research R11 is taken).

## 5. A release candidate on `main`

Releases come only from `main`, so there is no test tag on a side branch. Bump `version` to
`0.1.0-rc.1` in one pull request (title `chore(release): 0.1.0-rc.1`), merge it, then:

```sh
git tag v0.1.0-rc.1 <merge commit> && git push origin v0.1.0-rc.1
```

Expected: a GitHub release `v0.1.0-rc.1`, marked pre-release, with five assets and notes; the
formula is **not** touched. Delete it afterwards (`gh release delete v0.1.0-rc.1 --cleanup-tag --yes`)
and bump to `0.1.0` for the real release.

## 6. A real stable release

Bump `version` in `Cargo.toml` (one pull request, title `chore(release): 0.2.0`), then tag `v0.2.0`
on the merge commit. Expected: published release; within 15 minutes a pull request
`chore(release): kb 0.2.0 formula` is open (or `kb.rb` is attached to the release with the reason in the
summary); merge it.

## 7. As a user, on a clean machine

```sh
shasum -a 256 -c --ignore-missing SHA256SUMS && tar -xzf kb-0.2.0-<target>.tar.gz
./kb-0.2.0-<target>/kb --version
gh attestation verify kb-0.2.0-<target>.tar.gz --repo cortiz/kb      # public repository only
brew tap cortiz/kb https://github.com/cortiz/kb && brew install kb   # public repository only
```

## 8. Weekly jobs

Run `perf` and `audit` by hand (`workflow_dispatch`) once: they pass, or a single tracking issue
appears and is updated (not duplicated) by a second failing run.
