---
kb_format: 1
type: cmd
title: Tail pod logs
tags: [k8s, prod]
shell: bash
vars:
  ns: { kind: text, default: default }
  region: { kind: text, default: eu-west-1, description: Cluster region }
---

```
kubectl -n {{ns}} logs -f --tail=100 -l app=api   # {{region}}
```

Follow logs for the API pods.
