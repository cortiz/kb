# Contract: repository settings for trunk-based development

GitHub settings cannot be changed from a file in the repository; these are the commands to run
(as the repository owner) and the protection to enable when GitHub allows it. `OWNER/REPO` =
`cortiz/kb`.

## Settings that work on a private free-plan repository

```sh
gh api -X PATCH repos/OWNER/REPO \
  -f allow_squash_merge=true  -F allow_merge_commit=false  -F allow_rebase_merge=false \
  -F delete_branch_on_merge=true  -F allow_auto_merge=true \
  -f squash_merge_commit_title=PR_TITLE  -f squash_merge_commit_message=COMMIT_MESSAGES
```

Effect: only squash merges; the squash commit's subject is the pull request title (checked to be a
Conventional Commit) and its body the individual commit messages; merged branches are deleted;
auto-merge can be enabled on a pull request.

Needed for the formula pull request (research R17); leave off until the first stable release if
you prefer. Without it the release still succeeds and the formula is attached to the release:

```sh
gh api -X PUT repos/OWNER/REPO/actions/permissions/workflow \
  -f default_workflow_permissions=read -F can_approve_pull_request_reviews=true
```

(`can_approve_pull_request_reviews` is the API name of "Allow GitHub Actions to create and approve
pull requests". The default token stays read-only; jobs ask for more per job.)

Git over HTTPS, so pushes work with the `gh` login:

```sh
gh auth setup-git
git remote set-url origin https://github.com/OWNER/REPO.git
```

## Branch protection or ruleset for `main` (when available: public repository, or a paid plan)

A repository ruleset targeting `~DEFAULT_BRANCH`, enforcement `active`:

| Rule | Value |
|---|---|
| Require a pull request before merging | on, 0 required approvals, dismiss stale approvals off |
| Require status checks | `ci-ok` and `pr-title`; "require branches to be up to date" on |
| Require linear history | on |
| Block force pushes | on |
| Restrict deletions | on |
| Bypass list | none (the owner uses auto-merge when `ci-ok` is green) |

Until it is available, `ci-ok` reports but does not block; merge only when it is green.

## Verification

```sh
gh api repos/OWNER/REPO --jq '{allow_squash_merge, allow_merge_commit, allow_rebase_merge, delete_branch_on_merge, allow_auto_merge, squash_merge_commit_title}'
gh api repos/OWNER/REPO/rulesets --jq '.[].name'          # 403 while private on the free plan
```
