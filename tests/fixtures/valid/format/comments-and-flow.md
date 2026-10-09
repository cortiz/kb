---
# Comments are kept when kb rewrites a file.
kb_format: 1   # format version
type: 'sql'
title: "Quoting styles: single, double and plain"
tags: [ yaml , "style" ]
dialect: mysql
vars: { limit: { kind: number, default: 10 } }  # flow map
---

```sql
select * from t limit {{limit}};
```
