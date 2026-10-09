# Quickstart: validating Authoring

Run from the repo root. `kb` is `cargo run -q --bin kb --`; `KB=$(mktemp -d)` below is a throwaway KB.
Prerequisites: a built workspace, `git`, and on Linux `xclip` or `wl-clipboard` for scenarios 1 and 2.

```sh
export KB_ROOT=$(mktemp -d) && kb init
```

## 1. Capture from the clipboard in the TUI (US1)

```sh
printf '%s\n' 'fields @timestamp, @message' '| filter @message like /{{fn}}/' \
  '| stats pct(@duration, 99) by bin({{ms}}ms)' | pbcopy      # or wl-copy / xclip -selection clipboard
kb          # Ctrl-N: type cw detected, {{fn}} and {{ms}} listed, focus on Title
            # type "Authorizer p99 latency", Tab, "lambda, cw", Ctrl-S
```

Expected: key bar `Saved cw/authorizer-p99-latency`, the entry is selected; `kb lint` exits 0; the
file starts with `kb_format: 1`, keys in the canonical order, `ms` declared `kind: number`.

## 2. Capture from the shell (US2)

```sh
echo 'kubectl rollout restart deploy/api' | kb add -t "Restart api" --tag k8s --yes   # prints cmd/restart-api
kb add --yes -t "No type" < /dev/null; echo $?                                          # 1: could not tell the type
kb add --yes < some.sql; echo $?                                                       # 1: a title is required
kb add --yes -t "Q" --type sql --json < some.sql | jq .id
kb add --stdin --clipboard; echo $?                                                    # 2: conflicting flags
```

## 3. Editor template (US3)

```sh
EDITOR="sh -c 'echo \"SELECT 1;\" >> \"\$0\"'" kb add --editor --type sql     # scripted editor appends to the file
EDITOR=true kb add --editor --type sql; echo $?                               # 130: unchanged template
```

Interactive: `kb add --editor --type sql`, leave the title empty, save: the `e / s / d` prompt appears;
`s` writes it; adding an `AKIA…` fixture key removes `s` from the choices.

## 4. Edit, move, delete (US4)

```sh
kb edit "restart api"                 # one match: opens the editor; clean save bumps `updated` only
kb mv cmd/restart-api ops/restart-api # usage and remembered values follow; (git mv) inside a git KB
kb rm ops/restart-api --yes ; echo $? # 0; file, usage and remembered values gone
kb mv cmd/a cmd/b; echo $?            # 1 when cmd/b exists
```

Check SC-005: `kb search --json | jq '.[].id'` before and after `kb mv` lists the entry in the same
position.

## 5. Variable suggestions (US5, SC-006)

In the add form, paste `_sourceCategory=prod/lambda/authorizer | where ts > now() - -30m`: suggestions
`prod → {{env}} enum` and `30 → {{minutes}} number` appear; nothing changes until `Space`. After accepting
both and saving, `kb use <id> --yes --print` prints the original text exactly.

## 6. Automated

```sh
cargo test -p kb-core author            # detection corpus (SC-002), suggestions, render, check, fsops
cargo test -p kb --test cli_add --test cli_edit --test cli_mv_rm
cargo test -p kb --test tui_add_pty --test authoring_secrets --test platform_boundary
cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --check
```

## Success criteria map

| SC | Where checked |
|---|---|
| SC-001 | `tui_add_pty`: key-press count and elapsed time with the fake clipboard |
| SC-002 | `author::detect` corpus test: ≥ 100 snippets, ≥ 95% right, no wrong `High` |
| SC-003 | golden tests: render → `kb lint` clean → parse → `to_bytes` identical |
| SC-004 | `authoring_secrets`: fixture secrets through form, `--yes`, editor template, `kb edit` → never on disk |
| SC-005 | `cli_mv_rm`: frecency order before and after `mv` |
| SC-006 | suggestion table test: accept all, resolve with defaults, compare to the pasted text |
