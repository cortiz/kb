---
kb_format: 1
type: runbook
title: Binding not in entry
steps:
  - id: step-1
    title: Step one
    entry: runbook-target
    vars:
      nonexistent: "foo"
      env: "stage"
      version: "v1.0"
---
