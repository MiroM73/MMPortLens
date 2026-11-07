# MM PortLens – architecture

How the program works inside. Read the section of a feature before changing it and update it
with the change.

## Source layout

| File | What it does |
|------|--------------|
| `src/main.rs` | Window start-up: icon, `--restore <file>` after an update, `update::cleanup_old`. |
| `build.rs` | Windows: icon (from `src/icon.rs`) and version information as a resource of the exe (`winresource`). |
| `src/icon.rs` | The program icon drawn in code (window icon, `.ico` for `build.rs`). |
| `src/elevation.rs` | Administrator mode: `is_elevated` (token elevation), `relaunch_elevated` (`ShellExecuteW` "runas"), command-line quoting. |
| `src/http.rs` | HTTPS GET over WinHTTP (raw FFI) for the update check – from RusTrace. |
| `src/update.rs` | GitHub releases, version compare, release notes as plain lines, ZIP download + SHA-256 check + in-place install, the state handed to the restarted program – from RusTrace. |
| `src/net/mod.rs` | `Connection`, `ConnKey`, `Protocol`, `TcpState`; `snapshot()` picks the platform backend. |
| `src/net/winrows.rs` | Pure decoding of the iphlpapi `*_OWNER_MODULE` table buffers (tested on every platform). |
| `src/net/windows.rs` | Windows backend: `GetExtendedTcpTable` / `GetExtendedUdpTable` with the owner module classes, `GetOwnerModuleFrom*Entry` with a cache (`ModuleCache`), declared directly with `unsafe extern "system"` (no binding crate). |
| `src/net/procfs.rs` | Linux backend: `/proc/net/{tcp,tcp6,udp,udp6}` + socket inode → PID from `/proc/<pid>/fd`. |
| `src/process.rs` | `ProcessInfo` (name, path, start time, user, command line, company, description, services, icon) and `ProcessCache` keyed by PID + start time (#7). |
| `src/winproc.rs` | Windows process details: image path, start time, user (token), command line (`NtQueryInformationProcess`), version strings, icon (`ExtractIconExW` → RGBA), services per PID (`EnumServicesStatusExW`) (#7). |
| `src/poller.rs` | Background thread: snapshot + process names every interval, sent over a channel. |
| `src/model.rs` | `Tracker`: merges snapshots into rows, marks new / changed / closed rows and expires the marks. |
| `src/colfilter.rs` | Column filter syntax: parsing and matching (`ColumnFilter`, `FilterKind`, `FilterError`) – core from RusTrace. |
| `src/selection.rs` | `Selection` (cursor, anchor, selected `ConnKey`s) and `Nav` (arrow / page targets) – pure, tested (#17). |
| `src/dns.rs` | Reverse DNS (#5): `DnsCache` (TTL, negative results, pending – pure), `Resolver` (cache + worker pool), `system_lookup` (`GetNameInfoW` / `getnameinfo`). |
| `src/services.rs` | Built-in table of well-known port names (#5). |
| `src/actions.rs` | Close connection (`SetTcpEntry` + DELETE_TCB) and End process (`TerminateProcess` / `kill`), with the pure checks `can_close` / `can_kill` and `tcp_row` (#6). |
| `src/columns.rs` | `ColumnId`, cell texts, value-based sorting (`SortSpec`), `ColumnLayout` (visibility + order). |
| `src/config.rs` | `Config` (JSON, every field has a default). |
| `src/i18n.rs` | `t!()` and the language files – the same module as in RusTrace. |
| `src/timefmt.rs` | Clock times for the table. |
| `src/app/` | The UI, split by area like RusTrace: `mod.rs` (state, frame loop), `menu.rs`, `toolbar.rs`, `table.rs`, `statusbar.rs`, `colfilters.rs` (column filter state and field), `admin.rs` (restart as administrator), `update_window.rs` (Help → Check for updates), `input.rs` (keyboard shortcuts, copying the selection), `help.rs` (Help → Keyboard shortcuts), `actions.rs` (context menu items, confirmation window and result of #6), `details.rs` (details pane, process icons and tooltip of #7). |
| `lang/*.lang` | Built-in translations (en, sk, cs, de), embedded with `include_str!`. |

## Reading the connections

`net::Reader::read()` returns every endpoint once (`net::snapshot()` is a one-off reader for the
tests). On Windows four calls are made (TCP/UDP × IPv4/IPv6) with `TCP_TABLE_OWNER_MODULE_ALL` /
`UDP_TABLE_OWNER_MODULE` (#14); the buffer is a `Vec<u64>` (the rows contain 8-byte fields, so they
start at offset 8 after `dwNumEntries` + padding) and grows when the API reports
`ERROR_INSUFFICIENT_BUFFER`. The bytes are decoded by `winrows`, whose tests build buffers in the
exact Windows layout (row sizes 160 / 192 / 160 / 176), and the test
`net::tests::live_snapshot_sees_own_sockets` opens a listener and a connection and checks that the
real backend reports them with the own PID – on the Windows CI it validates the iphlpapi path.

A listener has no remote side (`remote = None`), the same as UDP endpoints.

### Creation time and owner module (#14)
Every row carries `liCreateTimestamp` (a FILETIME, 0 when unknown) → `Connection::created` (ms
since the Unix epoch) and `OwningModuleInfo` (16 opaque `u64`s). `GetOwnerModuleFrom{Tcp,Tcp6,Udp,
Udp6}Entry` (class `TCPIP_OWNER_MODULE_INFO_BASIC`) turns the whole row (copied into an aligned
buffer) into `pModuleName` – the executable, or for a service hosted in svchost the **service
name** – stored in `Connection::module`. Windows answers this without administrator rights for
services too. The call costs a process query, so `ModuleCache` keeps the result per
(`ConnKey`, creation time) for as long as the endpoint exists; only new endpoints are asked (a
refresh of ~170 endpoints then takes ~1 ms). The live tests check that an own new connection was
created within a second of now and that its module is the test executable.

Columns: **Created** (`timefmt::stamp`: `HH:MM:SS.mmm` today, else the date), **Duration**
(`timefmt::duration`, from now; sorting by Duration is the reverse of Created), **Module**;
**First seen** is hidden by default but stays as an option. The **Windows services** column shows
the exact service when the module is one of the services of the process
(`columns::win_services`), otherwise all of them. Linux has none of this (`None`).

### Invalid TCP states (issue #21)
On some Windows 11 25H2 machines `GetExtendedTcpTable` (and `GetTcpTable`, `netstat`,
`Get-NetTCPConnection`, TCPView) report the state **0**, which is not a `MIB_TCP_STATE` (1–12), for
live sockets – even a freshly bound listener of our own process – and valid but wrong states for
others. It is an OS problem, not a decoding one. Consequences in the code:
- `TcpState::from_mib` maps any value outside 1–12 to `Unknown(n)`; `columns::state_text` shows it
  as the translated `state.invalid` ("Invalid (0)") and the table cell has the `state.invalid.tip`
  tooltip.
- Such a listener keeps its remote side `0.0.0.0:0` (the decoder only drops it for `Listen`).
- The live test finds its own sockets by protocol, local address and PID and skips only the state
  assertion when Windows reports an invalid state (`state_checkable`, prints a message); on a clean
  Windows (CI) the full check runs.

Limit of polling: a connection that opens and closes between two refreshes is never seen. ETW
(Microsoft-Windows-Kernel-Network) would catch those – see the issues.

## Rows, marks and refresh

`ConnKey` = protocol + local + remote + PID; the state is not part of it, so a state change keeps
the row and marks it *Changed*. `Tracker::update` takes the snapshot with its time (ms since the
Unix epoch – passed in, so tests need no waiting):

- the first snapshot is the starting point – nothing is marked new,
- a key not seen before → *New*; seen again after it was closed → *New* again,
- a key missing from the snapshot → *Closed*, kept until the mark expires,
- marks last `Config::highlight_ms` (3 s); `expire` clears them and drops closed rows.

The UI repaints when the poll thread delivers a snapshot (it calls `request_repaint`) and at the
next mark expiry (`Tracker::next_expiry`).

## Table

`App::view` is a filtered and sorted copy of the tracker rows, rebuilt only when `view_dirty` is
set (new snapshot, expired mark, filter, sort or column change). Sorting compares values
(`columns::compare`), then the `ConnKey` as a tie-breaker, so rows do not jump between refreshes.

Cells are not clipped (`Column::clip(false)`): labels truncate themselves and a marked row paints
its color into the gaps between the cells, so the row is one solid band (same technique as
RusTrace).

The table id contains the list of visible columns, so a changed column set starts with fresh
default widths.

## Selection and keyboard (#17)

`App::sel` is a `selection::Selection`: the cursor row, the anchor of Shift ranges and the set of
selected rows, all as `ConnKey`. Positions are never stored, so the selection survives refreshes,
re-sorting and filtering; functions that need positions get the keys of the rows shown
(`App::view_keys`, in table order). Clicks follow Windows Explorer (`Selection::click`): plain =
only the row, Ctrl = toggle (moves the anchor), Shift = range from the anchor, Ctrl+Shift = add the
range; an anchor that is filtered out starts the range at the clicked row. A right click on a row
outside the selection selects only that row before the context menu opens. After each snapshot
`Selection::retain` drops keys the tracker no longer has (closed rows that expired).

`App::handle_input` (`app/input.rs`) runs at the start of a frame, before the panels, so a
consumed key (Space, arrows) does not reach a focused button too. Keys go to a focused text field
first (`egui_wants_keyboard_input`) and to an open popup (`Popup::is_any_open`); Ctrl+F focuses the
toolbar filter (fixed id `toolbar_filter`), Esc clears it even from inside the field, otherwise
the selection. Navigation reads the Shift state itself, because `consume_key` ignores an extra
Shift (RusTrace #46); `selection::Nav::target` computes the new position (PgUp / PgDn = 20 rows)
and the table scrolls to it (`TableBuilder::scroll_to_row`). Ctrl+C arrives as `Event::Copy`;
`copy_selection` writes the selected rows in table order, tab-separated in the visible columns,
with a header line when there are several. The shortcut list (`app/help.rs`, F1) uses `kb()` to
localize Ctrl / Shift / Space and matches `docs/keyboard-shortcuts.md`.

## Host names and services (#5)

Columns **Remote host** and **Local host** (hidden by default) show the name of the address from
reverse DNS, **Service** the name of the well-known port (the remote port; for listeners and UDP the
local port) from the built-in table in `services.rs` – no `services` file. The port cells have the
service name as a tooltip.

- `dns::DnsCache` keeps per address the last result, when it came and whether a lookup is pending.
  A name lives 10 minutes (`TTL_MS`), "no name" 5 minutes (`NEGATIVE_TTL_MS`); an expired name is
  still shown while it is asked again. `0.0.0.0` / `::` are never asked. Above 20 000 entries the
  expired ones are dropped.
- `dns::Resolver` adds a queue (`Mutex<VecDeque>` + `Condvar`) and `WORKERS` (8) threads started at
  the first lookup; each finished lookup goes back over a channel and wakes the UI. An address
  without a PTR record can take seconds (measured ~4.5 s per address on the development PC with
  .NET `Dns.GetHostEntry` as well), which is why several run in parallel; the UI never waits.
- `dns::system_lookup` builds a `sockaddr_in` / `sockaddr_in6` by hand (same layout on Windows
  and Linux, only `AF_INET6` differs: 23 / 10) and calls `GetNameInfoW` (after a one-time
  `WSAStartup`) or `getnameinfo` with `NI_NAMEREQD`, so a missing name is an error, not the address
  text.
- `App::rebuild_view` fills `Row::remote_host` / `local_host` of the view copies from the cache
  (only while **View → Resolve addresses** / the *DNS* button is on and a host column is visible;
  `Config::resolve_addresses`, default on). `Resolver::receive` in the frame loop marks the view
  dirty when names arrive. Unresolved addresses show the IP, so the text filter, the column filters
  and sorting work on what is shown; sorting puts names before bare addresses.
- The status bar shows "Resolving: n" while lookups are pending.

## Process details (#7)

`ProcessCache` (in the poll thread) looks up a PID when it first owns an endpoint and keeps the
`Arc<ProcessInfo>` while the PID has endpoints. A PID is identified together with the **start
time** of its process (`GetProcessTimes`; Linux `/proc/<pid>/stat` field 22 + `btime`): a PID
reused by a new process has another start time and is looked up again. Without access (other
users' and protected processes when not elevated) the start time is `None` and the entry stays.

- Windows (`winproc.rs`, all with `PROCESS_QUERY_LIMITED_INFORMATION`): image path
  (`QueryFullProcessImageNameW`), user (`OpenProcessToken` + `TokenUser` + `LookupAccountSidW` →
  `DOMAIN\user`), command line (`NtQueryInformationProcess` class 60, Windows 8.1+; the
  `UNICODE_STRING` must point inside the returned buffer), company and description
  (`GetFileVersionInfoW` + `VerQueryValueW`, the first translation, then 0409/04b0 and 0409/04e4).
  Linux: `/proc/<pid>/cmdline` and the UID from `/proc/<pid>/status` mapped by `/etc/passwd`.
- Icons: `ExtractIconExW` (the large 32 px icon of the exe; no COM needed, unlike
  `SHGetFileInfoW`), `GetIconInfo` + `GetDIBits` as top-down 32-bit BGRA, converted by the pure
  `icon_rgba` (an icon without alpha takes it from the AND mask). Cached per executable path in
  the poll thread; the UI creates one texture per path (`App::load_icon_textures`, before the table
  borrows `self`) and draws it in the Process cell and the details pane.
- Services: `EnumServicesStatusExW(SC_ENUM_PROCESS_INFO, SERVICE_WIN32, SERVICE_ACTIVE)` read every
  10 s (`SERVICES_EVERY`) into PID → (service name, display name); processes whose list changed get
  a new `Arc<ProcessInfo>`. The entries of `ENUM_SERVICE_STATUS_PROCESSW` are read with the
  pointer-size stride (two string pointers + 9 DWORDs, the PID is the 8th). No administrator rights
  needed, so the services of `svchost.exe` are known even when its path is not. The exact service of
  an endpoint comes with #14 (owner module).
- Columns (hidden by default): Windows services, User, Command line, Company, Description. The
  Process cell tooltip shows description – company, services and the command line.
- **View → Details pane** (`Config::show_details`, default on): a resizable bottom panel for the
  cursor row (or the first selected row): the connection fields on the left, the process with its
  icon on the right; values are selectable. Without a path and without elevation it explains that
  Windows needs administrator rights.

## Close connection, End process (#6)

The row context menu has **Close connection** (also Del) and **End process**; both work on all
selected rows shown (`App::selected_rows`, table order).

- `actions::can_close` allows only IPv4 TCP connections with a remote side that are not Listen /
  TimeWait / Closed / DeleteTcb (an invalid state of #21 is allowed – Windows reports it for live
  connections; a listener with an invalid state keeps `0.0.0.0:0` as its remote side and is
  refused). Windows has no IPv6 counterpart of `SetTcpEntry`. The disabled menu item has the
  reason as its tooltip; Del on rows that cannot be closed shows the reason in the status bar.
- `actions::can_kill` refuses PID 0, 4 (Windows) and MM PortLens itself; End process acts on the
  distinct PIDs of the selection.
- `App::pending_action` holds the targets until the confirmation window (`confirm_window`, centered,
  Enter = confirm, Esc = cancel; `handle_input` does nothing while it is open) is answered. It lists
  up to 12 targets (connection: process, PID, local → remote; process: name, PID, path) and, when
  not elevated, a hint that Windows usually refuses this without administrator rights.
- `close_connection` sends `MIB_TCPROW` with state 12 (DELETE_TCB) built by the pure `tcp_row`
  (copied into a `[u32; 5]` for alignment); `kill_process` uses
  `OpenProcess(PROCESS_TERMINATE)` + `TerminateProcess` (Linux: `kill(pid, SIGTERM)`). Access
  denied (`ERROR_ACCESS_DENIED`, and 317 `ERROR_MR_MID_NOT_FOUND`, which `SetTcpEntry` returns
  without elevation) becomes `ActionError::AccessDenied`; other errors carry the system text
  (`FormatMessageW`).
- The calls return at once, so they run on the UI thread; the result ("Done: n" or "Done: n,
  failed: m – reason") stays in the status bar until dismissed (✖) or replaced, and a refresh is
  requested so the closed rows are marked right away.

## Column filters (#1)

A filter row under the column headers has one text field per visible column (**View → Show column
filters**). The syntax is implemented in `colfilter.rs`, whose core (alternatives, `=` exact,
`!` negation, numeric ranges) comes from RusTrace:

- every column: `text` (contains), `=text`, `/regex/`, `a|b`, `!…`; case-insensitive. A `|` inside
  `/…/` belongs to the regex (`split_alternatives` tracks an open regex: it starts with `/` at the
  beginning of an alternative and ends with a `/` followed by `|` or the end);
- `FilterKind::Number` (PID, ports – `ColumnId::filter_kind`): `>n`, `>=n`, `<n`, `<=n`, `a-b`,
  `a..b`; a value that is not a number never matches them;
- `FilterKind::Address` (local / remote address): CIDR (`10.0.0.0/8`, `fe80::/10`, prefix compared
  as bits; IPv4-mapped IPv6 is treated as IPv4) and `loopback`, `private` (RFC 1918, CGNAT
  100.64/10, link-local, ULA fc00::/7, fe80::/10), `public` (none of these, nor unspecified,
  multicast or broadcast). An empty address (listener, UDP) never matches them.

`ColumnFilter::parse` returns `Err(FilterError)` for an invalid regex or prefix length; the field
turns red with the error as a tooltip and the filter is not applied. Valid filters get a colored
field. `App::apply_column_filters` recompiles them after every change (`col_filter_text` →
`col_filters`, `col_filter_errors`); `row_shown` ANDs the filters of the **visible** columns with
the protocol switches and the toolbar text filter, by matching the displayed cell text. Matching
5 000 addresses takes well under a millisecond in a release build (test
`five_thousand_rows_are_fast` checks < 50 ms even in debug).

The toolbar shows "✖ Column filters: n" while any field has text (also when the row is hidden);
it clears all of them, like **View → Clear column filters**. With **View → Remember column
filters** the texts are saved in `Config::column_filters` and set again at start; they are also
handed to the new version after an update (`SessionState::col_filters`).

## Settings and languages

`Config` is saved debounced (0.8 s after the last change, and on close) through a temporary file.
`ColumnLayout::normalize` repairs layouts from older versions: duplicates removed, new columns
added hidden at the end, at least one column visible.

Every user-visible string goes through `t!("key")`; `i18n::tests::builtin_languages_are_complete`
checks that all four built-in languages have the same keys and placeholders.

## Icon and version resource (#15)

`icon::sample(x, y)` gives the color of a point of the icon (coordinates 0..1): a teal rounded
square, a white magnifying lens and inside it three rows in the colors of the row marks (new
green, changed yellow, closed red). `icon::rgba(size)` renders it with 4 × 4 anti-aliasing and is
used for the window icon (64 px); `icon::ico(sizes)` writes a 32-bit `.ico` (16 – 256 px).
`build.rs` includes `src/icon.rs` with `#[path]`, writes `mmportlens.ico` into `OUT_DIR` and
compiles it with `FileDescription` / `ProductName` (the version comes from Cargo) through
`winresource` – this needs `rc.exe` from the Windows SDK (present on the GitHub runners and with
Visual Studio). On other platforms `build.rs` does nothing.

## Administrator mode (#15)

One executable (no `requireAdministrator` manifest, no second exe); it can start itself again
elevated, like *Show details for all processes* in Process Explorer.

- `elevation::is_elevated` reads `TokenElevation` of the own process token; `App::elevated` keeps
  the answer, the window title gets "(Administrator)".
- **File → Restart as administrator** and the "Not administrator" button in the status bar call
  `App::restart_elevated`: the `SessionState` of the update restart (filter, column filters,
  pause, window) is written to a temporary file and the exe is started with
  `ShellExecuteW("runas", exe, "--restore <file> --elevated")`. Confirmed UAC → settings saved,
  poller stopped, window closed. Cancelled UAC (`ERROR_CANCELLED`) or another error → the file is
  deleted, the program keeps running and the status bar shows why.
- **File → Always start as administrator** (`Config::run_as_admin`): `main` checks it before the
  window is created; a program that is not elevated and was not started with `--elevated`
  starts itself elevated with its own arguments plus `--elevated` and ends. `--elevated` stops a
  loop when elevation does not produce an elevated token. A failure is shown in the status bar.
  The check runs before `restore_arg`, which deletes the restore file the child still needs.
- `join_args` quotes the arguments by the `CommandLineToArgvW` rules (unit-tested).
- Elevating to a *different* account (over-the-shoulder UAC) uses that account's `%APPDATA%`
  settings and `%TEMP%`; the restore file path is absolute, so it is still found.
- An update installed by an elevated program restarts it elevated (a child of an elevated
  process); a non-elevated one elevates itself again when the setting is on.

## Updates (#15)

Taken over from RusTrace (`update.rs`, `http.rs`, the window in `app/update_window.rs`) with the
names changed; the logic is the same.

- **Check:** `update::fetch_updates` (background thread) reads
  `api.github.com/repos/MiroM73/MMPortLens/releases?per_page=100` over WinHTTP (system proxy and
  certificates, no TLS crate), drops drafts, pre-releases and tags that are no version, and returns
  the latest release plus the *missed* ones (newer than this program). `Config::update_when` =
  `start` (default) / `daily` / `never`; the time of the last check is `update_last_check`. A
  check at start-up is *silent*: it opens the window only for a newer version that the user did
  not skip (`update_skipped`) and reports no errors (offline). A newer version also shows a
  green "⬆ <version>" button in the status bar.
- **Window:** release notes of every missed version (Markdown shown as plain lines by
  `plain_notes`: headings, bullets, link text without the URL, no bold / italic / code marks),
  *Download and install*, *Release page*, *Skip this version*, *Later*, *Check again* and the
  check setting.
- **Install:** the release must have `MMPortLens-v<version>-windows-x64.zip` and its `.zip.sha256`
  (made by `scripts/package.ps1`). The ZIP is downloaded with progress (cancellable, max 200 MB),
  its SHA-256 compared, unpacked by the built-in reader (`unzip`: stored / deflate, CRC checked, no
  absolute paths or `..`) and installed: write access is probed first, the running exe is renamed
  to `mmportlens.exe.old` (Windows allows renaming a running exe), the new one written under the
  original name (renamed back on failure), the other files (`lang/*.lang`, `LICENSE`) written next
  to it; own files in the folder stay. The next start deletes `.old` (`cleanup_old`).
- **Restart:** the settings are saved, `SessionState` (toolbar filter, paused, window position /
  size / maximized) is written to `%TEMP%\mmportlens-restore-<pid>.json` and the new exe is started
  with `--restore <file>`; it reads and deletes the file, opens the window at the same place and
  takes the filter over.
- **Testing:** `MMPORTLENS_UPDATE_AS_VERSION=0.0.9` makes the program pretend an older version, so
  the window and the installation can be tried with the published release;
  `cargo test fetch_latest_release -- --ignored` downloads and installs the latest release into a
  temporary folder.

## Tests

`cargo test` – decoding of the Windows owner module tables (incl. creation times, row offsets),
the live creation time and owner module of an own connection, and `/proc/net` lines, state codes, the tracker
(new / changed / closed / expiry / reappearing), column layout and sorting, settings round trip,
language files, reverse DNS (cache TTL / negative results / pending, `sockaddr` layout, a
300-address run through the worker pool with a slow fake resolver – never more than `WORKERS` at
once, the caller never blocks –, the system resolver on 127.0.0.1), port names, process details (own process: start time, user, command line; notepad.exe: company,
description, icon; services by PID; start time identifies a reused PID; icon alpha / mask
conversion, FILETIME, version keys), close / kill checks and the `MIB_TCPROW` layout, ending an own child process,
closing an own loopback connection (elevated: it disappears; not elevated: exactly
`AccessDenied`), host / service
cells, selection (clicks, ranges, re-sorting, navigation targets), column filters (syntax, regex, numeric ranges, CIDR, address classes, errors,
speed), the icon / `.ico` layout, update logic (versions, GitHub JSON, missed releases,
release notes, SHA-256 files, ZIP reading, installation into a temporary folder, session state)
and the live snapshot test.

## Release

Taken over from RusTrace, same procedure and file names (with `MMPortLens` in place of `RusTrace`).

- `scripts/release.ps1 <version> [-Publish] [-SkipTests] [-DryRun] [-Yes] [-NoWatch]` – checks
  (tools, `gh` login, branch `master`, clean tree, not behind `origin`, tag free), writes the version
  into `Cargo.toml`, runs `cargo test` (backs up and restores `%APPDATA%\MMPortLens\config.json`),
  commits `Release v<version>`, pushes `master`, creates and pushes the annotated tag `v<version>`,
  waits for the workflow and with `-Publish` publishes the draft as the latest release.
  `-SkipTests` skips the local tests when they cannot pass for a reason outside the code (#21) and
  runs `cargo update --workspace` instead, so `Cargo.lock` still gets the new version.
- `.github/workflows/release.yml` – on a `v*` tag: `cargo test --locked`, `cargo build --release
  --locked`, `scripts/package.ps1 -Version <tag>`, then a **draft** release titled
  `MM PortLens v<version>` with generated notes and the four files from `dist/`. A manual run only
  uploads `dist/*` as an artifact.
- `scripts/package.ps1` – stages `MMPortLens\{mmportlens.exe, lang\*.lang, LICENSE}` and produces
  `dist\MMPortLens-v<version>-windows-x64.exe` (7-Zip `7z.sfx` module + 7z archive),
  `…-windows-x64.zip` and a `.sha256` file for each (`<hash>  <file name>` + LF, readable by
  `sha256sum -c`). Needs 7-Zip with `7z.sfx` (preinstalled on the GitHub Windows runners).
- The release notes follow RusTrace: *What's new*, *What's changed*, *What's fixed*, *Removed*,
  each item with its issue number, and a *Full Changelog* link.
- The CRT is linked statically (`.cargo/config.toml`), so the exe needs no Visual C++ runtime.
