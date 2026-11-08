# MM PortLens – roadmap (order of the issues)

The order in which the GitHub issues are solved, agreed with the owner. The owner refers to the
phases by number ("fáza 1" = phase 1). Keep the status column up to date: mark an issue done
when its commit lands, add new issues to the phase where they fit. When every issue of a phase is
done, add "(done)" to the phase heading, and once a release ships it, name that release there
("(done, released in vX.Y.Z)"; "(done, not released yet)" until then).

Every issue follows the procedure of `CLAUDE.md`: take the code from RusTrace where it exists,
`t!` strings in all four languages, `Config` defaults, fmt + clippy + tests, check in the running
program, README / `docs/architecture.md` / `docs/history.md`, detailed commit with `Closes #N`
(or `Part of #N`). Nothing is pushed or released without asking.

## Phase 0 – quick fix before #15 (done, released in v0.1.5)
| Issue | What | Status |
|-------|------|--------|
| #21 parts 1, 2, 4 | Invalid TCP states: live test finds own sockets by address and PID and skips the state check for state 0 / > 12; UI shows "Invalid (0)" | done (67eb388) |

## Phase 1 – chosen by the owner (done, released in v0.1.5)
| Issue | What | Status |
|-------|------|--------|
| #15 | Icon (`src/icon.rs`), `build.rs` + `winresource`, update check / in-place update (`update.rs`, `http.rs` from RusTrace) | done (734d765) |
| #15 | Administrator mode: restart as administrator, always start as administrator (one exe) | done (10588a3) |
| #15 | Acceptance: a release built by the workflow (v0.1.5) shows its notes in the update window (v0.1.0 has no update check: checked with the released v0.1.5 and `MMPORTLENS_UPDATE_AS_VERSION=0.1.0`) | done (v0.1.5) |
| #1 | Column filters under the headers (`colfilter.rs` from RusTrace + regex, ranges, CIDR, address classes) | done (d7cf3fe) |

## Phase 2 – the table and shared code (done, released in v0.2.5)
| Issue | What | Status |
|-------|------|--------|
| #27 | Horizontal scroll bar when the columns are wider than the window (port from RusTrace `7904f90`); before #2, which makes wide tables more common | done (b96c34c) |
| #2 | Column order by drag and drop, remembered column widths (same table code as #1) | done (50c362f) |
| #9 | Export CSV / Excel / JSON (`tabexport/` from RusTrace; exports the filtered view of #1) | done (9f9de77) |

## Phase 3 – parity with TCPView (done, released in v0.2.0)
| Issue | What | Status |
|-------|------|--------|
| #17 | Keyboard shortcuts and multi-selection (needed for bulk actions in #6 and copying) | done (a85607f) |
| #5 | Reverse DNS (off the UI thread) | done (a56191c) |
| #6 | Close a connection, end a process | done (fec39e0); closing as administrator not yet checked by hand |
| #7 | Process details (svchost services, user, command line, icon) | done (3ffd6a3) |
| #14 | Real creation time and owner module of TCP connections | done (1f660a1) |
| #8 | Bytes sent / received per connection (ESTATS, needs admin – administrator mode of #15 exists) | done (50055c3); the counters as administrator not yet checked by hand |

## Phase 4 – comfort (done, released in v0.2.9)
| Issue | What | Status |
|-------|------|--------|
| #28 | Process properties window from the row context menu (reuses the process details of #7 and the end-process action of #6) | done (af117ca) |
| #10 | Custom color rules for rows (highlight rules from RusTrace) | done (83df645) |
| #13 | Saved filter presets (builds on #1) | done (0cd38de) |
| #12 | Group rows by process | done (7e6f2af) |

## Phase 4b – asked by the owner after v0.2.9 (done, released in v0.3.7)
| Issue | What | Status |
|-------|------|--------|
| #30 | Live traffic (TCP packets, ESTATS of #8) and I/O operations graph in the process properties window of #28, `egui_plot` (reusable by #19) | done (0243216); the TCP plot as administrator not yet checked by hand |

## Phase 4c – asked by the owner after #3 (done; #33–#36 released in v1.0.7, #42 in v1.1.1)
| Issue | What | Status |
|-------|------|--------|
| #33 | Process properties (#28, #30) in their own OS windows (egui viewports), several at a time, one per PID | done ("Process properties in their own OS windows"), released in v1.0.7 |
| #34 | Service name of well-known ports: tooltip over the whole port cell (with the owning Windows service of #14), `port (name)` in the details pane, optional system services file | done ("Port service names: whole-cell tooltip, details, system services file"), released in v1.0.7 |
| #35 | Column filters: AND (`&`) and NOT (`!`) per term, precedence `!` > `&` > `\|`, migration of saved `!a\|b` texts (filters, presets, highlight rules); before #16, which reuses the filters. RusTrace port: MiroM73/Rustrace#61 | done ("Column filters: AND and per-term NOT"), released in v1.0.7; the RusTrace port is still open |
| #36 | Filter field tooltip: examples for the column and its possible values (State, Protocol); after #35 so the examples can use `&` | done ("Column filter tooltip: examples and values per column"), released in v1.0.7 |
| #42 | Help → Command line window (#38) in its own OS window like the process properties of #33 (focus when open, Esc / ×, remembered position and size, embedded fallback), zoom as in the `--help` window of #39, the #33 setting becomes a general View → *Open windows separately* (old value kept) | done ("Help → Command line in its own zoomable window"), released in v1.1.1 |

