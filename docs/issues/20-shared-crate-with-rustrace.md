# Shared code with RusTrace in a common crate

## Summary
RusNet copies modules from RusTrace (i18n, column filters, table export, update, CLI helpers). Once two or three are copied, move them into a shared crate so fixes land in both programs.

## Details
- Candidates: `i18n.rs` (identical except the folder name – make the app name a parameter), `colfilter.rs`, `tabexport/`, `update.rs` + `http.rs`, theme / palette helpers.
- Options: a separate repository `MiroM73/rus-common` used as a git dependency, or a Cargo workspace. Decide after the column filters and export are ported.
- Keep the language files per application (they differ), share only the mechanism.

## Acceptance
- Both projects build against the shared crate; their tests still pass.
