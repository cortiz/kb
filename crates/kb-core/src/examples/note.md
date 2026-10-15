---
kb_format: 1
type: note
title: Authorizer role cache
tags: [example, auth]
---

# Authorizer role cache

The authorizer keeps each user's roles in Redis for 300 seconds, so the
primary database only sees one lookup per user per five minutes.

## What to watch

- 5xx rate: [[sumo/example-5xx-by-authorizer]]
- Cache hit ratio below 90% usually means a deploy flushed Redis

| Event        | Action      | TTL       |
|:-------------|:------------|:---------:|
| Role granted | `SETEX`     | 300 s     |
| Role revoked | `DEL`       | immediate |

> [!WARNING]
> Do not lower the TTL below 60 s during an incident without asking the team.
