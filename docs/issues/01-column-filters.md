# Column filters under the column headers

## Summary
Add a filter row under the table headers – one text field per visible column – like the column filters in RusTrace (#49 there). This is the main feature TCPView is missing.

## Details
- Reuse `src/colfilter.rs` from RusTrace (syntax, parsing, tests) and adapt it to the RusNet columns.
- Syntax per field:
  - plain text = contains (case-insensitive), `!text` = does not contain,
  - `/regex/` = regular expression,
  - `a|b` = any of several values,
  - numeric columns (PID, ports): `443`, `1000-2000`, `>1024`, `<1024`,
  - address columns: CIDR `10.0.0.0/8`, `fe80::/10`, and keywords `loopback`, `private`, `public`.
- All column filters combine with AND, and with the toolbar text filter and the protocol switches.
- A field with an active filter gets a colored background; a ✖ button clears all filters; View menu item "Show column filters" toggles the row.
- Filters are kept in the settings (optional, a setting "Remember column filters").

## Acceptance
- Pure filter logic in its own module with unit tests (syntax, numeric ranges, CIDR, invalid input = no filter + hint).
- Filtering 5 000 rows stays well under one frame.
