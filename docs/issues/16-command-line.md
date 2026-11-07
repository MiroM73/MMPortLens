# Command line: filters, headless snapshot export and recording

## Summary
Make RusNet usable from scripts and scheduled tasks.

## Details
- Reuse the parser style of RusTrace `src/cli.rs` (help in the UI language, message box vs. console).
- Options: `--filter <text>`, `--preset <name>`, `--interval <ms>`, `--paused`, `--lang <code>`.
- Headless: `--export <file.csv|xlsx|json>` (one snapshot, no window), `--record <file> --duration <10m>` (no window, Ctrl+C stops), exit codes for errors.
- `--help`, `--version`.

## Acceptance
- `rusnet --export x.csv` from PowerShell writes the file and returns 0 without opening a window.
