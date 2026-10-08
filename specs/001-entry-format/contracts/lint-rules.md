# Contract: Lint Rules (kb_format 1)

Rule codes are stable: a code is never reused for a different check, and a retired code stays
reserved. The JSON `code` field holds the full form (`KB001-unknown-type`). Severities are `E`
(error), `W` (warning) and `I` (info). "Fix" says whether the finding carries a suggested fix.

Every error carries a file and a 1-based line and column (SC-003). File-level errors with no better
position point at `1:1`.

## File and frontmatter structure

| Code | Sev | Trigger | Fix |
|------|-----|---------|-----|
| KB001-unknown-type | E | `type` is not one of the seven types. The message lists the valid types. | Yes: closest type (prefix or edit distance ≤ 2), for example `sumologic` → `sumo` |
| KB002-no-frontmatter | E | The first line (after an optional BOM) is not `---`. | Yes: add a frontmatter block with `kb_format`, `type`, `title` |
| KB003-unclosed-frontmatter | E | An opening `---` with no closing `---` line. Reported at 1:1. | Yes: add a closing `---` line |
| KB004-empty-file | E | The file is empty or only whitespace. | Yes: start from an example entry |
| KB005-invalid-utf8 | E | Bytes are not valid UTF-8. The message gives the first bad byte offset. | Yes: re-save the file as UTF-8 |
| KB006-yaml-syntax | E | The frontmatter is not valid YAML. Position and message come from the parser. | No |
| KB007-frontmatter-not-mapping | E | The frontmatter is valid YAML but not a key/value mapping. | Yes: use `key: value` lines |
| KB008-unsupported-yaml | E | An anchor (`&a`), alias (`*a`) or explicit tag (`!x`) is used. | Yes: write the value out in full |
| KB009-duplicate-key | E | The same key appears twice in one mapping. Reported at the second one. | Yes: remove one of them |

## Required fields and keys

| Code | Sev | Trigger | Fix |
|------|-----|---------|-----|
| KB010-missing-kb-format | E | No `kb_format` key. | Yes: add `kb_format: 1` |
| KB011-invalid-kb-format | E | `kb_format` is not an integer, or is less than 1. | Yes: `kb_format: 1` |
| KB012-unsupported-kb-format | E | `kb_format` is greater than the highest supported (1). The file is never rewritten. | Yes: upgrade kb |
| KB013-missing-type | E | No `type` key. | Yes: add `type:` with the list of types |
| KB014-missing-title | E | No `title` key. | Yes: add `title:` |
| KB015-invalid-title | E | `title` is not a string, is empty or blank, or is longer than 120 characters. | Yes: shorten it or fill it in |
| KB016-unknown-key | I | A top-level key kb doesn't define. The key is preserved. | Yes when a known key is within edit distance 2 (`tilte` → `title`) |
| KB017-field-not-for-type | W | A known type-specific key used on another type (for example `url` on `sql`). | Yes: names the types that use it |
| KB018-wrong-value-type | E | A known key has the wrong YAML shape (for example `tags: lambda`, `log_groups: x`, `line: "x"`). | Yes: shows the expected shape |

## Tags and dates

| Code | Sev | Trigger | Fix |
|------|-----|---------|-----|
| KB020-invalid-tag | E | A tag is not a string, or is empty after normalization. | Yes: remove it |
| KB021-duplicate-tag | W | Two tags are equal after lowercasing and removing a leading `#`. | Yes: remove the duplicate |
| KB022-invalid-date | E | `created` or `updated` is not `YYYY-MM-DD` or an RFC 3339 date-time. | Yes: shows both accepted forms |

## Type-specific fields and payload

