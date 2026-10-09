# Test fixtures

A fixture KB shared by the `kb-core` integration tests and the `kb` CLI tests.

- `valid/`: entries that must lint with **no findings at all**, not even info. They must also
  round-trip byte for byte.
- `invalid/`: one entry per lint rule, each with a `.expect` sidecar.
- `secrets/`: secret samples (public documentation example values only), each with a `.expect`
  sidecar.

## `.expect` sidecars

`<name>.expect` sits next to `<name>.md` and lists every finding expected for that file, one per
line, as `<line>:<column> <CODE>`, for example:

```text
3:7 KB001-unknown-type
```

Lines are sorted by line, then column, then code. An empty `.expect` means "no findings". Lines
starting with `suppressed ` list expected suppressions as `suppressed <line> <CODE>`.

kb ignores every file that isn't `.md`, so sidecars never become entries.

## `search/`: the M2 search KB

A separate, lint-clean KB (`kb lint --strict` reports nothing) used by the M2 search tests. It covers all
seven entry types, accented text, tags, variables, a code repo, notes-only matches, and an entry
whose title equals a common word. The over-1-MB entry is not committed: tests generate it.

`queries.tsv` has one query per line: `query<TAB>expected_first_id<TAB>must_be_in_top3<TAB>story`.
`must_be_in_top3` is a comma-separated list of IDs (may be empty); `story` is `us1`, `us3` or
`us4` and says which user story makes the row pass. Lines starting with `#` are comments.

| Entry | Why it exists |
|-------|---------------|
| `sumo/5xx-by-authorizer` | the spec's headline example (`5xx auth`); tags `auth`, `lambda` |
| `cw/authorizer-latency`, `sql/authorized-users` | share the `authori` prefix with it |
| `cw/latency-prod`, `cw/latency-stage` | `type:`/`tag:`/`-tag:` filters |
| `sumo/login-failures` | second entry with tags `auth` and `prod` |
| `prompt/role-cache-notes`, `cmd/cache-role-flush` | phrase `role cache` vs the words reversed |
| `prompt/cache`, `prompt/cache-warmup` | exact title beats a higher term frequency |
| `sql/configuracion-red`, `cmd/network-config` | accent and case folding |
| `cmd/kubectl-pods`, `cmd/deploy-service` | variables (`ns`, `region`, `env`) for `var:` |
| `code/request-handler`, `code/retry-policy` | `repo:acme/api`; `code/auth-middleware` uses `acme/gateway` |
| `prompt/postmortem-template` | `gateway` appears only in its notes |

The directory holds only entries and `queries.tsv` (kb ignores non-`.md` files), so this description lives here and not in a `README.md` that would itself count as an entry.

## `tui/`: the TUI preview KB

A third lint-clean KB (seven entries) for the M3 preview and snapshot tests: a long Sumo query with
Markdown notes, a command with `enum`, `text` and `secret` variables and an escaped `\{{literal}}`,
a prompt whose title mixes CJK and emoji, a link with a very long URL, a code entry with a symbol and
an end line, a SQL entry with comments, strings and numbers, and a CloudWatch entry with log groups.
Every entry has an `updated:` date so the preview's "edited" value does not depend on file times.
