# Contract: Configuration File Schema

**Branch**: `008-actions-code-links` | **Date**: 2026-10-09 | **Spec**: [spec.md](../spec.md)

This contract defines the additions to `~/.config/kb/config.toml` (or platform equivalent) for actions, deep links, code repositories, and execution safety.

---

## Configuration Schema

```toml
# General TUI settings
[tui]
# Controls what Enter does in the main list:
# "copy" (default) -> copies payload / location
# "primary"        -> executes primary open action (Enter never runs commands)
enter = "copy"

# Sumo Logic integration
[sumo]
base_url = "https://service.us2.sumologic.com"

# AWS CloudWatch integration
[aws]
default_region = "us-east-1"

# Code links and editors
[code]
# Default target when opening code: "web", "vscode", or "idea" (defaults to "web")
default_open = "web"

# Directory roots to search 1-level deep for git clones
search_paths = [
    "~/repos",
    "~/code",
]

# Custom web permalink template for non-GitHub/GitLab/Bitbucket hosts
# Placeholders: {repo}, {ref}, {path}, {line}, {end_line}
web_url = "https://source.corp.internal/{repo}/blob/{ref}/{path}#L{line}"

# Explicit repository clone overrides
[code.repos]
"auth0/auth0-java" = "~/repos/auth0-java"
"cortiz/kb" = "~/repos/jmpeax/kb"

# Command execution settings
[run]
# Whether to pause and show exit code/duration after successful (code 0) runs
pause_after = true

# User-defined destructive patterns requiring 'run' typed confirmation
destructive_patterns = [
    "ansible-playbook.*--extra-vars.*prod",
    "prisma migrate reset",
]

# Custom actions per type
[[actions.sql]]
name = "psql"
key = "p"
command = "psql -d {{database}} -h {{host}}"
input = "stdin"  # "stdin" (default) or "arg"

[[actions.prompt]]
name = "claude"
key = "c"
command = "claude -p"
input = "stdin"

[[actions.cmd]]
name = "dry-run"
key = "d"
command = "echo [DRY RUN]: {{payload}}"
input = "arg"
```

---

## Validation & Error Handling

- **Key Conflicts**: Custom action keys must not conflict with built-in keys for that entry type. Conflicting keys emit a startup warning and the custom action is disabled.
- **Missing Placeholders**: If a custom action command references `{{field}}` and the active entry does not declare that field, the action is disabled for that specific entry with an explanatory reason.
- **Region Fallback Chain**: For `cw` entries:
  1. Entry's `region` field
  2. `aws.default_region` in config
  3. `AWS_REGION` environment variable
  4. `AWS_DEFAULT_REGION` environment variable
  If none is resolved, action execution fails with a diagnostic message.
- **Destructive Patterns Invariant**: Built-in destructive patterns cannot be disabled or overwritten. Any entries in `run.destructive_patterns` are purely additive.
