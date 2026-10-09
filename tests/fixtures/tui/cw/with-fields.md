---
kb_format: 1
type: cw
title: Lambda timeouts by function
tags: [prod, lambda]
updated: 2026-09-12
log_groups: [/aws/lambda/api, /aws/lambda/worker]
region: eu-west-1
range: -1h
---

```
fields @timestamp, @message, @logStream
| filter @message like /Task timed out/
| stats count(*) as n by @logStream
| sort n desc
| limit 20
```

Check the function memory before raising the timeout.
