# Contract: `kb init` and `kb lint`

## Global

```text
kb [--root <path>] <command> [options]
kb --help | --version
```

- `--root <path>`: the KB root. Resolution order (FR-022): `--root`, `KB_ROOT`, `root` in the
  config file, then `~/kb`. Config locations are in [research.md R7](../research.md#r7-configuration-file-and-kb-root).
- Exit codes for every command: `0` success, `1` the command ran and its result is a failure
  (lint findings), `2` usage, configuration or I/O error. Errors that cause exit 2 go to stderr as
  `kb: <message>`. With `--json` they are also printed to stdout as
  `{"error": {"kind": "...", "message": "..."}}`.
- Output is never colored when stdout is not a terminal, or when `NO_COLOR` is set.

## `kb init [path] [--no-examples] [--json]`

Creates a KB, or completes an existing one. Never overwrites, truncates or deletes anything.

- `path`: the directory to initialize. If omitted, the resolved KB root is used.
- Actions, in order, each skipped if the target already exists:
  1. create the directory (and its parents),
  2. create `.kb/`,
  3. create `.kb/.gitignore` containing `*` followed by a newline,
  4. unless `--no-examples` is given or the directory already holds entries, create one example
     per type: `sql/example-active-connections.md`, `sumo/example-5xx-by-authorizer.md`,
     `cw/example-lambda-errors.md`, `cmd/example-tail-pod-logs.md`,
     `prompt/example-summarize-incident.md`, `link/example-runbook.md`,
     `code/example-request-handler.md`.
- Files are created with exclusive create, so a file that appears concurrently is never
  overwritten.
- Fails with exit 2 if `path` exists and is not a directory, or if anything can't be created.
- Every example passes `kb lint --strict` with no findings at all.

Human output:

```text
Initialized KB at /home/carlos/kb
  created  .kb/
  created  .kb/.gitignore
  created  sql/example-active-connections.md
  …
```

On an existing KB, lines say `exists` instead of `created`, and the first line says
`KB at /home/carlos/kb is up to date` when nothing was created. When `path` differs from the
resolved root, a final hint explains how to set `KB_ROOT` or the config `root`.

JSON output: [init-report.schema.json](init-report.schema.json).

## `kb lint [paths...] [--strict] [--quiet] [--json]`

- No paths: lint the whole KB. Paths may be files or directories. Relative paths are resolved
  against the current directory if they exist there, otherwise against the KB root. A path that
  doesn't exist or is outside the root is exit 2.
- `--strict`: warnings also fail the run.
- `--quiet`: print only the summary line (human output). It has no effect on `--json`.
- Exit: `1` if there is any error, or any warning under `--strict`, otherwise `0`. Info findings
  and suppressions never fail the run.

Human output, ordered by path, then line, then column:

```text
sumo/5xx.md:2:7: error[KB001-unknown-type]: unknown type "sumologic"; expected one of sql, sumo, cw, cmd, prompt, link, code
  fix: use "sumo"
cmd/deploy.md:14:1: warning[KB048-unused-var]: variable "region" is declared but never used
  fix: remove it from vars, or reference it as {{region}}

284 entries, 1 error, 1 warning
```

Summary line: `<n> entries, <e> errors, <w> warnings`, then `, <i> info` and `, <s> suppressed`
only when those are non-zero. Singular forms are used for 1 (`1 error`).

JSON output: one document on stdout, [lint-report.schema.json](lint-report.schema.json).
