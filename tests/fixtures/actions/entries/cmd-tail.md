---
kb_format: 1
type: cmd
title: Tail authorizer logs
tags: [logs, deploy]
shell: bash
cwd: ~/deploy
vars:
  env:
    kind: enum
    options: [prod, stage]
    default: prod
  token:
    kind: secret
    env: SUMO_TOKEN
---

```sh
tail -f /var/log/{{env}}-authorizer.log
```

Follow authorizer logs on host.
