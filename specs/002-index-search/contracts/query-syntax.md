# Contract: Query syntax

The same grammar is used by `kb search`, the core API (raw string form) and later the TUI search
box and the shell picker.

## Tokens

A query is split on whitespace into tokens, except inside double quotes.

| Token | Meaning |
|-------|---------|
| `word` | Free-text term. All terms must match (AND), each in any field (FR-006). |
| `"two words"` | Exact phrase: the words in order, adjacent, after folding (FR-008). |
| `type:cw` | Entry type is `cw`. Repeating it means OR. Unknown type name → no results plus a stderr hint listing the types. |
| `tag:prod` | Entry has tag `prod`. Repeating it means AND. Case-insensitive, leading `#` ignored. |
| `-tag:stage` | Entry does not have tag `stage`. |
| `var:region` | Entry uses variable `region` (declared or referenced). Repeating it means AND. |
| `repo:acme/api` | Code entry whose `code.repo` equals `acme/api` (case-insensitive). |
| `in:title` `in:payload` `in:notes` | Restrict all free-text terms and phrases to that field. Only one `in:` per query; a later one replaces an earlier one. |
| `foo:bar` (unknown key) | Treated as free text `foo:bar`, plus a hint on stderr. |
| `-word` | Not a negation. A leading `-` is ignored for free text (`-stage` searches `stage`). Only `-tag:` excludes. On the command line a bare `-stage` looks like a flag to the argument parser: write `kb search -- -stage` or `kb search 'x -stage'`. (`-tag:value` is the one exception kb rewrites to `--not-tag value`, so it works anywhere.) |

Keys are case-insensitive (`TYPE:cw` works). Values of `type:` and `tag:` are folded.
A key with an empty value (`tag:`) is treated as free text.

## Matching rules

- **Fold**: lowercase and remove accents on both sides (FR-009). `configuracion` matches
  `Configuración`.
- **Words**: text is split into words at any character that is not a letter or digit (like
  SQLite's `unicode61`). `5xx-by-authorizer` is `5xx`, `by`, `authorizer`.
- **Prefix**: a term matches any word that starts with it (FR-010). `auth` matches `authorizer`.
- **Typo**: a term of five or more characters also matches words within one edit — insert,
  delete, substitute, or swap of adjacent characters (FR-011). `authroizer` matches
  `authorizer`. Typo matches rank below exact and prefix matches. Not applied inside phrases and
  not applied to `type:`/`tag:`/`var:`/`repo:` values.
- **Stop words** (`a an and are as at be by for in is it of on or the to`) are dropped from free
  text terms. Phrases keep them. A query with no terms and no phrases is an *empty query*: all
  entries, ordered by frecency, narrowed by any filters.
- **Punctuation only** (`"???"`, `-`, `:`) yields no terms: an empty query.

## Equivalence with flags (FR-025)

| Flag | Query syntax |
|------|--------------|
| `--type cw` | `type:cw` |
| `--tag prod` | `tag:prod` |
| `--not-tag stage` | `-tag:stage` |

`Query::parse("latency type:cw tag:prod")` equals the query built from terms `["latency"]` and
the flags `--type cw --tag prod`. Flags and inline filters combine (union of values).

## Quoting at the shell

`kb search` joins its arguments with single spaces and then parses. To pass a phrase, quote it for
the shell too:

```sh
kb search role cache            # two terms
kb search '"role cache"'        # one phrase
kb search 'latency type:cw'     # one argument works as well
```

## Errors

An unterminated quote is closed at the end of input (`"role cache` is the phrase `role cache`).
The parser never fails; every string is a valid query.
