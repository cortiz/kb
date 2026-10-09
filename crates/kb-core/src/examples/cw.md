---
kb_format: 1
type: cw
title: Lambda errors in the last hour
tags: [example, cloudwatch]
log_groups:
  - "/aws/lambda/{{function}}"
range: -1h
region: us-east-1
vars:
  function: { kind: text, default: example-function }
---

```
fields @timestamp, @message
| filter @message like /ERROR/
| sort @timestamp desc
| limit 50
```

CloudWatch Logs Insights query. `log_groups` is a list.
