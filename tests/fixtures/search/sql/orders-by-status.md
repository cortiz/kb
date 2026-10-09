---
kb_format: 1
type: sql
title: Orders by status
tags: [db]
dialect: postgres
---

```
select status, count(*) from orders group by status
```

Order counts per status.
