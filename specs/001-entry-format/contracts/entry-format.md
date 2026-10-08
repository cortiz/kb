# Contract: Entry File Format, `kb_format: 1`

This is the user-facing format. It is documented in `README.md` in the same change that ships it
(constitution, Development Workflow).

## File

- Location: any `*.md` file under the KB root, where no path component below the root starts with
  `.`. Other files are ignored.
- Encoding: UTF-8, with an optional BOM (kept as-is). Line endings: LF or CRLF (kept as-is).
- ID: the path relative to the root, without `.md`, with `/` separators. `sumo/5xx-by-authorizer.md`
  → `sumo/5xx-by-authorizer`. IDs that differ only by case are an error.

## Layout

```text
---                     ← line 1 (after the optional BOM), exactly three dashes
<YAML frontmatter>
---                     ← the next line that is exactly three dashes
<Markdown body>
```

Trailing spaces or tabs after either `---` are allowed. The body may be empty.

## Frontmatter

A YAML 1.2 mapping, restricted to a subset that keeps every value at one place in the file: no
anchors, aliases or explicit tags, and no duplicate keys. Comments and both flow (`{ }`, `[ ]`)
and block styles are allowed.

### Common keys

| Key | Required | Shape | Notes |
|-----|----------|-------|-------|
| `kb_format` | yes | integer ≥ 1 | Only `1` exists. Higher values: the file is reported and never rewritten. |
| `type` | yes | string | `sql`, `sumo`, `cw`, `cmd`, `prompt`, `link`, `code` |
| `title` | yes | string | Non-blank, ≤ 120 characters |
| `tags` | no | list of strings | Read in lowercase without a leading `#`. The file keeps what was written. |
| `vars` | no | mapping name → definition | See Variables |
| `created`, `updated` | no | string | `YYYY-MM-DD` or RFC 3339 date-time |

### Type-specific keys

| Type | Keys | Payload |
|------|------|---------|
| `sql` | `dialect` (string), `database` (string) | yes |
| `sumo` | `range` (string, may use variables) | yes |
| `cw` | `log_groups` (list of strings), `range` (string), `region` (string) | yes |
| `cmd` | `shell` (`sh` default, `bash`, `zsh`, `fish`), `cwd` (string) | yes |
| `prompt` | none | yes |
| `link` | `url` (required, absolute `http`/`https`) | no |
| `code` | `code` (required map, below) | no |

`code` map: `repo` (`owner/name` or a git remote URL), `path` (relative, no `..`), `line`
(integer ≥ 1), `ref` (SHA, tag or branch), optional `symbol` (string), optional `end_line`
(integer ≥ `line`).

Any other top-level key is kept and reported as info.

## Body

- **Payload** (`sql`, `sumo`, `cw`, `cmd`, `prompt`): the content of the first CommonMark fenced
  code block (backticks or tildes, three or more). Use a longer fence to put fences inside a
  prompt. The info string (for example ```` ```sql ````) is allowed and ignored.
- **Notes**: all body text outside the payload. For `link` and `code`, the whole body.

## Variables

```yaml
vars:
  env:     { kind: enum, options: [prod, stage, dev], default: prod }
  minutes: { kind: number, default: 15, description: "Look-back window" }
  user:    { kind: text }
  token:   { kind: secret, env: API_TOKEN }
```

| Field | Allowed for | Notes |
|-------|-------------|-------|
| `kind` | all | Required: `text`, `number`, `enum`, `secret` |
| `default` | not `secret` | `enum`: must be in `options`. `number`: a number. |
| `options` | `enum` (required) | Non-empty list of scalars |
| `description` | all | String |
| `env` | all | Environment variable name to read at use time (M5) |

- A reference is `{{name}}` with `name` matching `[a-z][a-z0-9_]*`, and no spaces inside the
  braces.
- References are found in the payload and in `range`, `url`, `cwd` and each `log_groups` item.
- `\{{` is a literal `{{`. The file keeps the backslash, and the parsed value drops it.
- An undeclared reference is a `text` variable with no default.

## Secrets

Entries must not hold credentials. Use a `secret` variable read from `env`. To keep a line that
looks like a secret but isn't, end it with `kb:allow-secret` (in whatever comment syntax the
payload uses).
