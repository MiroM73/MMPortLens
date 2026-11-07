# Column order by drag and drop, remembered column widths

## Summary
The skeleton can already hide / show columns and move them left / right (View → Columns, right click on a header). Make it comfortable and persistent.

## Details
- Drag a column header to a new position (drop indicator between headers).
- Remember column widths per column in `Config` (today the widths reset when the column set changes, because the table id contains the visible columns).
- Double click on the header border = fit the width to the content.
- "Reset columns" also resets widths.
- Column window: drag and drop in the list as well as the ⏴ ⏵ buttons.

## Acceptance
- Order and widths survive a restart; old config files still load (`ColumnLayout::normalize`).
