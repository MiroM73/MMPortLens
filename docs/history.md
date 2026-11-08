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

## 0.2.5 (2026-11-08) – phase 2, the table
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
- #20 (shared crate with RusTrace), state after porting the filters (#1) and the export (#9) –
  lines that differ between the copies in the two repositories: `tabexport/zip.rs` 2 (comment),
  `tabexport/csv.rs` 38 and `html.rs` 39 (tests only), `http.rs` 34, `i18n.rs` 43 (app name),
  `jsonl.rs` 50 (date source), `update.rs` 225 (app / asset names, environment variable),
  `tabexport/xlsx.rs` 253 (columns, summary sheets), `colfilter.rs` 846 (numbers / addresses /
  CIDR here, dates there). A shared crate would need an `mm-*` repository used as a git
  dependency by both programs and changes in RusTrace, so the decision is left to the owner.
- Found on the way: the status bar overlaps in a narrow window (new #29); the ⏴ ⏵ buttons of the
  column window are empty boxes like the sort arrow (comment on #22); the live snapshot test failed
  again with the own client connection reported as FinWait1 (comment on #23).
- #20 deferred indefinitely by the owner (2026-11-08): the modules shared with RusTrace stay
  copied; phase 2 is done.
- Released **v0.2.5** (`scripts/release.ps1 0.2.5 -NoWatch` run under PowerShell 7, written release
  notes). The local tests passed (103); the workflow was watched with `gh run watch` (OK) and the
  draft published with the notes. Check of the released ZIP: checksum OK, `mmportlens.exe` has
  product version 0.2.5, and the update code (`cargo test fetch_latest_release -- --ignored`) sees
  v0.2.5 as the latest release. The update window of an older version was not opened by hand.

## 0.2.9 (2026-11-08) – phase 4, comfort
- #28: process properties window (`src/app/properties.rs`) from **Properties…** in the row context
  menu or Alt+Enter: icon, name, description, path, company, user, start time, command line,
  services, PID and the number of TCP / UDP endpoints, not tied to the selection; Copy, Open file
  location, File properties (`ShellExecuteExW` "properties"), End process (confirmation of #6),
  Close, Esc. When the process exits the last values stay with "(process has exited)". Verified
  with a unit test of the exit decision and in the running program on a `python -m http.server`
  listener: the window showed the same values as the details pane, End process from the window
  ended it after the confirmation and the window switched to the exited state (0 TCP, End process
  disabled), File properties opened the Windows dialog of python.exe, Esc closed the window.
- #10: custom color rules (`src/rules.rs`, window `src/app/rules.rs`, View → Color rules… /
  Ctrl+H): per rule a column (or any visible column) and a pattern in the column filter syntax,
  text / background colors per theme, order decides, marks over rules configurable, example rules
  (RDP, PowerShell, VPN range), import without duplicates / export of chosen rules, Excel / HTML
  export with the rule colors. Verified: 8 unit tests (order, any column, disabled / invalid
  rules, marks vs rules, export colors, examples valid, import + old files, moving) and the
  settings test; in the release build with a rule "Local port 8765" the `python -m http.server`
  row was blue with white text, the window showed the rule, changing the pattern to 9999 removed
  the color at once and was saved to the settings file. The ⚠ of an invalid pattern was not
  checked by hand (unit tests only).
- #13: saved filter presets (`src/presets.rs`, `src/app/presets.rs`): toolbar combo box with the
  presets (switches, text filter, column filters at once; `*` when changed afterwards), Save current
  as…, Manage presets… (rename, order, replace, delete, restore examples), example presets,
  `--preset <name>` on the command line. Verified: 4 unit tests (comparison, defaults / find /
  save, command line, JSON) and the settings test; in the release build: `--preset "Listening
  ports"` started with `State =Listen`, typing `msedge` showed "Listening ports *", Save current as
  "Edge" stored it (and made it the chosen one), after a restart `--preset edge` set both filters
  again, Manage presets listed all six with their filters. Found on the way: the ★ character is an
  empty box in egui's fonts (like #22), ⭐ is used instead.
- #12: group rows by process (`src/grouping.rs`, `src/app/groups.rs`): toolbar switch *Group*
  and View → Group by process / Expand all / Collapse all; one line per process (icon, name, PID,
  TCP / UDP count, traffic sum), click collapses / expands, collapsed groups remembered by
  process name; groups follow the sort column, filters apply inside the groups. Verified: 4 unit
  tests (order of groups and rows, collapsed groups, groups per PID, 5 000 rows / 300 processes)
  and the settings test; in the release build: group lines over the whole table width with the
  icon, a click collapsed `<11160>` and it stayed collapsed after a restart, sorting by PID
  (descending) ordered the groups by PID, a click and ↓ moved the selection inside a group with
  the details pane following. Found and fixed on the way: a label in the group line widened the
  first column (and would have saved that width), a half-transparent tint left stripes between
  the cells.
- Released **v0.2.9** (`scripts/release.ps1 0.2.9 -NoWatch -Yes` run under PowerShell 7, written
  release notes). The local tests passed (120); the workflow was watched with `gh run watch` (OK)
  and the draft published with the notes as the latest release. Check of the released ZIP:
  checksum OK, `mmportlens.exe` has product version 0.2.9, and the update code (`cargo test
  fetch_latest_release -- --ignored`) passes with v0.2.9 as GitHub's latest release. The update
  window of an older version was not opened by hand. Comments added to #16 (`--preset` of #13 is
  the first piece of the command line) and #23 (the live snapshot test failed once more).

## 0.3.7 (2026-11-08) – live traffic graph (phase 4b) and small fixes (any time)
- #30: live traffic and I/O graph in the process properties window (`src/rates.rs`,
  `src/app/properties_graph.rs`, new dependency `egui_plot` 0.38 built on egui 0.37): TCP segments
  per second of the process from the ESTATS counters of #8 (collected while the window is open;
  administrator only) and I/O operations per second from `GetProcessIoCounters` / `/proc/<pid>/io`,
  last two minutes, sampled once per snapshot with per-connection positive deltas. Verified: 8
  sampler unit tests, `/proc/<pid>/io` parsing, own I/O counters readable; in the release build
  (not elevated) on a PowerShell downloading in a loop: the I/O plot moved (~40–60 "other"
  operations/s), the TCP part showed the administrator note, a system process without access showed
  both notes, ending the PowerShell froze the graph with "(process has exited)". The TCP plot as
  administrator was not checked by hand (no elevated session). Also fixed a misplaced doc comment
  of `process::start_time`.
