---
kb_format: 1
type: cmd
title: Repeat a name
shell: bash
vars:
  name: { kind: text, default: web }
---

```
echo {{name}} && echo {{name}}
```
