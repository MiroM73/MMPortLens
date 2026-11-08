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
| `src/net/estats.rs` | Traffic counters per TCP connection (ESTATS, #8): `Collector`, the pure `tcp_row` / `tcp6_row` / `decode_data`. |
| `src/net/procfs.rs` | Linux backend: `/proc/net/{tcp,tcp6,udp,udp6}` + socket inode → PID from `/proc/<pid>/fd`. |
| `src/process.rs` | `ProcessInfo` (name, path, start time, user, command line, company, description, services, icon) and `ProcessCache` keyed by PID + start time (#7). |
| `src/winproc.rs` | Windows process details: image path, start time, user (token), command line (`NtQueryInformationProcess`), version strings, icon (`ExtractIconExW` → RGBA), services per PID (`EnumServicesStatusExW`) (#7), I/O counters (`GetProcessIoCounters`, #30). |
| `src/rates.rs` | `Sampler`: per-second TCP and I/O rates of one process from cumulative counters, ring buffer of the last two minutes – pure, tested (#30). |
| `src/poller.rs` | Background thread: snapshot + process names every interval, sent over a channel. |
| `src/model.rs` | `Tracker`: merges snapshots into rows, marks new / changed / closed rows and expires the marks; `update_with_events` / `present_events` for a recording (#3). |
| `src/recording/` | Recording (#3): `mod.rs` (`Event`, `EventKind`, `EventKinds`, start-window `Settings`, `parse_duration`, `file_arg`), `db.rs` (all SQL: `RecordingWriter`, `RecordingReader`, `EventQuery`; tests in `db_tests.rs`), `threads.rs` (writer thread `Recorder`, `ReplayWorker`). |
| `src/colfilter.rs` | Column filter syntax: parsing and matching (`ColumnFilter`, `FilterKind`, `FilterError`) – core from RusTrace. |
| `src/selection.rs` | `Selection` (cursor, anchor, selected `ConnKey`s) and `Nav` (arrow / page targets) – pure, tested (#17). |
| `src/dns.rs` | Reverse DNS (#5): `DnsCache` (TTL, negative results, pending – pure), `Resolver` (cache + worker pool), `system_lookup` (`GetNameInfoW` / `getnameinfo`). |
| `src/services.rs` | Built-in table of well-known port names (#5). |
| `src/actions.rs` | Close connection (`SetTcpEntry` + DELETE_TCB) and End process (`TerminateProcess` / `kill`), with the pure checks `can_close` / `can_kill` and `tcp_row` (#6). |
| `src/columns.rs` | `ColumnId`, cell texts, value-based sorting (`SortSpec`), `ColumnLayout` (visibility, order, widths), `drop_slot`. |
| `src/tabexport/` | Table export (#9): `mod.rs` (scope, column set, typed `Cell` of a row, `write_table`, `run`), writers `csv.rs`, `xlsx.rs` + `zip.rs`, `jsonl.rs`, `html.rs` – the writers from RusTrace. |
| `src/config.rs` | `Config` (JSON, every field has a default). |
| `src/i18n.rs` | `t!()` and the language files – the same module as in RusTrace. |
| `src/timefmt.rs` | Clock times for the table. |
| `src/app/` | The UI, split by area like RusTrace: `mod.rs` (state, frame loop), `menu.rs`, `toolbar.rs`, `table.rs`, `statusbar.rs`, `colfilters.rs` (column filter state and field), `admin.rs` (restart as administrator), `update_window.rs` (Help → Check for updates), `input.rs` (keyboard shortcuts, copying the selection), `help.rs` (Help → Keyboard shortcuts), `actions.rs` (context menu items, confirmation window and result of #6), `details.rs` (details pane, process icons and tooltip of #7), `properties.rs` (process properties window of #28), `properties_graph.rs` (its live traffic / I/O graph, #30), `rules.rs` (color rules window and row colors of #10), `presets.rs` (filter preset combo box and windows of #13), `groups.rs` (group lines of #12), `columnlayout.rs` (header drag and drop, remembered widths, fit on a border double click – #2), `export.rs` (File → Export… window, export thread, status – #9), `record.rs` (start window, events to the writer, ● REC – #3), `replay.rs` (opened recording, replay bar with the time slider – #3), `replay_events.rs` (event list of a recording – #3). |
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
- The status bar counts the TCP rows with an invalid state (`Counts::invalid`) and shows
  "Invalid TCP states: N" in the warning color with an explaining tooltip (`status.invalid_states`)
  while there are any, so the user knows the states of this computer are not trustworthy.
- The decoder drops the remote side `0.0.0.0:0` / `[::]:0` by the address alone, whatever the
  state (`winrows::remote_or_none`): Windows reports some listeners alternately with state 0 and
  `Listen`, and deciding by the state gave the same socket two `ConnKey`s, so the row flickered
  as closed + new, lost its selection and "First seen" (#24). Now only the state changes (marked
  *Changed*).
- The live test finds its own sockets by protocol, local address and PID, retries the snapshot up
  to five times (50 ms apart) until both have the expected state, and prints the rows of its own
  PID when it fails (#23). `check_state` (pure, tested) skips the state assertion with a message
  when Windows reports an invalid state, or a valid but wrong one (the own client as Listen,
  FinWait1 or Closed – seen in about 15 of 100 runs on the affected PC) in a snapshot that also
  contains invalid states; on a clean Windows (CI) and on Linux a wrong state fails.

Limit of polling: a connection that opens and closes between two refreshes is never seen. ETW
(Microsoft-Windows-Kernel-Network) would catch those – see the issues.

### Traffic per connection (#8)
`net::estats::Collector` runs in the poll thread after the tables are read, only while
`PollControl::collect_stats` is set – `App::rebuild_view` sets it when a traffic column (Sent,
Received, Sent/s, Received/s, Sent packets, Received packets – `ColumnId::traffic`) is visible,
because the counters cost CPU in the TCP stack. For every TCP connection with a remote side that is
not Listen / TimeWait / Closed (`has_stats`):

- the first time (per `ConnKey` + creation time) `SetPerTcp[6]ConnectionEStats(TcpConnectionEstatsData,
  EnableCollection = 1)` switches the collection on; the counters start then, not when the
  connection was opened;
- every refresh `GetPerTcp[6]ConnectionEStats` reads `TCP_ESTATS_DATA_ROD_v0` (DataBytesOut,
  DataSegsOut, DataBytesIn, DataSegsIn) into `Connection::stats`.

The rows passed to the API are `MIB_TCPROW` / `MIB_TCP6ROW` built by the pure `tcp_row` /
`tcp6_row` (state Established – Windows matches the addresses; the reported state may be invalid,
#21); `actions::tcp_row` (DELETE_TCB) uses the same builder. Windows allows this only to
administrators: the first `ERROR_ACCESS_DENIED` stops the collector for the rest of the run, the
snapshot carries `stats_denied` and the status bar shows "Traffic: administrator only" with an
explanation; the cells stay empty.

`Tracker::update` computes the rates from the difference to the previous snapshot of the same row
(`model::rate`: bytes × 1000 / ms between the snapshots, 0 when a counter went back); the texts are
`columns::bytes` (1 KB = 1024 B) and `columns::rate`. UDP has no per-endpoint counters (ETW, #4).
The properties window also switches the collection on for its graph (#30).

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

## Status bar

`src/app/statusbar.rs`: the refresh time (or "Paused") is measured first and placed at the right
end of the first line; the other items (counts, hints, admin badge, action / export messages,
errors, a newer version) get only the width left of it, in a left-to-right layout with main-axis
wrapping, one line high like `horizontal_wrapped` and with `TextWrapMode::Extend`. In a narrow
window an item that does not fit starts a new line and the panel grows, instead of the right part
painting over the left one (#29); in a wide window it is one line as before.

## Table

`App::view` is a filtered and sorted copy of the tracker rows, rebuilt only when `view_dirty` is
set (new snapshot, expired mark, filter, sort or column change). Sorting compares values
(`columns::compare`), then the `ConnKey` as a tie-breaker, so rows do not jump between refreshes.

Cells are not clipped (`Column::clip(false)`): labels truncate themselves and a marked row paints
its color into the gaps between the cells, so the row is one solid band (same technique as
RusTrace).

### Column order and widths (#2)

`ColumnLayout` (in the settings) holds every column with its visibility and an optional width
(`None` = the default `ColumnId::width`; `normalize` clamps a broken width to 40–2000 px, old files
without widths load unchanged). The table has a fixed id; `egui_extras` keeps the widths of the
columns *by position* in its own state, so whenever the visible column list differs from the one
the widths were applied for (`App::table_cols`) or `App::apply_widths` is set (fit, Default
columns), `TableBuilder::reset` drops that state and the columns start with
`Column::initial(layout.width(id))`. After drawing, `remember_column_widths` writes the header
cell widths back into the layout once no mouse button is down (a resize does not write the
settings every frame); the last column fills the rest of the window and keeps its stored width.

The title of the sorted column is followed by a painted triangle (▲ ascending, ▼ descending):
the characters ⏶ / ⏷, ▲ / ▼ and ⏴ / ⏵ are not in egui's built-in fonts and were drawn as empty
boxes (#22). `table::paint_triangle` draws it; the title label is truncated to the cell width
minus the space reserved for it (`sort_arrow_width`). The ◀ / ▶ move buttons of the column window
(`table::arrow_button`) and the triangle of a group line use the same function.

A header title line senses click and drag over the whole cell width: a click sorts, a drag starts
`App::header_drag` (`src/app/columnlayout.rs`). While dragging, the cell under the pointer and its
half give the drop place (`columns::drop_slot`, pure and tested), drawn as a line in a foreground
layer together with the title at the pointer; near the left / right edge the table scrolls
horizontally (#27). The drop uses the place shown in the last frame (`ColumnDrag::slot`), not the
pointer of the release frame; Esc cancels. `ColumnLayout::move_to` moves the column before / after
the target in the full list (hidden columns keep their places). The column window (View →
Columns, right click on a header) has a ≡ handle per row with the same drag logic turned
vertical, next to the ⏴ ⏵ buttons.

A double click on a header border (within the resize grab radius) fits the column:
`fit_column` measures the title plus the space of the sort arrow and the cell texts of the first 5 000 rows
shown (plus the icon for Process). It is detected by `App` itself, because `egui_extras` 0.37
reads the double click of its resize handle under a different id than it creates it with, so its
own auto-size never triggers.

### Horizontal scrolling (#27)

`egui_extras` tables only scroll vertically, and wrapping the table in a horizontal `ScrollArea`
would move its vertical scroll bar out of view when the table is wide. Instead (same as RusTrace
`7904f90`) `App::table` lays the table out in a child `Ui` whose rect starts `table_scroll_x` left
of the visible area and is clipped (`set_clip_rect`) to it, so the right edge with the vertical
scroll bar stays at the window edge. The column filter row is part of the header and scrolls with
the columns.

Every frame the natural width `table_width` is measured from the header cells (left edge of the
first to the left edge of the last cell, plus `LAST_COLUMN_MIN` = 150 px, the minimum width of the
last `Column::remainder()`); the offset is clamped to `table_width - visible width`, so resizing
the window or a column never leaves a gap. Only when that maximum is above 0 is a horizontal bar
drawn under the table: its own `ScrollArea::horizontal()` with the solid scroll style (floating
bars only appear on hover), always visible, fed with `table_scroll_x` and read back from
`state.clamped_offset().x`; its height is taken from the table's rect. Shift + mouse wheel (egui
turns it into a horizontal delta) and horizontal touchpad scrolling change the offset while the
pointer is over the table. Both fields are runtime state, not saved.

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

### Process properties window (#28)

**Properties…** in the row context menu (for the right-clicked row, disabled for PID 0) and
Alt+Enter (cursor row) open one window (`src/app/properties.rs`, `App::properties`) for a PID; it
is not tied to the selection and is reused for the next process. Its content is the same
`process_grid` as the details pane (`details.rs`, the grid id is a parameter so both can be on
screen; `process_values` gives the name / value lines for the grid and the Copy text) plus the PID
and the number of TCP / UDP endpoints of the PID in the tracker (closed rows and rows of a reused
PID with another start time are not counted). Every frame the window takes the newest
`Arc<ProcessInfo>` from those rows; when none is left it calls `process::start_time` at most once a
second and the pure `liveness` decides: same start time → "no endpoints in the table", PID gone or
another start time → "(process has exited)" with the last known values (no known start time –
access denied – cannot be decided and counts as running). Buttons: Copy, Open file location (the
`show_in_folder` of the export, `explorer /select,"path"`), File properties
(`winproc::file_properties`: `ShellExecuteExW` with the `properties` verb and
`SEE_MASK_INVALIDATELIST`; the shell shows the dialog on its own thread), End process
(`request_kill_process` → the confirmation window of #6), Close. Esc closes the window before it
clears the text filter or the selection.

#### Live traffic and I/O graph (#30)

Below the values a collapsible section draws two `egui_plot` plots of the last
`rates::WINDOW_MS` (2 minutes; x = seconds before the newest sample, fixed bounds, no drag / zoom,
legend in the left top corner because the newest values are on the right):

- **TCP packets per second** – received / sent segments, with the current segments and bytes per
  second as text. The counters are the ESTATS counters of #8: `rebuild_view` keeps
  `PollControl::collect_stats` on while the window is open, not only while a traffic column is
  shown. Without administrator rights (`stats_denied`) a note replaces the plot (on Linux: "only on
  Windows"). UDP has no per-endpoint counters (ETW, #4).
- **I/O operations per second** – read / write / other from `process::io_counters`
  (`GetProcessIoCounters` with `PROCESS_QUERY_LIMITED_INFORMATION`, works without administrator
  rights for the own user's processes; `/proc/<pid>/io` `syscr` / `syscw` on Linux). Disk, network
  and device I/O together – socket I/O of e.g. .NET's WebClient is counted as "other". A note
  replaces the plot when the counters were never readable (access denied).

`App::receive` calls `App::sample_properties(snap.taken)` once per poller snapshot (not every
frame): the TCP counters of the window's rows (same PID / start time / not closed filter as the
endpoint count) keyed by `(ConnKey, created)` and the I/O counters go into `rates::Sampler::push`.
The sampler stores rates between two polls: TCP deltas are taken **per connection and only when
positive** – a closed connection drops out of the sum and a connection seen for the first time (or
whose collection has just been switched on) is only a baseline, so neither makes a negative value
or a spike; I/O needs two readable polls; an equal or older snapshot time adds nothing. Samples
older than the window (or beyond 2 000) are dropped. After the process exits the sampling stops and
the last curves stay. The Copy text adds the latest rates.

## Close connection, End process (#6)

The row context menu has **Close connection** (also Del) and **End process**; both work on all
selected rows shown (`App::selected_rows`, table order).

- `actions::can_close` allows only IPv4 TCP connections with a remote side that are not Listen /
  TimeWait / Closed / DeleteTcb (an invalid state of #21 is allowed – Windows reports it for live
  connections; a listener with an invalid state has no remote side, #24, and a remote port 0 is
  refused as well). Windows has no IPv6 counterpart of `SetTcpEntry`. The disabled menu item has the
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

## Export (#9)

File → Export… (Ctrl+S) opens a window (`src/app/export.rs`): rows (shown / selected / all
endpoints, with counts), columns (visible in table order / all: the visible ones first, then the
hidden ones), format (CSV, Excel, JSON Lines, HTML) with its options, "open the file after the
export", and the last exported file (open / show in folder / copy path). The settings are
`Config::export` (`tabexport::Settings`, every field with a default).

The rows are **copied** out of the view (`Vec<Row>`; "all" = the tracker rows with host names and
the table's sort order), so the export thread never touches the live table and a refresh during
the export changes nothing. The file is chosen with the native save dialog (`rfd`); a known
extension typed there wins over the chosen format. `tabexport::run` writes in a thread
(`mmportlens-export`) and reports progress / cancel through `Control`; a failed or cancelled export
removes its file. The result stays in the status bar for 10 s.

`tabexport::cell` gives every cell a type: PID, ports, bytes, packets and rates (rounded B/s) are
integers, Duration is seconds (`0.000` in Excel), Created / First seen are dates (`Cell::Date`
with the local wall-clock ms for Excel and the ISO text `2026-11-08 12:43:55.123` for CSV / JSON
Lines / HTML); the other columns are the table text (`cell_text`). Column titles with a value that
differs from the table text get the unit (`Sent (B)`, `Duration (s)`); JSON keys are the column
key without `column.` (`local_address`).

The writers come from RusTrace (its #32) with the log-specific parts removed: CSV (RFC 4180
quoting, separator incl. the Windows list separator, UTF-8 BOM / UTF-8 / UTF-16 LE, formula
guard), XLSX (streamed into a deflate ZIP – no zip crate –, typed cells, frozen header,
AutoFilter, widths from the table defaults, split at 1 048 576 rows, cells cut at 32 767
characters, row colors = the light mark colors), JSON Lines (empty cells left out) and HTML (one
file, no scripts). Ctrl+C uses the CSV quoting with a tab separator.

`cargo test xlsx_sample -- --ignored` with `MMPORTLENS_XLSX_OUT=<file>` writes 5 000 rows with
every column for a check in Excel.

## Recording and replay (#3)

**Events.** `Tracker::update_with_events` appends one `recording::Event` per counted change
(new, closed, state changed – the same rules as the marks; the first snapshot produces none).
A closed event has `conn.state = None` and the last state in `state_before`; traffic counters are
not recorded. `Tracker::present_events` gives the starting state (`EventKind::Present`), timed at
the tracker's last snapshot (`App::last_poll`): snapshots taken while the save dialog was open are
still queued and are processed later with later times, so the file stays in time order.

**Start** (`app/record.rs`, File → Start recording… or the red toolbar button): duration (until
stopped, 1 / 5 / 15 / 60 min, custom `10m` / `1h30m` / `90s`), event kinds, "only rows matching the
current filters". The filter is frozen at the start: `App::row_filter()` returns a `RowFilter`
(switches, text filter, column filters, visible columns) – the same struct `rebuild_view` uses –
and the recording keeps a copy. Every refresh `receive` passes the events of the chosen kinds and
endpoints to the writer; the duration ends in `check_recording`, the program waits for the writer
when it closes. The status bar shows `● REC 01:23 / 05:00 · 812 events` and Stop; afterwards the
result with an *Open* button (30 s).

**File** (`recording/db.rs`, the only SQL in the program; `rusqlite` with the bundled SQLite):

- `meta(key, value)`: `format = mmportlens-recording`, `computer`, `start_ms`, `end_ms`,
  `interval_ms`, `app_version`, `events` (recorded kinds), `duration_ms`, `filter`;
  `PRAGMA user_version = 1` (a newer file is refused, a SQLite file without `format` is "not a
  recording").
- `processes(id, pid, name, path, create_time)` – interned by PID + start time + name + path.
- `events(id, time_ms, kind, proto, local_addr, local_port, remote_addr, remote_port,
  state_before, state_after, created, process_id, conn_id)`; kind 0 present / 1 new / 2 closed /
  3 changed, proto 1 TCP / 2 TCPv6 / 3 UDP / 4 UDPv6, states as `MIB_TCP_STATE` numbers
  (`TcpState::to_mib`), addresses as text (readable in DB Browser for SQLite; the view
  `events_readable` spells everything out). `conn_id` numbers the endpoints in the writer.
- Indexes `events_time(time_ms)` and `events_conn(conn_id, time_ms, kind)`.
- WAL journal with `synchronous = NORMAL`, one transaction per refresh; `finish` writes `end_ms`,
  checkpoints (`wal_checkpoint(TRUNCATE)`) and switches to `journal_mode = DELETE`, so a finished
  recording is one file. An existing target file and its journals are replaced.

**Replay** (`RecordingReader::state_at(at, hold)`): for every `conn_id` the last event with
`time_ms <= at` (`max(id) … GROUP BY conn_id`, served by `events_conn`), without endpoints closed
more than `hold` ago; rows whose event is younger than `hold` (`Config::highlight_ms`) are marked
new / changed / closed like in the live table, `first_seen` is its last present / new event.
`neighbours(at)` gives the previous / next event time for the step buttons. Measured with the
ignored test `busy_hour` (release build; 2000 endpoints at the start, 30 events per second for an
hour = 110 000 events): 11.1 MB (101 B per event; 8.7 MB without `events_conn`), the state at the
end of the hour in 32 ms (66 ms without the index, 136 ms grouping by the address columns), a
filtered count + page in 95 ms.

**Threads** (`recording/threads.rs`): `Recorder` owns the `RecordingWriter` in the writer thread
and receives `Vec<Event>` per refresh over a channel; `stop` drops the sender, `finished` joins
once the thread ended (also after a write error, which ends the recording with a message).
`ReplayWorker` owns the `RecordingReader`; requests are coalesced – of the queued ones only the
newest `State` is answered (a dragged slider) and only the pages of the newest event query
(`generation`); the count of a query is computed once per generation.

**UI** (`app/replay.rs`, `app/replay_events.rs`): File → Open recording…, the *Open* button after a
recording, or `mmportlens.exe file.plrec` (`recording::file_arg`, also a double-clicked file) set
`App::replay`. `App::source_rows()` then returns the replayed rows instead of the tracker's, so the
table, column / text filters, sorting, color rules, grouping, details pane and export work
unchanged; the selection is kept within the replayed keys. Close connection, End process and
Properties are disabled (the rows are not live). The replay bar shows the file (tooltip: the meta
data), step buttons, the slider time and *Back to live*. The event list under the table asks the
worker only for the pages (200 events) of the rows on screen (`EventList::missing`, keeping four
pages around them), filters by text (`LIKE` over process, path, addresses, ports, PID; `%` / `_`
escaped) and kinds, sorts by time (header click); a click moves the slider to the event. The live
refresh and a running recording go on in the background.

Not done here: headless recording from the command line (`--record`, `--duration` – #16),
the *Duration* column in the replay counts to now (#31), invalid-state flips of Windows produce
many *State changed* events (#32).

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


## Color rules (#10)

`src/rules.rs` (pure, from the highlight rules of RusTrace): a `ColorRule` has a name, a column
(`None` = any visible column), a pattern in the **column filter syntax** (compiled with
`ColumnFilter::parse` for the column's `FilterKind`, so `3389`, `>1024`, `10.0.0.0/8`, `private`,
`/regex/` work as in the filter row) and text / background colors for the light and the dark
theme (`fg_dark` / `bg_dark` with alpha 0 = not set → copied from the light colors after loading).
`Compiled::new` keeps per rule `None` (off / empty), the filter, or its `FilterError` (⚠ in the
window, the rule colors nothing); `first_match` returns the first matching rule (order decides).
A rule for one column checks that column even when it is hidden; "any column" checks the visible
columns.

- `Config::color_rules` defaults to three examples (RDP remote port 3389, PowerShell processes,
  a disabled 10.0.0.0/8 VPN rule), so existing settings get them once; an emptied list stays
  empty. `Config::marks_over_rules` (default on): the new / changed / closed marks keep their
  color; off, the rule's colors win where the rule sets them (`row_look`).
- The match runs in `rebuild_view` (`App::view_rule`, parallel to `view`), not per frame; the
  table takes `row_colors` (background painted as for the marks, text via `RichText::color`).
- Export (#9): `App::export_looks` gives `tabexport::Job::looks` (light colors, `export_look`), and
  `write_rows` uses them for Excel / HTML; an empty list means "by the marks" (tests use
  `write_table`).
- **View → Color rules…** / Ctrl+H (`src/app/rules.rs`): table of the rules (on, name, column,
  pattern with the error mark, text and background color with on / off, preview on the edited
  theme's background, up / down / delete), theme switch with "Copy from the other theme", "marks
  keep their color", Add, Import (merged without duplicates by name + pattern, `import_rules`),
  Export of ticked rules to JSON, Restore example rules. Every change recompiles and rebuilds the
  view, so the table is the live preview.

## Filter presets (#13)

`src/presets.rs` (pure): a `FilterPreset` is a name plus the protocol switches (TCP, UDP, IPv4,
IPv6, Listening), the toolbar text filter and the column filter texts of #1. Without the name it
is also the snapshot of the current filters (`App::current_filters`); `same_filters` compares
two of them ignoring the name, blank texts and the order of the column filters. `Config::
filter_presets` defaults to examples (All endpoints, Only Established `State =Established`,
Listening ports `State =Listen`, No loopback `Local address !loopback`, Teams `Process teams`; the
state names are the netstat names, not translated, so the filters work in every language).

- Toolbar combo box (`src/app/presets.rs`) before the protocol switches: choosing a preset sets
  everything at once (`apply_preset`: switches, text, `set_column_filters`; a column with a filter
  is made visible, because filters of hidden columns do not apply). The chosen preset
  (`App::active_preset`, runtime only – the text filter is not kept over a restart either) is
  shown with `*` when the current filters differ from it.
- "Save current as…" (name field; an existing name is replaced in place, `presets::save`) and
  "Manage presets…" (rename in place, up / down, 💾 = replace with the current filters, delete,
  restore the examples).
- Command line `--preset <name>` or `--preset=<name>` (`presets::preset_arg`, the first piece of
  #16): applied after start (exact name first, then ignoring case); an unknown name is reported
  in the status bar. The settings of a restart after an update (`--restore`) win over it; an
  elevated restart keeps the argument.

## Group by process (#12)

`src/grouping.rs` (pure) groups the filtered and sorted rows of `rebuild_view` by PID (two
`svchost.exe` are two groups). A group appears where its first row is in the sorted order, so
the groups follow the sort column as well (Process → by name, PID → by PID, other columns → by
their best row) and the rows keep the order inside their group; only rows that passed the filters
are grouped, so empty groups never appear. A group carries the name, PID, the number of TCP /
UDP endpoints and the sum of the traffic counters (#8), when any row has them.

- The rows of a collapsed group (`Config::collapsed_processes`, by process name, so it survives
  a restart) are left out of `App::view`, so selection, keyboard navigation, Ctrl+C, the details
  pane and the export of the "rows shown" keep working on `view` unchanged. The status bar counts
  the filtered rows including the collapsed ones.
- `App::lines` are the table lines (`Line::Group` / `Line::Row(view index)`), `line_of_row` maps a
  view position to its line (keyboard `scroll_to`), `group_samples` keeps a row per group for the
  icon and the tooltip of the group line (also of a collapsed group; `load_icon_textures` includes
  them). Without grouping `ungrouped` gives one line per row.
- The group line (`src/app/groups.rs`) paints an opaque tint over every cell, a painted triangle
  (`table::paint_triangle`; ▶ / ▼ are empty boxes in egui's fonts, #22) and the process icon in the first cell, and the
  text in the last cell with the clip rect of the whole visible line – painted, not a label,
  so it never widens the first column (whose width is remembered, #2), and after every cell's
  background. A click toggles the group.
- Toolbar switch *Group*, View → Group by process / Expand all groups / Collapse all groups.
- 5 000 rows of 300 processes are grouped in about a millisecond (unit test with a generous limit
  for debug builds).
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
- **Failures (#26):** `http::get` returns an `HttpError` – `Status` (code plus the `Retry-After` /
  `X-RateLimit-Remaining` / `X-RateLimit-Reset` headers read with `WINHTTP_QUERY_CUSTOM`),
  `Network` (WinHTTP error number and text) or `Other`. Pure functions decide its `kind()`
  (5xx → server unavailable; 429, or 403 with no requests left → rate limited; 404; network) and
  `retry_delay(attempt)`: only 502 / 503 / 504 and WinHTTP 12002 / 12029 / 12030 are retried, after
  2, 5 and 10 s (a `Retry-After` up to 30 s is honoured); never 4xx. `http::retry` runs the loop
  (the sleep is passed in, so tests do not wait). `update::CheckError::from_http` turns the error
  into a translated text that keeps the raw code ("GitHub is temporarily unavailable (HTTP 503).
  Try again later.", the rate limit with the local reset time), marks it `transient` (server,
  rate limit, network) and `server` (5xx: the window links to githubstatus.com). A transient
  failure does not update `update_last_check`, so the daily check is tried again at the next
  start. The download of an installation uses the same `get` without retries (its errors are
  shown as text).
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
  temporary folder. `MMPORTLENS_UPDATE_URL` replaces the GitHub URL of the check (e.g.
  `https://httpbin.org/status/503` shows the failure handling); `cargo test real_http_errors --
  --ignored` checks the error kinds, the retries and the header reading against httpbin.org and
  GitHub.

## Tests

For checks in the running program the environment variable `MMPORTLENS_CONFIG` points the
settings to another file (`Config::path`), so a test instance never touches
`%APPDATA%\MMPortLens\config.json` (and does not restart itself as administrator when the real
settings say so).

`cargo test` – decoding of the Windows owner module tables (incl. creation times, row offsets),
the live creation time and owner module of an own connection, `/proc/net` lines, ESTATS rows and
counters (elevated: 200 KB over an own connection are counted; not elevated: exactly
AccessDenied), traffic rates between snapshots, the rate sampler of the properties graph (per-connection deltas, closed / new connections, resets, window), `/proc/<pid>/io` parsing and the own I/O counters, byte / rate texts, state codes, the tracker
(new / changed / closed / expiry / reappearing), column layout and sorting, settings round trip,
language files, reverse DNS (cache TTL / negative results / pending, `sockaddr` layout, a
300-address run through the worker pool with a slow fake resolver – never more than `WORKERS` at
once, the caller never blocks –, the system resolver on 127.0.0.1), port names, process details (own process: start time, user, command line; notepad.exe: company,
description, icon; services by PID; start time identifies a reused PID; icon alpha / mask
conversion, FILETIME, version keys), close / kill checks and the `MIB_TCPROW` layout, ending an own child process,
closing an own loopback connection (elevated: it disappears; not elevated: exactly
`AccessDenied`), host / service
cells, selection (clicks, ranges, re-sorting, navigation targets), the table export (typed cells, every writer, cancel), column filters (syntax, regex, numeric ranges, CIDR, address classes, errors,
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
  runs `cargo update --workspace` instead, so `Cargo.lock` still gets the new version. The run of
  the tag is found with `gh run list --json` filtered by `ConvertFrom-Json` / `Where-Object`, not
  with `--jq`: Windows PowerShell 5.1 drops double quotes inside native arguments (#25), so the
  script must not pass quoted strings to native programs.
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
