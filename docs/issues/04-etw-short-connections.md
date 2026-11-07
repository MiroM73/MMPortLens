# Capture short-lived connections with ETW (Windows)

## Summary
Polling (`GetExtendedTcpTable`) misses every connection that opens and closes between two refreshes – exactly the ones that are often interesting (malware beacons, failing health checks, DNS over TCP). TCPView has the same blind spot. Event Tracing for Windows can see them all.

## Details
- Real-time ETW session on the kernel network provider (Microsoft-Windows-Kernel-Network, `{7DD42A49-5329-4832-8DFD-43D979153A88}`, TCP connect / accept / disconnect, UDP send / receive events) or the TCPIP provider.
- Requires administrator rights – show a clear hint and fall back to polling when not elevated.
- Events are merged into the `Tracker`: a connection seen only by ETW appears as a new row (marked e.g. with a ⚡ icon "seen by ETW only") and then as closed.
- Optional per-connection byte counters from the send / receive events.
- Run the session in its own thread; stop it cleanly on exit (orphaned sessions stay in the system otherwise).
- Evaluate crates (`ferrisetw`) vs. direct `StartTrace` / `OpenTrace` / `ProcessTrace` bindings.

## Acceptance
- A test program that opens and closes 100 connections within 50 ms each: all of them show up (recorded or highlighted).
- Switch in View → "Capture short connections (ETW, admin)".
