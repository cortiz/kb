---
kb_format: 1
type: sql
title: Find a customer
vars:
  name: { kind: text, escape: sql }
---

```sql
select * from customers where name = {{name}};
```
