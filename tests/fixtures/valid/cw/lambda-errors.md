---
kb_format: 1
type: cw
title: Lambda errors in the last hour
tags: [lambda, cloudwatch]
log_groups:
  - "/aws/lambda/{{env}}-authorizer"
  - /aws/lambda/shared-auth
range: -1h
region: us-east-1
vars:
  env:
    kind: enum
    options: [prod, stage]
    default: stage
    description: Deployment environment
---

```
fields @timestamp, @message
| filter @message like /ERROR/
| sort @timestamp desc
| limit 50
```
