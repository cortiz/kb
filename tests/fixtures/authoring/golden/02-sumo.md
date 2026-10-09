---
kb_format: 1
type: sumo
title: 5xx by authorizer
tags: [lambda, auth]
vars:
  env: { kind: text }
  minutes: { kind: number }
created: 2026-10-09T12:00:00Z
updated: 2026-10-09T12:00:00Z
---

```
_sourceCategory={{env}}/lambda/authorizer
| where status >= 500
| timeslice {{minutes}}m
| count by _timeslice, errorCode
```
