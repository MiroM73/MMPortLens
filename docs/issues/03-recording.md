# Record connection events for a period of time

## Summary
Record what happens on the network for a chosen time (e.g. while reproducing a problem) and review it afterwards. Polling-based TCPView has nothing like it.

## Details
- Toolbar / File menu: **Start recording…** with options: duration (until stopped, 1 / 5 / 15 / 60 min, custom), target file, which events (new, closed, state changes), optional filter (only rows matching the current filters).
- Every refresh produces events from the `Tracker` (`Changes` already counts them): time, event type, protocol, local / remote address and port, state before / after, PID, process name, path.
- Storage: JSON Lines (`.plrec`, one event per line, a header line with computer name, start, interval, version) – simple, appendable, crash-safe. Optional CSV / Excel export of a recording.
- Writing happens in the poll thread or a writer thread, never on the UI thread.
- Status bar shows ● REC with elapsed / remaining time and number of events; stop automatically at the end of the duration.
- **File → Open recording…** shows the events in the same table (read-only mode) with a time column, filters and sorting; a time slider shows the state of the connections at a chosen moment (replay).
- Command line: `mmportlens --record out.plrec --duration 10m [--filter …]` for headless recording (see the command-line issue).

## Notes
- Short connections between two refreshes are missed by polling – combine with the ETW issue for a complete record.

## Acceptance
- Unit tests for the event generation and the file format (write → read round trip).
- A 1-hour recording at 1 s interval with a busy machine stays small (< 50 MB) and the UI stays responsive.
