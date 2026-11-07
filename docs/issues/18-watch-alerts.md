# Watch rules and alerts

## Summary
Be notified when something specific happens, e.g. "any process connects to port 4444", "a new listener appears", "chrome.exe connects outside 10.0.0.0/8".

## Details
- Rules use the column filter syntax plus an event type (new connection, new listener, state change, closed).
- Actions: Windows toast notification, sound, bring window to front, write to a log file, start recording.
- Rule hits listed in an "Alerts" pane with time and a jump to the row.
- Works while minimized (tray icon optional).

## Acceptance
- A test rule on a local port fires once per new connection, not on every refresh.
