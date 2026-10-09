---
kb_format: 1
type: sql
title: Active connections by state
tags: [example, postgres]
dialect: postgres
vars:
  db: { kind: text, default: app, description: Database to inspect }
---

Counts open sessions per state. This is an example entry: copy it to start
a new SQL snippet, then run `kb lint` to check your copy.

The payload is the first fenced code block. `{{db}}` in the payload is a
variable, declared above under `vars`.

```sql
select state, count(*)
from pg_stat_activity
where datname = '{{db}}'
group by state
order by 2 desc;
```
