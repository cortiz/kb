---
kb_format: 1
type: cw
title: Authorizer p99 latency
tags: [lambda, cloudwatch, prod]
log_groups:
  - "/aws/lambda/prod-authorizer"
  - /aws/lambda/shared-auth
range: -30m
region: us-east-1
vars:
  ms:
    kind: number
    default: 200
---

```
filter @duration > {{ms}}
| stats percentile(@duration, 99) by bin(5m)
```

Monitor latency spikes in authorizer.
