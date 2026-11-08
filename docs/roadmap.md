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

## Phase 4 – comfort
| Issue | What | Status |
|-------|------|--------|
| #28 | Process properties window from the row context menu (reuses the process details of #7 and the end-process action of #6) | done (af117ca) |
| #10 | Custom color rules for rows (highlight rules from RusTrace) | done (83df645) |
| #13 | Saved filter presets (builds on #1) | done (0cd38de) |
| #12 | Group rows by process | done |

## Phase 5 – recording and automation
| Issue | What | Status |
|-------|------|--------|
| #3 | Record connection events for a period of time | |
| #16 | Command line: filters (#1), export (#9), recording (#3) – therefore after them | |
| #4 | ETW for short-lived connections (extends the recording of #3) | |
| #18 | Watch rules and alerts (builds on #10 and #3) | |
| #19 | Statistics over time (data from the recording of #3) | |
| #11 | Country and ASN (offline) – least critical, depends on an offline database | |

## Any time (small fixes found on the way)
| Issue | What | Status |
|-------|------|--------|
| #21 part 3 | Status bar hint when Windows reports invalid TCP states | |
| #22 | Sort arrow in the header drawn as an empty box | |
| #23 | Live snapshot test failed once (flaky) – better message, retry; reproduced: own connection reported as Listen (#21) | |
| #24 | Listener rows flicker between two keys when Windows alternates state 0 and Listen | |
| #25 | `release.ps1` loses the jq quotes under Windows PowerShell 5.1 while waiting for the workflow | |
| #26 | Update check: retry transient GitHub errors (HTTP 502/503/504, timeouts) and explain them; port the change back to RusTrace (copied `update`/`http` code, see #20) | |
| #29 | Status bar texts overlap in a narrow window (found in the #27 check) | |

## Deferred indefinitely (owner's decision)
| Issue | What | Status |
|-------|------|--------|
| #20 | Shared crate with RusTrace – the modules stay copied; fixes in copied code (i18n, colfilter, update/http, tabexport) are ported to the other program by hand. The diff of the copies at the time of the decision is in `docs/history.md` | deferred 2026-11-08 |
