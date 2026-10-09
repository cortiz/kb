---
kb_format: 1
type: sql
title: Slow queries
tags: [db, perf, prod]
dialect: postgres
---

```
select query, mean_exec_time from pg_stat_statements order by 2 desc limit 20
```

Top statements by mean time.
