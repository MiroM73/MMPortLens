# Real creation time and owner module of TCP connections

## Summary
The skeleton shows "First seen" (when MM PortLens saw the endpoint). Windows knows the real creation time and the owner module.

## Details
- Use `TCP_TABLE_OWNER_MODULE_ALL` / `UDP_TABLE_OWNER_MODULE` (rows `MIB_TCPROW_OWNER_MODULE` with `liCreateTimestamp` and `OwningModuleInfo`) instead of the `OWNER_PID` tables; extend the pure decoders in `src/net/winrows.rs` with tests for the new layouts.
- New column **Created** (local time, ms) and **Duration** (how long the connection exists); keep First seen as an option.
- `GetOwnerModuleFromTcpEntry` gives the module / service name – feeds the Service column of the process-details issue.

## Acceptance
- Decoders tested with synthetic buffers; the live test checks the creation time of an own connection is within a second of now.
