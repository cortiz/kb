---
kb_format: 1
type: sql
title: Active connections by state
tags: [postgres, ops]
created: 2026-10-09T12:00:00Z
updated: 2026-10-09T12:00:00Z
---

```sql
-- Active connections
select state, count(*)
from pg_stat_activity
group by state;
```

Counts sessions per state.
