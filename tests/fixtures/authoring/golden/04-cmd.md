---
kb_format: 1
type: cmd
title: Restart api
tags: [k8s]
vars:
  ns: { kind: text }
created: 2026-10-09T12:00:00Z
updated: 2026-10-09T12:00:00Z
---

```sh
kubectl rollout restart deploy/api -n {{ns}}
```
