---
kb_format: 1
type: sql
title: Authorized users
tags: [auth, db]
dialect: postgres
---

```
select id, email from users where authorized = true
```

Users with access granted.
