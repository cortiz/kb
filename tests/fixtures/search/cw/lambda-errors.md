---
kb_format: 1
type: cw
title: Lambda errors
tags: [prod, lambda]
log_groups: [/aws/lambda/worker]
---

```
fields @timestamp, @message | filter @message like /ERROR/ | limit 50
```

Recent errors from the worker function.
