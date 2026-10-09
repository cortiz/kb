---
kb_format: 1
type: sql
title: Active connections
tags: [db, perf]
dialect: postgres
database: orders
---

```
select pid, state, query from pg_stat_activity where state = 'active'
```

Connections doing work right now.
