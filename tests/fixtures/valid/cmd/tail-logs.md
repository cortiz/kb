---
kb_format: 1
type: cmd
title: Tail pod logs
tags: [k8s]
shell: bash
cwd: ~/work
vars:
  namespace: { kind: text, default: default }
  app: { kind: text, description: The app label }
  token: { kind: secret, env: KUBE_TOKEN }
---

```bash
kubectl --token "{{token}}" -n {{namespace}} logs -f -l app={{app}} --tail=100
```
