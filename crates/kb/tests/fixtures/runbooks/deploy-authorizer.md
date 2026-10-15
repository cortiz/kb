---
kb_format: 1
type: runbook
title: Deploy authorizer to prod
tags: [deploy, lambda, auth]
vars:
  env: { kind: enum, options: [stage, prod], default: stage }
  version: { kind: text, description: "Release tag, e.g. v2.14.0" }
steps:
  - id: baseline
    title: Check current error rate
    entry: sumo/5xx-by-authorizer
    vars: { minutes: 30 }
  - id: deploy
    title: Deploy {{version}} to {{env}}
    entry: cmd/deploy-authorizer
    confirm: true
  - id: watch
    title: Watch p99 latency for 10 minutes
    entry: cw/authorizer-p99-latency
  - id: rollback
    title: Roll back if p99 > 800 ms
    do: |
      Only if p99 stayed above 800 ms for 5 minutes.
      Post in #auth-oncall before rolling back.
    optional: true
---

Owner: platform security. Last rehearsed in stage on 2026-10-02.
