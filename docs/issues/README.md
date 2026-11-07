# Planned features – issue drafts

Each file is one GitHub issue: the first line (`# …`) is the title, the rest is the body.
`scripts/create-issues.ps1` creates them in `MiroM73/MMPortLens` with the `enhancement` label.
Once created, the issues on GitHub are the source of truth; these drafts are not updated.

- [Column filters under the column headers](01-column-filters.md)
- [Column order by drag and drop, remembered column widths](02-column-order-widths.md)
- [Record connection events for a period of time](03-recording.md)
- [Capture short-lived connections with ETW (Windows)](04-etw-short-connections.md)
- [Resolve remote addresses to host names](05-reverse-dns.md)
- [Close a connection, end a process](06-close-connection-kill-process.md)
- [Process details: services of svchost, user, command line, icon](07-process-details.md)
- [Bytes sent / received per connection](08-traffic-statistics.md)
- [Export the table to CSV, Excel, JSON](09-table-export.md)
- [Custom color rules for rows](10-highlight-rules.md)
- [Country and ASN of remote addresses (offline)](11-geoip.md)
- [Group rows by process](12-group-by-process.md)
- [Saved filter presets](13-filter-presets.md)
- [Real creation time and owner module of TCP connections](14-create-time-module.md)
- [Icon, version resource, release workflow, update check](15-packaging-icon-updates.md)
- [Command line: filters, headless snapshot export and recording](16-command-line.md)
- [Keyboard shortcuts and multi-selection](17-keyboard-selection.md)
- [Watch rules and alerts](18-watch-alerts.md)
- [Statistics: connections over time, top processes and hosts](19-statistics-view.md)
- [Shared code with RusTrace in a common crate](20-shared-crate-with-rustrace.md)
