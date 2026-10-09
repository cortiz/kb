---
kb_format: 1
type: cmd
title: A number with an unusable default
shell: bash
vars:
  count: { kind: number, default: 1e3 }
---

```
head -n {{count}} file
```
