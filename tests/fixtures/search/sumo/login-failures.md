---
kb_format: 1
type: sumo
title: Login failures
tags: [auth, prod]
---

```
_sourceCategory=prod/login | where outcome = "failure" | count by reason
```

Failed sign-in attempts grouped by reason.
