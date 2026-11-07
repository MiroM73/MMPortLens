# Keyboard shortcuts and multi-selection

## Summary
Basic comfort known from TCPView and RusTrace.

## Details
- Shortcuts: `F5` refresh now, `Space` pause / resume, `Ctrl+F` focus the filter, `Esc` clear the filter, `Ctrl+C` copy selected rows, `Ctrl+A` select all, `Ctrl+E` export, `Del` close connection (with confirmation), arrows / PgUp / PgDn / Home / End move the selection.
- Multi-selection with Ctrl / Shift click (reuse `app/selection.rs` of RusTrace); selection is kept by `ConnKey` across refreshes.
- Help → Keyboard shortcuts window (`docs/keyboard-shortcuts.md`).

## Acceptance
- All shortcuts documented and translated; selection survives refreshes and re-sorting.
