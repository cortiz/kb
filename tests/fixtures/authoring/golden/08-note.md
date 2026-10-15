---
kb_format: 1
type: note
title: Role cache TTL
tags: [redis, private]
created: 2026-10-09T12:00:00Z
updated: 2026-10-09T12:00:00Z
---

# Role cache TTL

The authorizer caches roles in Redis for 300 s.

| Event | TTL |
|:--|--:|
| Grant | 300 s |
