---
kb_format: 1
type: sql
title: T
vars:
  a: { kind: enum, options: [x, y], default: z }
---

```
select '{{a}}';
```
