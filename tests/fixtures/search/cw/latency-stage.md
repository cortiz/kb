---
kb_format: 1
type: cw
title: Latency p99 staging
tags: [stage, latency]
log_groups: [/aws/lambda/api-stage]
---

```
fields @duration | stats pct(@duration, 99) by bin(1h)
```

Tail latency in staging.
