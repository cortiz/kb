---
kb_format: 1
type: runbook
title: Runbook with Type Mismatch Binding
vars:
  env: { kind: text }
steps:
  - id: check
    title: Deploy with text env to enum step
    entry: cmd/deploy-authorizer
    vars: { env: "{{env}}" }
---