| Code | Sev | Trigger | Fix |
|------|-----|---------|-----|
| KB030-missing-url | E | A `link` entry has no `url`. | Yes: add `url: https://…` |
| KB031-invalid-url | E | `url` is not an absolute `http` or `https` URL. Variable references are allowed and treated as text when checking. | Yes |
| KB032-missing-code-ref | E | A `code` entry has no `code` map. | Yes: shows the map with all four required keys |
| KB033-invalid-code-ref | E | The `code` map is missing `repo`, `path`, `line` or `ref`; `repo` is not `owner/name` or a git URL; `path` is absolute or contains `..`; `line` or `end_line` is not a positive integer; `end_line` < `line`. One finding per problem. | Yes |
| KB034-invalid-shell | E | `shell` is not `sh`, `bash`, `zsh` or `fish`. | Yes: closest shell |
| KB035-missing-payload | E | A `sql`, `sumo`, `cw`, `cmd` or `prompt` entry has no fenced code block. Reported on the line after the closing `---`. | Yes: shows a fenced block |
| KB036-empty-payload | W | The payload block is empty or only whitespace. | Yes |

## Variables

| Code | Sev | Trigger | Fix |
|------|-----|---------|-----|
| KB040-invalid-var-name | E | A key under `vars` doesn't match `[a-z][a-z0-9_]*`. | Yes: lowercase/underscore form |
| KB041-missing-var-kind | E | A variable definition has no `kind`. | Yes: lists the kinds |
| KB042-unknown-var-kind | E | `kind` is not `text`, `number`, `enum` or `secret`. | Yes: closest kind |
| KB043-secret-default | E | A `secret` variable has a `default`. | Yes: remove it; use `env` |
| KB044-enum-options | E | An `enum` variable has no `options`, or an empty list. | Yes |
| KB045-default-not-in-options | E | An `enum` default is not one of its `options`. | Yes: lists the options |
| KB046-invalid-number-default | E | A `number` default is not a number. | Yes |
| KB047-undeclared-var | I | `{{name}}` is used but not declared. It is treated as `text` with no default. | Yes: shows a `vars` entry to add |
| KB048-unused-var | W | A declared variable is never referenced. | Yes: remove it or use it |
| KB049-not-a-variable | I | Text like `{{ Name }}` or `{{x-y}}` looks like a reference but isn't one, so it stays literal. | Yes: fix the name, or escape as `\{{` |

## Files and the KB as a whole

| Code | Sev | Trigger | Fix |
|------|-----|---------|-----|
| KB050-mixed-line-endings | W | The file mixes LF and CRLF line endings. | Yes: pick one |
| KB051-file-too-large | W | The file is larger than 1 MB (1,048,576 bytes). | No (by design; entries are meant to be small) |
| KB052-symlink-outside-root | W | A symlink points outside the KB root. It is skipped. | Yes: move the target into the KB, or remove the link |
| KB053-id-case-collision | E | Two or more IDs differ only by letter case. Reported on each file. | Yes: rename one |

## Secrets (FR-013)

All secret findings are errors, and their messages never contain more than the first 4 characters
of the match. A line ending in `kb:allow-secret` suppresses them for that line, and each
suppression is counted and listed in the report.

| Code | Sev | Trigger | Fix |
|------|-----|---------|-----|
| KB060-secret-aws-access-key | E | AWS access key ID | Yes: use a `secret` variable with `env` |
| KB061-secret-aws-secret-key | E | AWS secret access key in an assignment | Yes (same) |
| KB062-secret-bearer-token | E | `Bearer <token>` | Yes (same) |
| KB063-secret-jwt | E | JSON Web Token | Yes (same) |
| KB064-secret-private-key | E | Private key header | Yes: remove the key from the entry |
| KB065-secret-high-entropy | E | High-entropy value assigned to a name like `password`, `token`, `secret`, `api_key` | Yes: use a `secret` variable with `env` |

Patterns and the entropy threshold are in [research.md](../research.md#r4-secret-detection-fr-013).

## Coverage (SC-003)

38 error codes, of which 37 carry a suggested fix (all except KB006): 97%.