- #22: the sort arrow in the header is a painted triangle (`table::paint_triangle`) instead of
  ⏶ / ⏷, which egui's fonts do not contain; the ⏴ / ⏵ move buttons of the column window (same
  problem, comment on #22) are buttons with a painted triangle, and the group line triangle of #12
  uses the same function. Verified in the release build: ▲ after Process (ascending), ▼ after PID
  (descending) with grouping on, the column window shows ◀ / ▶ with the disabled first / last
  button faded; fmt, clippy, 129 tests.
- #24: a TCP row with the remote side `0.0.0.0:0` / `[::]:0` has no remote side whatever its
  state (`winrows::remote_or_none` decides by the address alone), so a listener that Windows
  reports alternately with state 0 and Listen keeps one key and is only marked changed instead of
  closed + new. Verified: new decoder test (IPv4 and IPv6 rows with state 0 and Listen have the
  same key), `can_close` test for both forms, the live test finds its listener by
  `remote == None`; in the release build the listeners with "Invalid (0)" and the alternating
  `:5040` show no remote side and the last refresh stayed at +0 −0 ~0. 130 tests.
- #23: the live snapshot test retries the snapshot (5 × 50 ms) before checking the states, its
  failure messages list the observed rows of the own PID, and a valid but wrong state of the own
  sockets is skipped with a message when the same snapshot contains invalid states (#21),
  decided by the pure `check_state`. Verified: 100 consecutive runs of the test passed on the
  development PC; every run skipped the listener (state 0) and 15 skipped the client (reported as
  Listen, FinWait1 or Closed despite the retries); unit test of `check_state`; 130 tests.
- #21 part 3 (closes #21): the status bar shows "Invalid TCP states: N" in the warning color with
  a tooltip explaining that Windows on this computer reports broken states (also in netstat and
  TCPView) while any TCP row has an invalid state; new keys in all four languages. Verified in
  the release build on the affected PC: "Invalid TCP states: 39" after the state counts; 130
  tests (including the language completeness test).
- #29: the status bar reserves the width of the refresh time at the right and lets the other items
  wrap onto more lines (one-line-high wrapping layout, labels not wrapped inside) instead of
  painting the refresh time over them. Verified in the release build at 1500, 800 and 600 px: one
  line as before when wide, two / three lines without overlap when narrow; 130 tests.
- #25: `release.ps1` finds the workflow run of the tag by filtering the JSON of `gh run list` in
  PowerShell instead of a `--jq` expression with a quoted tag, which Windows PowerShell 5.1 passed
  without the quotes. Verified: the new filter returns the run of v0.2.9 and nothing for a missing
  tag under both `powershell.exe` 5.1 and `pwsh` 7.6; the release of v0.3.7 runs the watch step
  under 5.1. The same bug in RusTrace is MiroM73/Rustrace#59.
- #26: the update check retries transient GitHub failures (HTTP 502 / 503 / 504, WinHTTP timeouts
  and broken connections; 2, 5, 10 s, `Retry-After` up to 30 s) and explains failures in the
  user's language with the raw code kept ("GitHub is temporarily unavailable (HTTP 503). Try again
  later.", rate limit with the reset time, network); a 5xx links to githubstatus.com, and a
  transient failure is not counted as the daily check. `http::HttpError` with pure `kind` /
  `retry_delay` / `retry`, `update::CheckError`, test override `MMPORTLENS_UPDATE_URL`. Verified:
  6 new unit tests; the ignored network tests (`real_http_errors` against httpbin.org and GitHub:
  404 / 503 kinds, three retries, rate-limit headers read from a GitHub 404, unresolvable name =
  network; `fetch_latest_release`) pass; in the release build with the URL set to httpbin's 503
  the Updates window kept the spinner during the retries and then showed the message with the
  status page link; a 503 at start left `update_last_check` unchanged, a 404 updated it. 135
  tests. Port to RusTrace: MiroM73/Rustrace#60.
