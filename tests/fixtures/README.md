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
