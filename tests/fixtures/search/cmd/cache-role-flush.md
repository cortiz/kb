---
kb_format: 1
type: cmd
title: Flush role mapping
tags: [cache]
shell: bash
---

```
redis-cli --scan --pattern 'roles:*' | xargs redis-cli del
```

Clears the cache role mapping so it is rebuilt.
