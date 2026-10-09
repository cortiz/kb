---
kb_format: 1
type: cmd
title: Deploy with secret token
tags: [deploy, prod]
updated: 2026-09-20
shell: bash
cwd: ~/src/service
vars:
  env: { kind: enum, options: [prod, stage], default: stage }
  region: { kind: text, default: eu-west-1, description: AWS region for the rollout }
  token: { kind: secret, description: Deploy token, read from the environment }
---

```bash
# literal braces stay: echo \{{literal}}
./deploy.sh --env {{env}} --region {{region}} --token "$DEPLOY_TOKEN"   # {{token}}
```

Prompts for the token at use time.
