# Statistics: connections over time, top processes and hosts

## Summary
A small dashboard window: how many connections over time and who uses the network.

## Details
- Line chart of established / listening / time-wait counts over the last N minutes (ring buffer of the status counters, `egui_plot`).
- Top processes by number of connections (and by traffic once the traffic issue is done), top remote hosts / ports.
- Click on an item = set it as a filter in the main table.

## Acceptance
- Window opens from View → Statistics, updates live, no measurable cost when closed.
