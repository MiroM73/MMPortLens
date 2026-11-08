# Keyboard shortcuts

All keyboard and mouse shortcuts of the main window. The same list is in **Help → Keyboard
shortcuts** (F1).

[← Back to README](../README.md)

| Shortcut | Action |
|---|---|
| F5 | Refresh now (also while paused) |
| Space | Pause / resume the automatic refresh |
| Ctrl+F | Go to the filter field of the toolbar |
| Esc | Close the process properties window; else clear the text filter; when it is empty, clear the selection |
| ↑ ↓ PgUp PgDn Home End | Move in the table |
| Shift + ↑ ↓ PgUp PgDn Home End | Extend the selection from the anchor row |
| Click | Select one row |
| Ctrl + click | Add / remove a row |
| Shift + click, Ctrl+Shift + click | Select / add the range from the anchor row |
| Drag a header | Move the column (Esc cancels); the drop place is shown as a line |
| Drag a header border / double click it | Resize the column / fit it to its content |
| Right click | Context menu (on a row outside the selection: selects only that row first) |
| Shift + mouse wheel | Scroll the table horizontally when the columns do not fit (also the horizontal touchpad gesture) |
| Ctrl+A | Select all rows shown |
| Del | Close the selected connections (asks first; IPv4 TCP only) |
| Alt+Enter | Properties of the process of the cursor row (also **Properties…** in the row context menu) |
| Ctrl+C | Copy the selected rows, tab-separated in the visible columns (a header line for several rows; values with a tab, quote or line break are quoted, so they paste into Excel) |
| Ctrl+S | Export the table (CSV, Excel, JSON Lines, HTML) |
| Ctrl+H | Color rules window (open / close) |
| F1 | Keyboard shortcuts window |

The selection belongs to the connections, not to the positions in the table: it stays when the
table is refreshed, re-sorted or filtered. A closed connection leaves the selection when its row
disappears.

Ctrl+S opens the Export window (File → Export…, #9).

In the confirmation window of Close connection / End process, Enter confirms and Esc cancels.
