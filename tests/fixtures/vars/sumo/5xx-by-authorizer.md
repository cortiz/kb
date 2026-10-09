---
kb_format: 1
type: sumo
title: 5xx by authorizer
tags: [auth]
vars:
  env: { kind: enum, options: [prod, stage, dev], description: Environment }
  minutes: { kind: number, default: 15 }
---

```
_sourceCategory={{env}}/api | where status >= 500 | timeslice {{minutes}}m | count by authorizer
```

Which authorizer returned the 5xx responses.
