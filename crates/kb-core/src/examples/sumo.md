---
kb_format: 1
type: sumo
title: 5xx by authorizer
tags: [example, lambda, auth]
range: "-{{minutes}}m"
vars:
  env: { kind: enum, options: [prod, stage, dev], default: prod }
  minutes: { kind: number, default: 15 }
---

```
_sourceCategory={{env}}/lambda/authorizer
| where status >= 500
| timeslice 1m
| count by _timeslice, errorCode
```

Variables can also appear in `range`. An `enum` variable lists its
`options`, and its `default` must be one of them.
