---
kb_format: 1
type: cw
title: API gateway 5xx
tags: [prod]
log_groups: [/aws/apigateway/access]
---

```
fields @timestamp, status | filter status >= 500 | limit 100
```

Look at gateway timeouts first.
