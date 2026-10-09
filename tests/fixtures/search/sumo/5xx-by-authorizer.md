---
kb_format: 1
type: sumo
title: 5xx by authorizer
tags: [auth, lambda]
updated: 2026-09-30
---

```
_sourceCategory=prod/api | where status >= 500 | count by authorizer
```

Which authorizer returned the 5xx responses.
