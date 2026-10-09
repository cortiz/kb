---
kb_format: 1
type: cw
title: Latency p99 production
tags: [prod, latency]
log_groups: [/aws/lambda/api]
---

```
fields @duration | stats pct(@duration, 99) by bin(1h)
```

Tail latency in production.
