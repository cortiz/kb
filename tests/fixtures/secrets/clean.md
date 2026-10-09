---
kb_format: 1
type: cmd
title: Things that look like secrets but are not
vars:
  token: { kind: secret, env: API_TOKEN }
  password: { kind: secret }
---

```
curl -H "Authorization: Bearer {{token}}" -u "admin:{{password}}" https://example.com
export TOKEN=$API_TOKEN
export API_KEY=${API_KEY}
# password: changeme_please
psql -c "select * from commits where sha = 'a94a8fe5ccb19ba61c4c0873d391e987982fbbd3'"
```
