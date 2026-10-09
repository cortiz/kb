# Quickstart: Index and Search

Runnable checks that prove M2 works end to end. Commands assume a release build on `PATH` as `kb`,
run from the repository root. Contracts: [cli.md](contracts/cli.md),
[query-syntax.md](contracts/query-syntax.md).

## Setup

```sh
cargo build --release && export PATH="$PWD/target/release:$PATH"
export KB_ROOT="$(mktemp -d)/kb"
cp -R tests/fixtures/search "$KB_ROOT"            # the M2 fixture KB (every type, accents, typos)
kb init "$KB_ROOT" --no-examples
```

## 1. Find an entry (US1)

```sh
kb search 5xx auth | head -1
# expected: first line is the sumo/5xx-by-authorizer entry (stderr: index rebuilt)
kb search auth --ids | head -3        # IDs only, no escape codes
kb search auth --json | jq '.results[0] | {id, tier, snippet: .snippet.text}'
kb search zzzznotfound; echo "exit=$?"
# expected: stdout empty, stderr "kb: no matches", exit=1
```

## 2. Freshness without commands (US2)

```sh
kb search 5xx auth --ids | head -1
sed -i.bak 's/5xx by authorizer/gateway timeouts by authorizer/' "$KB_ROOT/sumo/5xx-by-authorizer.md"
kb search gateway timeouts --ids          # expected: the edited entry
mv "$KB_ROOT/sumo/5xx-by-authorizer.md" "$KB_ROOT/sumo/renamed.md"
kb search renamed --ids                   # expected: sumo/renamed
rm "$KB_ROOT/sumo/renamed.md"
kb search renamed --ids; echo "exit=$?"    # expected: nothing, exit=1
rm -f "$KB_ROOT/.kb/index.db"
kb search auth --ids | head -1            # expected: stderr "kb: index rebuilt (N entries)"
printf 'garbage' > "$KB_ROOT/.kb/index.db"
kb search auth --ids | head -1            # expected: rebuilt again, no prompt
```

## 3. Filters and phrases (US3)

```sh
kb search latency type:cw                 # only cw entries
kb search tag:prod tag:auth               # entries with both tags
kb search auth -tag:stage                 # excludes stage
kb search '"role cache"'                  # exact phrase
diff <(kb search --type cw --tag prod --ids) <(kb search 'type:cw tag:prod' --ids) && echo same
kb search --ids | head -3                 # empty query: all entries, most used first
```

## 4. Forgiving search (US4)

```sh
kb search authori --ids | head -1         # prefix
kb search authroizer --ids | head -1      # one transposition (5+ chars), ranks below exact
kb search configuracion --ids | head -1   # accents and case
```

## 5. Index management (US5)

```sh
kb index status
kb index status --json | jq .entries_indexed
kb reindex
```

## 6. Usage affects order, search does not record it

```sh
# in a Rust test: Kb::record_use("sql/b", now) x5, then the empty query lists sql/b first;
# `kb search` repeated 10 times leaves .kb/state.json byte-identical (FR-017).
```

## 7. Read-only `.kb/` and corrupt state

```sh
chmod -w "$KB_ROOT/.kb"; rm -f "$KB_ROOT/.kb/index.db" 2>/dev/null
kb search auth --ids | head -1            # expected: stderr warning about in-memory index
chmod +w "$KB_ROOT/.kb"
printf '{' > "$KB_ROOT/.kb/state.json"; kb search auth --ids >/dev/null
ls "$KB_ROOT/.kb/state.json.bak"          # expected: backup exists, one warning printed
```

## Automated checks

```sh
cargo fmt --check && cargo clippy --all-targets -- -D warnings
cargo test                                # incl. search_table (SC-004), freshness, concurrency (SC-005)
cargo test --release -- --ignored perf    # SC-001 to SC-003 on 10,000 generated entries
```

| Check | Proves |
|-------|--------|
| `tests/search_table.rs` over `queries.tsv` (≥ 40 rows) | SC-004: ≥ 90% first, 100% top three |
| `crates/kb/tests/freshness.rs` | US2, SC-006 |
| `crates/kb/tests/concurrency.rs` | SC-005, FR-004 |
| `crates/kb/tests/cli_search.rs` | US1, exit codes, `--ids`/`--json`, `NO_COLOR` |
| `crates/kb/tests/perf.rs` (ignored by default) | SC-001, SC-002, SC-003 |
