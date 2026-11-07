# Close a connection, end a process

## Summary
Context menu actions known from TCPView: **Close connection** and **End process**.

## Details
- Close connection: `SetTcpEntry` with `MIB_TCP_STATE_DELETE_TCB` (IPv4 only – Windows has no IPv6 equivalent; show the action disabled with a tooltip for IPv6 / UDP).
- End process: `OpenProcess(PROCESS_TERMINATE)` + `TerminateProcess`, with a confirmation dialog showing the process name, PID and path.
- Both need administrator rights for most targets – show the error from Windows in the status bar; optional "Restart as administrator" menu item (ShellExecute with `runas`).
- Works on a multi-selection (after the keyboard / selection issue).

## Acceptance
- Closing a test connection makes it disappear in the next refresh (marked closed).
