# Quickstart: validating M5

Prerequisites: `cargo build --release`; fixture KB `tests/fixtures/vars` (17 entries: one per kind, escape mode and location). Never run against `~/kb`.

```sh
export KB_ROOT=$(mktemp -d)/kb; cp -R tests/fixtures/vars "$KB_ROOT"
```

## Automated

```sh
cargo test --workspace                          # engine, escaping tables, prefill, memory, lint KB054
cargo test -p kb --test cli_use --test cli_vars --test cli_use_pty  # scripted and prompted scenarios, exit codes
cargo test -p kb --test tui_form_pty            # screen 2 end to end
cargo test -p kb --test secrets_scan            # SC-004
cargo test -p kb --test varres_boundary         # FR-004
cargo test -p kb --release --test perf -- --ignored --test-threads=1   # SC-002
```

## By hand (macOS here)

| Step | Command | Expect |
|---|---|---|
| 1 | `kb vars 5xx` | table with kind, options, default, source, pre-fill |
| 2 | `kb use 5xx --var env=prod --var minutes=30; pbpaste` | resolved query, `{{` gone |
| 3 | `kb use 5xx -y --print` (needs `--var env=…` the first time: the enum has no default) | pre-filled values substituted on stdout |
| 4 | `kb use 5xx </dev/null` | exit 1, `missing values:` list |
| 5 | `kb use 5xx --var nope=1` | exit 2, lists `env, minutes` |
| 6 | `kb use cmd/git-checkout --var branch='feature/x; rm -rf ~' --print` | `git checkout 'feature/x; rm -rf ~'` |
| 7 | `kb use link/search --var query='a&b c' --print` | `…q=a%26b%20c` |
| 8 | `KB_TEST_TOKEN=s3cr3t kb use cmd/curl-token -y; pbpaste` | text has the token; stderr has the secret notice |
| 9 | `grep -r s3cr3t "$KB_ROOT"` | no match |
| 10 | `kb use cmd/curl-token` (no env) | masked prompt, nothing echoed |
| 11 | `kb`, select `5xx`, Enter, Tab, `30`, Enter | form, live preview, clipboard matches step 2 |
| 12 | in the form type `abc` in minutes, Enter | no copy, focus stays, message |
| 13 | `kb`, select `5xx`, Alt-C | the template with `{{env}}` is on the clipboard |
| 14 | `kb copy 5xx -y` / `kb copy --raw 5xx` | resolved / template |
| 15 | `kb --print payload` on `5xx`, Enter, fill, Enter | resolved text on stdout |

## Manual matrix (cannot be automated here)

Alt-Enter and Ctrl-Z delivery in Terminal.app, iTerm2, kitty, tmux (default and `xterm-keys`), over
SSH; fish quoting round trip (`fish -c`); masked prompt over SSH. Record results like M4's matrix.
