---
kb_format: 1
type: cmd
title: T
---

```
curl -H 'Authorization: Bearer abcdefghijklmnopqrstuvwxyz012345' https://example.com # kb:allow-secret
psql -c "select 'Bearer abcdefghijklmnopqrstuvwxyz012345'" -- kb:allow-secret
```
