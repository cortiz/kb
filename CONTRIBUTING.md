# Contributing

## Workflow (trunk-based)

`main` is the trunk and always releasable. Work happens on short-lived branches:

1. Branch from `main`, keep the change small, open a pull request early.
2. The pull request title is a Conventional Commit: `type(scope): subject`, with `type` one of
   `feat fix docs test refactor perf build ci chore revert` (add `!` for a breaking change), at most
   100 characters, no trailing period. It becomes the squash commit's subject and feeds the release
   notes.
3. Merge with **squash** only (merge and rebase are off). Turn on auto-merge and it merges when the
   required checks pass; the branch is deleted afterwards.

Required checks: `ci-ok` (fmt, clippy, tests on Linux and macOS, docs, MSRV 1.88, cargo-deny,
workflow lint, formula audit when `Formula/` changed) and `pr-title`. `pr-staleness` only warns
about a pull request older than 7 days or more than 50 commits behind `main`.

Branch protection requiring those two checks needs a public repository or a paid plan. Until then
the rule is by discipline; the commands for the settings that do work are in
[specs/005-ci-release-pipeline/contracts/repo-settings.md](specs/005-ci-release-pipeline/contracts/repo-settings.md).

## Checks on your machine

```sh
scripts/ci-local.sh        # fmt, clippy, tests, docs, and deny/actionlint/shellcheck when installed
scripts/tests/run.sh       # the tests of the scripts themselves
scripts/check-pins.sh      # every external action pinned to a commit hash with a version comment
```

## Releasing

1. In a pull request titled `chore(release): X.Y.Z`, set `version` under `[workspace.package]` in
   `Cargo.toml` (and `Cargo.lock` follows). Merge it.
2. Tag that merge commit and push the tag:

   ```sh
   git tag vX.Y.Z <merge-commit>
   git push origin vX.Y.Z
   ```

   `vX.Y.Z-rc.1` style tags make a pre-release.
3. The Release workflow checks the tag equals the Cargo version and sits on `main`, runs the full CI,
   builds the four binaries, smoke-tests each, and only then publishes the release: four archives
   plus `SHA256SUMS`, with notes grouped by commit type. Nothing is published if any step fails.
4. To retry a failed run, re-run it from the Actions page (it reuses the draft release; it never
   makes a second one). To remove a bad release: `gh release delete vX.Y.Z --cleanup-tag`, fix, and
   tag again. A manual run of the Release workflow (`gh workflow run release.yml`) is a dry run that
   builds and packages without publishing.

After a **stable** release a pull request `chore(release): kb X.Y.Z formula` updates
`Formula/kb.rb`. If its checks do not start (pull requests opened by Actions do not trigger
workflows), close and reopen it. If Actions may not open pull requests, the formula is attached to
the release as `kb.rb` instead.

Every external action is pinned to a commit hash; Dependabot proposes updates weekly.

## Authoring tests

Type detection (`kb_core::author::detect`) is judged on the corpus in
`tests/fixtures/authoring/detect/`, not on hand-picked examples: at least 95% of the snippets must
be detected right and nothing may be detected `High` and wrong. **A change to the detection rules
comes with a corpus case** that shows why (put deliberately ambiguous snippets in
`detect/low/<type>/`). Rendering is checked byte for byte against `tests/fixtures/authoring/golden/`.
Tests that touch the clipboard use fake `pbpaste` / `wl-paste` programs on `PATH` (see
`support::fake_paste` and the pty `Spec::with_fake_tool`), editors are shell scripts, and every
test gets its own `TMPDIR` when it checks that temp files are cleaned up. `KB_TEST_NOW` fixes the
clock in debug builds. Never point a test at `~/kb` or the real clipboard.

