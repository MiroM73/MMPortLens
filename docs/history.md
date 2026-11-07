# MM PortLens – history

## 0.1.0 (2026-11-07) – skeleton
- Project created in the structure of RusTrace: Cargo, eframe/egui 0.37, CI (Windows + Linux),
  i18n module and language files (en, sk, cs, de), settings.
- Reading TCP / UDP endpoints (IPv4 + IPv6) with PIDs: Windows iphlpapi, Linux `/proc/net`.
- Process name and path per PID (cached).
- Background refresh with pause, refresh now and interval; new / changed / closed rows highlighted.
- Table with sorting, columns on / off and their order (View → Columns, right click on a header),
  protocol switches, text filter, copy row, status bar.
- Planned features written as issue drafts in `docs/issues/`.
- Renamed from the working name RusNet to **MM PortLens** (`MMPortLens`) before publishing:
  RusNet collides with RustNet (a Rust network monitor) and the Russian IRC network, and all of the
  owner's programs get the MM prefix.
- Fixed the executable name left over from RusTrace in the comment of `.cargo/config.toml`.
- Fixed `scripts/create-issues.ps1`: `Get-ChildItem -Filter` does not understand `[0-9]`, so the
  script found no drafts; it now matches the `NN-` prefix with a regex. Issues #1–#20 created from it,
  #21 for the invalid TCP states Windows reported on the development PC.
- Release infrastructure from RusTrace (part of #15): `scripts/package.ps1` (SFX `.exe` + `.zip` +
  `.sha256`), `scripts/release.ps1` (with `-SkipTests` for #21) and `.github/workflows/release.yml`;
  README got a Download section. First release **v0.1.0**.
- #21 (parts 1, 2, 4): the live test no longer fails on a PC where Windows reports TCP state 0 – it
  finds its sockets by address and PID and skips only the state check; an invalid state is shown
  as a translated "Invalid (0)" with a tooltip instead of `? (0)`; documented in
  architecture.md. Part 3 (status bar hint) remains open.

## 0.1.5 (2026-11-07)
- #15: program icon drawn in code (`src/icon.rs`: lens over three rows in the mark colors) used
  for the window and, through `build.rs` + `winresource`, as the exe icon with version
  information. Update check and in-place update taken over from RusTrace (`src/http.rs`,
  `src/update.rs`, `src/app/update_window.rs`): check at start / daily / never, release notes of
  all missed versions, skip a version, status bar button, SHA-256-checked ZIP install with
  restart keeping the filter, pause and window. Release notes now show link text instead of raw
  Markdown links and drop `*italic*` marks. Verified with `MMPORTLENS_UPDATE_AS_VERSION=0.0.9`:
  the window listed v0.1.0 with its notes, and *Download and install* replaced the exe in a test
  folder and started v0.1.0. Remaining in #15: the optional administrator variant.
- #15: administrator mode without a second exe (`src/elevation.rs`, `src/app/admin.rs`):
  File → Restart as administrator / the status bar button restart through UAC with the same
  filters and window; File → Always start as administrator (`run_as_admin`) elevates at start;
  `--elevated` prevents a loop; a cancelled UAC is reported in the status bar; the title says
  "(Administrator)".
- Released **v0.1.5** (`scripts/release.ps1 0.1.5`, written release notes). #15 acceptance: the
  released ZIP (checksum OK) started with `MMPORTLENS_UPDATE_AS_VERSION=0.1.0` showed the v0.1.5
  notes in the update window, and *Download and install* replaced and restarted it; #15 closed.
- #1: column filters under the headers (`src/colfilter.rs`, core from RusTrace): text, `=exact`,
  `/regex/` (a `|` inside belongs to the regex), `a|b`, `!`; numbers `>n`, `<=n`, `a-b`, `a..b`;
  addresses CIDR and `loopback` / `private` / `public`. Invalid regex / prefix = red field with
  the error, not applied. View menu: show / remember / clear; toolbar button with the count;
  kept over an update restart. Verified in the running program (port 443 + public, port range,
  invalid `/33`, remembered `/^(code|claude)/` + `!loopback` after a restart).

## In progress (phase 3)
- #17: multi-selection by `ConnKey` (`src/selection.rs`) – Ctrl / Shift + click, Shift + arrows,
  Ctrl+A – kept over refreshes and re-sorting; keyboard shortcuts F5, Space, Ctrl+F, Esc, arrows /
  PgUp / PgDn / Home / End, Ctrl+C (selected rows with a header), F1 = Help → Keyboard shortcuts
  (`docs/keyboard-shortcuts.md`); the context menu copies the selected rows. Verified in the
  running program: Shift+↓ / Shift+PgDn ranges, Ctrl+click, re-sorting by PID kept 23 selected
  rows, Ctrl+F + text + Esc, Ctrl+A + Ctrl+C = header + 123 rows. Ctrl+E (export) comes with #9,
  Del (close connection) with #6.
- #5: reverse DNS (`src/dns.rs`): columns Remote host / Local host, cache with 10 min TTL and
  5 min for "no name", 8 worker threads, `GetNameInfoW` / `getnameinfo` with `NI_NAMEREQD`;
  Service column and port tooltips from a built-in port table (`src/services.rs`); View → Resolve
  addresses and the DNS toolbar button; "Resolving: n" in the status bar. Verified in the running
  program: names of ~40 public addresses arrived in the background while the table kept
  refreshing (github.com, akamai, amazonaws, 1e100.net, 127.0.0.1 → the computer name), sorting by
  Remote host, the column filter `github` on the names, DNS off shows the addresses again.
- #6: Close connection (`SetTcpEntry` + DELETE_TCB, IPv4 TCP only; Del) and End process
  (`TerminateProcess`) in the row context menu for the whole selection, with a confirmation window
  listing the targets (Enter / Esc) and the result in the status bar (`src/actions.rs`,
  `src/app/actions.rs`). Verified in the running program with a helper Python process (listener +
  loopback connection on port 5599): the listener was left out of Close connection, closing
  without administrator rights reported "access denied" for both connections, End process ended
  the helper and its rows disappeared. Not verified elevated (no UAC confirmation possible in the
  session); the test `close_own_connection` checks that case when run as administrator.
