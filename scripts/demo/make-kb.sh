#!/bin/sh
# Builds the throwaway KB the demo GIF is recorded against: scripts/demo/make-kb.sh <dir>
set -eu
root=${1:?usage: make-kb.sh <dir>}
rm -rf "$root"
mkdir -p "$root/.kb" "$root/sumo" "$root/cw" "$root/sql" "$root/cmd" "$root/prompt" "$root/link" "$root/notes"
echo '*' > "$root/.kb/.gitignore"

cat > "$root/sumo/5xx-by-authorizer.md" <<'E'
---
kb_format: 1
type: sumo
title: 5xx by authorizer
tags: [lambda, auth, prod]
range: "-{{minutes}}m"
vars:
  env: { kind: enum, options: [prod, stage, dev], default: prod }
  minutes: { kind: number, default: 15 }
---

```
_sourceCategory={{env}}/lambda/authorizer
| where status >= 500
| timeslice 1m
| count by _timeslice, errorCode
```

A spike in TOKEN_EXPIRED usually means the role cache TTL drifted.
E

cat > "$root/sumo/slow-requests.md" <<'E'
---
kb_format: 1
type: sumo
title: Slowest API requests
tags: [api, latency]
range: "-1h"
---

```
_sourceCategory=prod/api
| where duration_ms > 1000
| top 20 path by duration_ms
```
E

cat > "$root/cw/lambda-errors.md" <<'E'
---
kb_format: 1
type: cw
title: Lambda errors by function
tags: [lambda, errors]
log_groups: ["/aws/lambda/authorizer"]
region: us-east-1
range: "-1h"
vars:
  fn: { kind: text, default: authorizer }
---

```
fields @timestamp, @message
| filter @message like /ERROR/ and @message like /{{fn}}/
| stats count() by bin(5m)
```
E

cat > "$root/sql/active-connections.md" <<'E'
---
kb_format: 1
type: sql
title: Active connections by state
tags: [postgres, ops]
dialect: postgres
---

```sql
select state, count(*)
from pg_stat_activity
group by state
order by count(*) desc;
```
E

cat > "$root/sql/slow-queries.md" <<'E'
---
kb_format: 1
type: sql
title: Slowest queries
tags: [postgres, performance]
dialect: postgres
vars:
  limit: { kind: number, default: 10 }
---

```sql
select query, calls, mean_exec_time
from pg_stat_statements
order by mean_exec_time desc
limit {{limit}};
```
E

cat > "$root/cmd/tail-logs.md" <<'E'
---
kb_format: 1
type: cmd
title: Tail pod logs
tags: [k8s, ops]
shell: bash
vars:
  namespace: { kind: text, default: default }
  app: { kind: text, description: Value of the app label }
---

```bash
kubectl -n {{namespace}} logs -f -l app={{app}} --tail=100
```
E

cat > "$root/prompt/review-diff.md" <<'E'
---
kb_format: 1
type: prompt
title: Review this diff
tags: [review, ai]
---

```
Review the diff below for bugs, missing tests and unclear names.
Be specific: quote the line, say what breaks, suggest the fix.
```
E

cat > "$root/link/grafana-latency.md" <<'E'
---
kb_format: 1
type: link
title: Grafana latency dashboard
tags: [dashboards, prod]
url: https://grafana.example.com/d/latency
---
E

cat > "$root/notes/redis-role-cache-ttl.md" <<'E'
---
kb_format: 1
type: note
title: Redis role cache TTL
tags: [redis, architecture]
---

# Redis role cache TTL

The authorizer caches user permissions in Redis for **300 seconds** to protect the primary database.

## Invalidation rules

- Roles revoked: immediate `DEL`
- Session timeout: natural eviction

When 5xx errors spike, start with [[sumo/5xx-by-authorizer]].
E

cat > "$root/sumo/bad.md" <<'E'
---
kb_format: 1
type: sumologic
title: Old authorizer query
---

```
_sourceCategory=prod/lambda/authorizer
```
E
