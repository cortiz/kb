---
kb_format: 1
type: sql
title: T
vars:
  a: { kind: secret, default: x }
---

```
select '{{a}}';
```
