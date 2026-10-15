---
kb_format: 1
type: runbook
title: Invalid binding
vars:
  env: { kind: text }
steps:
  - id: step-1
    title: Step one
    entry: runbook-target
    vars:
      env: "{{env}}"
      version: "v1.0"
---
