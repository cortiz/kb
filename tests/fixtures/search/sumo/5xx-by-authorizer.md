---
kb_format: 1
type: sumo
title: 5xx by authorizer
tags: [auth, lambda]
---

```
_sourceCategory=prod/api | where status >= 500 | count by authorizer
```

Which authorizer returned the 5xx responses.
