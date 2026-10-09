---
kb_format: 1
type: cmd
title: Go template with literal braces
---

```
kubectl get pods -o go-template='\{{range .items}}\{{.metadata.name}}\{{"\n"}}\{{end}}'
```
