---
kb_format: 1
type: note
title: Role cache TTL and Redis architecture
tags: [auth, redis, architecture]
created: 2026-10-14
updated: 2026-10-14
---

# Role cache TTL and Redis architecture

The authorizer caches user roles in Redis for 300 seconds to protect the PostgreSQL primary.

## Architecture

See operational telemetry in [[sumo/5xx-by-authorizer]].

### Cache invalidation table

| Event | Action | TTL |
|:------|:-------|:---:|
| Role Revoke | Redis DEL | Immediate |
| Role Grant | Redis SETEX | 300 s |
| Session Expiry | Evicted | Natural |

> [!WARNING]
> Never set TTL lower than 60 seconds during active incidents without team approval.

- [ ] Check the hit ratio
- [x] Read the runbook

```python
# Check Redis TTL directly
import redis
r = redis.Redis(host="redis.internal", port=6379)
print(r.ttl("role:user_42"))
```

Strike ~~this~~ and a footnote[^1].

[^1]: Footnote text.
