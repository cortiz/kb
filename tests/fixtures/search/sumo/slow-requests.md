---
kb_format: 1
type: sumo
title: Slow requests by endpoint
tags: [prod, latency]
---

```
_sourceCategory=prod/api | where duration_ms > 1000 | count by endpoint
```

Endpoints whose duration exceeds one second.
