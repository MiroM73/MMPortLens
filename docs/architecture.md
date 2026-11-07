# MM PortLens – architecture

How the program works inside. Read the section of a feature before changing it and update it
with the change.

## Source layout

| File | What it does |
|------|--------------|
| `src/main.rs` | Window start-up. |
| `src/net/mod.rs` | `Connection`, `ConnKey`, `Protocol`, `TcpState`; `snapshot()` picks the platform backend. |
| `src/net/winrows.rs` | Pure decoding of the iphlpapi table buffers (tested on every platform). |
| `src/net/windows.rs` | Windows backend: `GetExtendedTcpTable` / `GetExtendedUdpTable`, declared directly with `unsafe extern "system"` (no binding crate). |
| `src/net/procfs.rs` | Linux backend: `/proc/net/{tcp,tcp6,udp,udp6}` + socket inode → PID from `/proc/<pid>/fd`. |
| `src/process.rs` | PID → name and path (`QueryFullProcessImageNameW` / `/proc/<pid>/exe`), cached per PID. |
| `src/poller.rs` | Background thread: snapshot + process names every interval, sent over a channel. |
| `src/model.rs` | `Tracker`: merges snapshots into rows, marks new / changed / closed rows and expires the marks. |
| `src/columns.rs` | `ColumnId`, cell texts, value-based sorting (`SortSpec`), `ColumnLayout` (visibility + order). |
| `src/config.rs` | `Config` (JSON, every field has a default). |
| `src/i18n.rs` | `t!()` and the language files – the same module as in RusTrace. |
| `src/timefmt.rs` | Clock times for the table. |
| `src/app/` | The UI, split by area like RusTrace: `mod.rs` (state, frame loop), `menu.rs`, `toolbar.rs`, `table.rs`, `statusbar.rs`. |
| `lang/*.lang` | Built-in translations (en, sk, cs, de), embedded with `include_str!`. |

## Reading the connections

`net::snapshot()` returns every endpoint once. On Windows four calls are made
(TCP/UDP × IPv4/IPv6) with `TCP_TABLE_OWNER_PID_ALL` / `UDP_TABLE_OWNER_PID`; the buffer is a
`Vec<u32>` (4-byte aligned) and grows when the API reports `ERROR_INSUFFICIENT_BUFFER`. The bytes
are decoded by `winrows`, whose tests build buffers in the exact Windows layout, and the test
`net::tests::live_snapshot_sees_own_sockets` opens a listener and a connection and checks that the
real backend reports them with the own PID – on the Windows CI it validates the iphlpapi path.

A listener has no remote side (`remote = None`), the same as UDP endpoints.

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

## Settings and languages

`Config` is saved debounced (0.8 s after the last change, and on close) through a temporary file.
`ColumnLayout::normalize` repairs layouts from older versions: duplicates removed, new columns
added hidden at the end, at least one column visible.

Every user-visible string goes through `t!("key")`; `i18n::tests::builtin_languages_are_complete`
checks that all four built-in languages have the same keys and placeholders.

## Tests

`cargo test` – decoding of the Windows tables and `/proc/net` lines, state codes, the tracker
(new / changed / closed / expiry / reappearing), column layout and sorting, settings round trip,
language files, and the live snapshot test.