- Released **v0.3.7** (`scripts/release.ps1 0.3.7 -Yes` run under Windows PowerShell 5.1, which
  ran through the watch step – the acceptance of #25; written release notes). The local tests
  passed (135), the workflow succeeded and the draft was published with the notes as the latest
  release. Check of the released ZIP: checksum OK, `mmportlens.exe` has product version 0.3.7, and
  `cargo test fetch_latest_release -- --ignored` passes with v0.3.7 as GitHub's latest release.
- #3: recording of connection events and their replay. `Tracker::update_with_events` produces the
  events of every refresh (new, closed, state changed) and `present_events` the starting state;
  `src/recording/` holds the SQLite format (`rusqlite` bundled; `meta`, `processes`, `events` with
  `conn_id`, indexes on `time_ms` and `(conn_id, time_ms, kind)`, view `events_readable`, schema
  version 1, WAL + one transaction per refresh, finished into a single file), the writer thread
  and the replay worker. UI: File → Start recording… / red toolbar button (duration, kinds, frozen
  filter as `RowFilter`), ● REC with time and events in the status bar, automatic stop, result
  with *Open*; File → Open recording… or `mmportlens.exe file.plrec` shows the state at a slider
  time in the normal table (filters, sorting, export work; close / end / properties disabled),
  step buttons, *Back to live*, and a paged event list (text and kind filter, time order, click =
  jump). Verified: 13 new unit tests (event generation, write → read round trip, replay state and
  marks, event pages and filter escaping, schema version / format check, single file after
  finish, writer and replay threads); the ignored `busy_hour` test: 110 000 events in 11.1 MB,
  state at the end of the hour in 32 ms. In the release build: recorded 14 s with local test
  connections (start window, save dialog, ● REC, stop from the toolbar and from the status bar,
  one file left), opened it with *Open* and from the command line, filtered the event list to the
  test port and jumped to an event, back to live. Found on the way: a modal save dialog let
  queued snapshots be recorded before the starting state (fixed: the start state takes the time of
  the last snapshot); #31 (Duration in the replay) and #32 (invalid-state flips flood a
  recording). Headless recording from the command line stays with #16. 148 tests.
