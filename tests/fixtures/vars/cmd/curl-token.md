---
kb_format: 1
type: cmd
title: Call the API
shell: bash
vars:
  token: { kind: secret, env: KB_TEST_TOKEN }
  path: { kind: text, default: health }
---

```
curl -H "Authorization: Bearer {{token}}" https://api.example.com/{{path}}
```
