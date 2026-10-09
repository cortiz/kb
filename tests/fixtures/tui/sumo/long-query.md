---
kb_format: 1
type: sumo
title: Long Sumo query with sixty lines
tags: [prod, logs, long]
updated: 2026-09-30
range: "-15m"
---

```
_sourceCategory=prod/api
| where status >= 401 | count by host_1
| where status >= 402 | count by host_2
// stage 3: filter and count
| where status >= 404 | count by host_4
| where status >= 405 | count by host_5
// stage 6: filter and count
| where status >= 407 | count by host_7
| where status >= 408 | count by host_8
// stage 9: filter and count
| where status >= 410 | count by host_10
| where status >= 411 | count by host_11
// stage 12: filter and count
| where status >= 413 | count by host_13
| where status >= 414 | count by host_14
// stage 15: filter and count
| where status >= 416 | count by host_16
| where status >= 417 | count by host_17
// stage 18: filter and count
| where status >= 419 | count by host_19
| where status >= 420 | count by host_20
// stage 21: filter and count
| where status >= 422 | count by host_22
| where status >= 423 | count by host_23
// stage 24: filter and count
| where status >= 425 | count by host_25
| where status >= 426 | count by host_26
// stage 27: filter and count
| where status >= 428 | count by host_28
| where status >= 429 | count by host_29
// stage 30: filter and count
| where status >= 431 | count by host_31
| where status >= 432 | count by host_32
// stage 33: filter and count
| where status >= 434 | count by host_34
| where status >= 435 | count by host_35
// stage 36: filter and count
| where status >= 437 | count by host_37
| where status >= 438 | count by host_38
// stage 39: filter and count
| where status >= 440 | count by host_40
| where status >= 441 | count by host_41
// stage 42: filter and count
| where status >= 443 | count by host_43
| where status >= 444 | count by host_44
// stage 45: filter and count
| where status >= 446 | count by host_46
| where status >= 447 | count by host_47
// stage 48: filter and count
| where status >= 449 | count by host_49
| where status >= 450 | count by host_50
// stage 51: filter and count
| where status >= 452 | count by host_52
| where status >= 453 | count by host_53
// stage 54: filter and count
| where status >= 455 | count by host_55
| where status >= 456 | count by host_56
// stage 57: filter and count
| where status >= 458 | count by host_58
| where status >= 459 | count by host_59
```

Use this when **every** stage matters. It is *slow* on big windows; prefer `timeslice` first.

- narrow the window with `range`
- check [the runbook](https://wiki.example.com/runbooks/sumo) before paging

1. run it once
2. read the counts
3. then narrow by host
