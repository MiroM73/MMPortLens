# Resolve remote addresses to host names

## Summary
Show host names instead of / next to remote IP addresses, like TCPView's "Resolve addresses".

## Details
- New column **Remote host** (and optionally Local host), plus a toggle "Resolve addresses" in the View menu / toolbar.
- Reverse lookups run in a small worker pool (never on the UI thread), with a cache (positive and negative results, TTL e.g. 10 min) and a limit on parallel queries.
- Windows: `GetNameInfoW`; Linux: `getnameinfo`. Unresolved addresses show the IP.
- Known well-known ports → service name column or tooltip (`443` → https), from a built-in table (no `services` file dependency).
- Filters and sorting work on the resolved names.

## Acceptance
- Hundreds of distinct remote addresses resolve without freezing the UI.
- Cache tested with a fake resolver.
