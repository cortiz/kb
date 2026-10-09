---
kb_format: 1
type: sql
title: Active connections by state
tags: [postgres, ops]
dialect: postgres
database: "{{db}}"
vars:
  db: { kind: enum, options: [app, reporting], default: app }
---

Counts sessions per state on database `{{db}}`.

```sql
select state, count(*)
from pg_stat_activity
where datname = '{{db}}'
group by state
order by 2 desc;
```
