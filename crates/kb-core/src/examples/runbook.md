---
kb_format: 1
type: runbook
title: Deploy authorizer to prod
tags: [example, deploy, auth]
vars:
  env: { kind: enum, options: [stage, prod], default: stage }
  token: { kind: secret, env: KUBE_TOKEN }
steps:
  - id: check-5xx
    title: Check 5xx rate
    entry: sumo/example-5xx-by-authorizer
    vars: { minutes: 30 }
  - id: watch-pods
    title: Watch pod logs
    entry: cmd/example-tail-pod-logs
    vars:
      app: authorizer
      token: "{{token}}"
  - id: verify
    title: Verify healthy response
    do: |
      Send a test healthcheck curl to the authorizer endpoint in {{env}}.
      Confirm HTTP 200 OK before signing off.
---

Example deploy procedure using queries and commands from this KB.
