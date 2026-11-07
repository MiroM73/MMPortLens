# Export the table to CSV, Excel, JSON

## Summary
Export the visible rows (filters and column order respected) like the table export of RusTrace (#32 there).

## Details
- Reuse `src/tabexport/` from RusTrace (CSV, XLSX writer, JSONL, HTML) and its dialog: format, visible / all columns, selected rows only.
- File → Export… and Ctrl+S; "Copy as table" for the selection (tab separated, pastes into Excel).
- Headless: `rusnet --export connections.csv` (see the command-line issue).

## Acceptance
- Export of 5 000 rows to XLSX opens in Excel with correct types (numbers as numbers).
