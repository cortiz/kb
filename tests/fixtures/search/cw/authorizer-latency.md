---
kb_format: 1
type: cw
title: Authorizer latency
tags: [auth, prod, latency]
log_groups: [/aws/lambda/authorizer]
region: eu-west-1
---

```
fields @timestamp, @duration | stats avg(@duration), max(@duration) by bin(5m)
```

Latency of the token authorizer function.
