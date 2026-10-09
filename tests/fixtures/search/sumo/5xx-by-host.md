---
kb_format: 1
type: sumo
title: 5xx errors by host
tags: [prod, http]
---

```
_sourceCategory=prod/web | where status >= 500 | count by host
```

Spot a bad host quickly.
