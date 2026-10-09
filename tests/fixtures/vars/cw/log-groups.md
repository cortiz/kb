---
kb_format: 1
type: cw
title: Errors for a service
log_groups: ["/aws/lambda/{{svc}}"]
vars:
  svc: { kind: text }
---

```
fields @timestamp, @message | filter @message like /ERROR/
```
