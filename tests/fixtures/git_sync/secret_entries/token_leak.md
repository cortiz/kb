---
kb_format: 1
id: test/token-leak
title: Bearer Token Leak Test
type: cmd
tags: [auth]
---
curl -H 'Authorization: Bearer abcdefghijklmnopqrstuvwxyz012345' https://api.example.com