- #16: command line. `src/cli.rs` (the parser style of RusTrace: help in the UI language,
  message box with a window, console without one) with `--preset` (kept from #13), `--filter`,
  `--column <col>=<filter>` (repeatable; column key or title), `--interval`, `--paused`, `--lang`
  (run-only, the saved values are written back), a `.plrec` RECORDING, `--help`, `--version`, and
  without a window `--export <file>` (one snapshot, `src/headless.rs`) and `--record <file>
  --duration <time>` (Ctrl+C / Ctrl+Break / closing the console finish the file properly); exit
  codes 0 / 1 / 2. `RowFilter` moved to `src/rowfilter.rs` so the command line shares it with the
  table and the recording; `FilterError::text`, `FilterPreset::description`,
  `tabexport::computer_name`, `poller::snapshot` shared too; the old `preset_arg` / `file_arg`
  scanners removed (unknown arguments are now errors). Verified: 11 new unit tests (the 2 of the removed scanners dropped; 157 in total),
  clippy, and in the release build from PowerShell: `--export x.csv | Out-Null` writes the file
  and `$LASTEXITCODE` is 0; filters and `--lang sk` messages; unknown preset / column, `.pdf` →
  exit 2; unwritable path → exit 1; `--record --duration 6s --interval 500` (282 events, meta with
  interval and duration), `--column state==Established` (`filter` meta), Ctrl+Break → finished
  file and exit 0; with a window: `--column`, `--filter`, `--interval 750` / `1500` in the
  toolbar, `--paused`, `--lang de` / `sk`, unknown preset in the status bar, the scratch settings
  kept language and interval, invalid line and `--help` as message boxes.
- #4: short-lived connections with ETW. `src/net/etw.rs` (pure: decoding of the
  Microsoft-Windows-Kernel-Network events from the provider manifest, `Collector` with a cutoff at
  the table read) and `src/net/etw_win.rs` (real-time session over `StartTraceW` / `EnableTraceEx2`
  / `OpenTraceW` / `ProcessTrace`, no crate; orphaned sessions of dead MM PortLens processes are
  stopped, `PollControl::shutdown` stops the own one on exit). The poll thread runs the session and
  hands the aggregated endpoints to `Tracker::merge_etw`, which adds the unknown ones as closed rows
  marked ⚡ (also the process of connections the table only shows as TimeWait with PID 0) and new +
  closed events with their ETW times. Recording schema version 2 (`events.source`, `meta.etw`;
  version 1 files still open), View → Capture short connections (ETW, admin), status bar item,
  `--record … --etw`. Found on the way: a standard user can create a trace session, only enabling
  the kernel provider is refused. Verified: 9 new tests (decode, FILETIME, collector, merge,
  recording round trip, version 1 file, session names, start refused / runs and stops) – 166 in
  total, clippy; without administrator: the session is refused and none is left behind
  (`logman query -ets`), the status bar shows "ETW: administrator only", the menu item, `--record
  --duration 2s --etw` warns on stderr and writes `etw = off`, `--etw` alone → "needs --record".
  As administrator (owner's elevated PowerShell): the acceptance test `etw_sees_short_connections`
  saw all 100 connections (each < 50 ms) on both sides – 100 client and 100 server rows from ETW;
  `--record --duration 8s --etw` with 20 connections of 100 ms to example.com:80 wrote schema 2,
  `etw = on` and 38 ETW events (19 connections of `pwsh.exe` as new + closed ~20 ms apart, the
  20th was caught by a refresh); `logman query -ets` empty afterwards. The first elevated run
  found that two sessions of one process (the two tests in parallel) shared the name, so one start
  stopped the other (`EnableTraceEx2` error 4201): the names are now `MMPortLens ETW <pid>-<n>`.
  Also found: #40 (console output garbled in an OEM code page console).
- #4 follow-up from the owner's check as administrator: (1) a received UDP datagram showed the
  sender as the local side. A first fix swapped the sides of every receive event; the owner's
  elevated run of the extended acceptance test showed the TCP receives on the client rows and the
  GUI showed TCP rows turned around, so only **UDP** receives are swapped (`saddr` = sender there,
  local side everywhere else). The acceptance test now checks both live: TCP received bytes 0 on
  the client and 100 on the server rows, UDP receiver row 5 bytes in with the sender as remote.
  Final elevated run: the acceptance test passes; the GUI with `=ETW` in the Source column shows
  correctly oriented rows (msedge DNS over TCP to the router, msedgewebview2 UDP DNS queries).
  (2) The owner asked how to show only the ETW rows: new hidden column **Source** (`ETW` / empty),
  so `=ETW` in its column filter, presets, color rules, export and `--column source=ETW` work.
  Verified: fmt, clippy, 166 tests (decode of receives, Source cell, visible column count),
  `--column source=ETW --export x.csv` in the release build (header with Source, 0 rows in a
  single snapshot as expected).
- Released **v1.0.0** (`scripts/release.ps1 1.0.0 -Yes`; written release notes: recording and
  replay #3, command line #16, ETW capture of short connections #4 with the Source column; known
  issues #31, #32, #37, #40, #21). The README status line names 1.0.0 instead of "early version".
  The local tests passed, the workflow succeeded and the draft was published with the notes as the
  latest release. Check of the released ZIP: checksum OK, `mmportlens.exe` has product version
  1.0.0, and `cargo test fetch_latest_release -- --ignored` passes with v1.0.0 as GitHub's latest
  release.
