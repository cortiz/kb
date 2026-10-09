`<name>.in.txt` is a pasted snippet, `<name>.out.txt` the body after accepting every suggestion in
order, and `<name>.vars.txt` the declarations that accepting adds (`name|kind|default|options`).
`none/*.txt` must produce no suggestion at all.

A time token inside a `timeslice 1m` also counts (`1m` is a relative range); the patterns are
deliberately simple, not smart. A run of nine or more digits is an `id`, whether it has 11 or 12
digits, unless it is in an ARN or after `account`.
