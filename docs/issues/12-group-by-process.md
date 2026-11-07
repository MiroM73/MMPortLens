# Group rows by process

## Summary
Optional tree view: one collapsible node per process (name, PID, number of connections, sum of traffic), its endpoints underneath.

## Details
- Toggle in the toolbar ("Group by process"); sorting applies within groups, groups sorted by the same column (count / name).
- Collapsed state remembered per process name.
- Works together with filters (a group shows only matching rows; empty groups hidden).

## Acceptance
- Smooth with 5 000 rows / 300 processes.
