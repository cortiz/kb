---
kb_format: 1
type: cmd
title: Deploy service
tags: [deploy, prod]
shell: bash
vars:
  env: { kind: enum, options: [prod, stage], default: stage }
  region: { kind: text, default: eu-west-1 }
---

```
./deploy.sh --env {{env}} --region {{region}}
```

Roll out the service.
