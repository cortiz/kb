---
kb_format: 1
type: cw
title: Queue depth
tags: [prod, sqs]
log_groups: [/aws/lambda/consumer]
---

```
fields @timestamp, depth | sort @timestamp desc | limit 20
```

Messages waiting in the queue.
