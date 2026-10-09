---
kb_format: 1
type: cmd
title: Tail pod logs
tags: [example, k8s]
shell: bash
vars:
  namespace: { kind: text, default: default }
  app: { kind: text, description: Value of the app label }
  token: { kind: secret, env: KUBE_TOKEN }
---

```bash
kubectl --token "{{token}}" -n {{namespace}} logs -f -l app={{app}} --tail=100
```

Never paste a real token into an entry. A `secret` variable is read from the
environment variable named in `env`, or prompted for, and is never stored.
