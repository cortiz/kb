---
kb_format: 1
type: runbook
title: Binding unknown var
steps:
  - id: step-1
    title: Step one
    entry: runbook-target
    vars:
      env: "{{unknown_rb_var}}"
      version: "v1.0"
---
