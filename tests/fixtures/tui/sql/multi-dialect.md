---
kb_format: 1
type: sql
title: Orders by status with comments
tags: [db, orders]
updated: 2026-09-01
dialect: postgres
database: orders
---

```sql
/* count orders
   per status */
select status, count(*) as n   -- how many
from orders
where created_at > now() - interval '7 days'
  and status <> 'cancelled'
group by status
order by n desc
limit 50;
```

Run on the replica, not the primary.
