# Keyboard shortcuts

All keyboard and mouse shortcuts of the main window. The same list is in **Help → Keyboard
shortcuts** (F1).

[← Back to README](../README.md)

| Shortcut | Action |
|---|---|
| F5 | Refresh now (also while paused) |
| Space | Pause / resume the automatic refresh |
| Ctrl+F | Go to the filter field of the toolbar |
| Esc | Clear the text filter; when it is empty, clear the selection |
| ↑ ↓ PgUp PgDn Home End | Move in the table |
| Shift + ↑ ↓ PgUp PgDn Home End | Extend the selection from the anchor row |
| Click | Select one row |
| Ctrl + click | Add / remove a row |
| Shift + click, Ctrl+Shift + click | Select / add the range from the anchor row |
| Right click | Context menu (on a row outside the selection: selects only that row first) |
| Shift + mouse wheel | Scroll the table horizontally when the columns do not fit (also the horizontal touchpad gesture) |
| Ctrl+A | Select all rows shown |
| Del | Close the selected connections (asks first; IPv4 TCP only) |
| Ctrl+C | Copy the selected rows, tab-separated in the visible columns (a header line for several rows) |
| F1 | Keyboard shortcuts window |

The selection belongs to the connections, not to the positions in the table: it stays when the
table is refreshed, re-sorted or filtered. A closed connection leaves the selection when its row
disappears.

Planned: Ctrl+E export (#9).

In the confirmation window of Close connection / End process, Enter confirms and Esc cancels.
