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

## 0.2.0 (2026-11-07) – phase 3, parity with TCPView
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
- #7: process details (`src/winproc.rs`, `src/process.rs`, `src/app/details.rs`): icon in the
  Process column, tooltip, details pane (View → Details pane), columns Windows services / User /
  Command line / Company / Description; services per PID from the service manager every 10 s;
  the process cache is keyed by PID + start time (PID reuse). Verified in the running program
  (not elevated): msedge.exe rows with the Edge icon, "Microsoft Edge", Microsoft Corporation,
  user, start time and the full command line in the details pane; `svchost.exe` rows without a
  path still show their services (CDPSvc, KeyIso, RpcEptMapper, Schedule …) and the hint about
  administrator rights. Linux build checked with `cargo clippy --target x86_64-unknown-linux-gnu`.
- #14: the Windows backend reads `TCP_TABLE_OWNER_MODULE_ALL` / `UDP_TABLE_OWNER_MODULE`: real
  creation time of every endpoint and its owner module (`GetOwnerModuleFrom*Entry`, cached per
  endpoint). Columns Created, Duration, Module (First seen now hidden by default); the Windows
  services column shows the exact service of the endpoint. Verified: the live tests (creation time
  of an own new connection within 1 s, module = the test exe), and in the running program without
  administrator rights the svchost endpoints showed their exact services (SessionEnv, CDPSvc, RpcSs,
  Schedule, EventLog, TermService, Spooler, WinDefend …), creation times with ms, durations, and a
  refresh took ~1 ms.
- #8: traffic per TCP connection (`src/net/estats.rs`): columns Sent, Received, Sent/s,
  Received/s, Sent packets, Received packets (hidden by default); ESTATS collection switched on per
  connection and read every refresh only while such a column is visible; rates from the difference
  between snapshots; without administrator rights the status bar says so. Verified: unit tests
  (ESTATS row layouts, counter decoding, rates incl. a counter reset) and, not elevated, the
  access error path in `counts_own_traffic` and in the running program (empty columns, "Traffic:
  administrator only"). The elevated path (counts growing during a download) could not be checked
  in this session – run `cargo test counts_own_traffic` as administrator to check it.
- Released **v0.2.0** (`scripts/release.ps1 0.2.0`, written release notes). The local tests passed
  (86); the script pushed and tagged, then stopped while waiting for the workflow under Windows
  PowerShell 5.1 (#25) – the workflow was watched by hand (tests on the runner OK) and the draft
  published. Check of the released ZIP (checksum OK) started with
  `MMPORTLENS_UPDATE_AS_VERSION=0.1.5`: the window title says 0.2.0 and the update window shows the
  v0.2.0 notes.
- #27: horizontal scroll bar under the table when the columns are wider than the window (ported
  from RusTrace `7904f90`): the table is drawn shifted and clipped in a child `Ui`, so its vertical
  scroll bar stays at the right edge; Shift + mouse wheel / horizontal touchpad scrolling; new
  `MMPORTLENS_CONFIG` variable for test instances with their own settings. Verified in the running
  program (800 px wide window with all default columns): the bar appears, a horizontal wheel
  moves the columns, dragging the bar to the end shows the last column (Command line) complete
  with the vertical bar still visible, and at 1900 px the bar disappears.
- #2: column order by dragging a header (drop line, title at the pointer, scrolling at the edges,
  Esc cancels) and by dragging the ≡ handle in the column window; column widths remembered in the
  settings (`ColumnSetting::width`), double click on a header border fits the width to the content,
  Default columns also resets the widths. Verified with unit tests (`move_to`, `drop_slot`, widths
  round trip and repair of old / broken values) and in the running program with its own settings
  file: PID dragged behind State, Protocol resized to 149 px and Local address fitted to 180 px,
  both kept after a restart; Local port dragged in the column window to the place the line showed;
  Default columns restored the order and widths (no width left in the file).
- #9: export of the table (File → Export…, Ctrl+S) as CSV, Excel, JSON Lines or HTML
  (`src/tabexport/` with the writers from RusTrace, `src/app/export.rs`): rows shown / selected /
  all, visible / all columns, typed cells, export thread with progress and cancel, last file with
  open / show in folder / copy path; Ctrl+C quotes values like CSV. New dependencies `rfd` (save
  dialog) and `open`, as in RusTrace. Verified: 14 new unit tests (typed cells, column sets, every
  writer, cancel removes the file); 5 000 rows with all 27 columns opened in Excel through COM
  (numbers as numbers, dates with the date format, duration `0.000`, formula-like text stays text,
  frozen header + AutoFilter, row colors); in the running program File → Export… → Excel → Save
  dialog wrote 197 rows × 11 columns, read back in Excel, and the window showed the last export.