## Phase 5 – recording and automation (#3, #16, #4 released in v1.0.0; #38, #39 in v1.0.7; #19, #31, #40 in v1.1.1)
| Issue | What | Status |
|-------|------|--------|
| #3 | Record connection events for a period of time | done ("Record connection events and replay them"), released in v1.0.0; the command-line part is #16 |
| #16 | Command line: filters (#1), export (#9), recording (#3) – therefore after them | done ("Command line: filters, headless export and recording"), released in v1.0.0 |
| #4 | ETW for short-lived connections (extends the recording of #3) | done ("Capture short-lived connections with ETW"), checked as administrator (acceptance test 100 + 100 connections, `--record --etw`); released in v1.0.0 |
| #18 | Watch rules and alerts (builds on #10 and #3) | |
| #19 | Statistics over time (data from the recording of #3) | done ("Statistics window: connections over time, top processes, hosts and ports"), released in v1.1.1; charts for an opened recording are not part of it |
| #11 | Country and ASN (offline) – least critical, depends on an offline database | |
| #31 | Replay: the Duration column counts to now instead of the slider time (found in the #3 check) | done ("Replay: Duration counts to the slider time"), released in v1.1.1 |
| #32 | Invalid TCP state flips of Windows flood recordings with State changed events (found in the #3 check) | |
| #37 | Process names of system / other users' processes without administrator rights from the process snapshot instead of `<pid>` (found in the #16 check) | |
| #38 | Help → Command line window: options, column names, exit codes and examples with copy buttons, from the same structured data as `--help` (extends #16) | done ("Help → Command line window"), released in v1.0.7 |
| #40 | Console output of the command line garbled in an OEM code page console (`–` → `ÔÇô`; found in the #4 check) | done ("Console messages in the code page PowerShell decodes with"), released in v1.1.1; RusTrace port is MiroM73/Rustrace#62 |
| #44 | Shells do not wait for the GUI exe; cmd `>` and PowerShell `$x = …` lose the messages and the exit code (found in the #40 check) | |
| #39 | `--help` / `-h` / `-?` / `/?` in a styled egui window instead of the message box: resizable by the bottom-right corner, text zoom (Ctrl + wheel, Ctrl + `+` / `-` / `0`); reuses the renderer of #38, so after it | done ("--help in a styled, resizable window"), released in v1.0.7 |

## Any time (small fixes found on the way) – done up to #29 released in v0.3.7, #41 in v1.0.7
| Issue | What | Status |
|-------|------|--------|
| #21 part 3 | Status bar hint when Windows reports invalid TCP states | done (df3a539) |
| #22 | Sort arrow in the header drawn as an empty box | done (4324a64) |
| #23 | Live snapshot test failed once (flaky) – better message, retry; reproduced: own connection reported as Listen (#21) | done (91e856a) |
| #24 | Listener rows flicker between two keys when Windows alternates state 0 and Listen | done (929cd05) |
| #25 | `release.ps1` loses the jq quotes under Windows PowerShell 5.1 while waiting for the workflow | done (16d6136) |
| #26 | Update check: retry transient GitHub errors (HTTP 502/503/504, timeouts) and explain them; port the change back to RusTrace (copied `update`/`http` code, see #20) | done (739a9c4); RusTrace port is MiroM73/Rustrace#60 |
| #29 | Status bar texts overlap in a narrow window (found in the #27 check) | done (10e1140) |
| #41 | Multi-line values in the language files cut after the first line: filter tooltip, regex error (found in #35) | done ("Language files: one line per value"), released in v1.0.7 |
| #43 | wgpu validation error (panic) when a window is larger than the maximum texture size, 8192 px (found in the #31 check) | |

## Deferred indefinitely (owner's decision)
| Issue | What | Status |
|-------|------|--------|
| #20 | Shared crate with RusTrace – the modules stay copied; fixes in copied code (i18n, colfilter, update/http, tabexport) are ported to the other program by hand. The diff of the copies at the time of the decision is in `docs/history.md` | deferred 2026-11-08 |
