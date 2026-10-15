---
kb_format: 1
type: cmd
title: Helper entry for runbook tests
vars:
  env: { kind: enum, options: [stage, prod], default: stage }
  version: { kind: text, default: "v1.0" }
---

```sh
echo "{{env}} {{version}}"
```
