---
kb_format: 1
type: cmd
title: Grep a pattern
shell: bash
vars:
  pattern: { kind: text }
---

```
grep "{{pattern}}" app.log
echo '{{pattern}}'
```
