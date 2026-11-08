# MM PortLens – a TCPView-style network connection viewer

> **This program was created entirely by AI.** Everything in this repository – the source code,
> program logic, architecture, user interface design, translations, tests, build scripts and the
> documentation – was created by **Claude Opus 5.5** (model ID `claude-opus-5-5`), a large
> language model by [Anthropic](https://www.anthropic.com). The project owner only specified the
> requirements and gave feedback; he did not write the code.
> The code was compiled and tested (unit tests and checks in the running application) during
> generation. Review it like any other third-party code before using it in production.

A desktop application in Rust (egui/eframe) that lists all TCP and UDP endpoints of the computer
with the process that owns them – like [TCPView](https://learn.microsoft.com/sysinternals/downloads/tcpview)
from Sysinternals, with the features TCPView is missing (column filters, column layout,
recording, …). It is the sibling of [RusTrace](https://github.com/MiroM73/Rustrace) and follows
its structure and conventions.

[![CI](https://github.com/MiroM73/MMPortLens/actions/workflows/ci.yml/badge.svg?branch=master)](https://github.com/MiroM73/MMPortLens/actions/workflows/ci.yml)

## Status

Early version (0.2.0). What works now:

- TCP and UDP endpoints, IPv4 and IPv6, with PID, process name and process path
  (Windows: `GetExtendedTcpTable` / `GetExtendedUdpTable`; Linux: `/proc/net`).
- The real **creation time** of each endpoint and how long it exists, and its **owner module** –
  for an endpoint of a service in `svchost.exe` the exact service (Windows).
- Automatic refresh in a background thread: pause / resume, refresh now, interval 0.5 s – 30 s.
- New endpoints are **green**, endpoints that changed their TCP state **yellow**, closed endpoints
  stay **red** for 3 seconds before they disappear.
- Sorting by a click on a column header (numbers and addresses sort as values).
- Columns on / off and their order: **View → Columns** or a right click on a header.
- A **horizontal scroll bar** under the table when the columns do not fit the window
  (Shift + mouse wheel scrolls as well).
- Switches TCP / UDP / IPv4 / IPv6 / Listening and a text filter over the visible columns.
- **Host names** of the addresses (reverse DNS in the background, cached) in the columns *Remote
  host* and *Local host*, and the **service** of well-known ports (443 → https) – switch with
  **View → Resolve addresses** or the *DNS* toolbar button.
- **Column filters** under the headers: text, `=exact`, `/regex/`, `a|b`, `!not`; ports and PID
  `>1024`, `1000-2000`; addresses `10.0.0.0/8`, `fe80::/10`, `loopback`, `private`, `public`
  (hover a field for the syntax).
- **Traffic per TCP connection** (as administrator): bytes and packets sent / received and the
  current rates, collected only while one of these columns is shown.
- **Process details**: icon, description and company from the version resource, user, command
  line, start time and the **Windows services** running in the process (which service of
  `svchost.exe` it is) – in the details pane under the table, as columns and as a tooltip.
- **Close connection** (IPv4 TCP, like TCPView) and **End process** from the context menu or with
  Del, for all selected rows, after a confirmation that lists them.
- **Multi-selection and keyboard shortcuts**: Ctrl / Shift + click, arrows, Ctrl+A, Ctrl+C copies
  the selected rows, F5, Space pauses, Ctrl+F, Esc – see
  [`docs/keyboard-shortcuts.md`](docs/keyboard-shortcuts.md) or **Help → Keyboard shortcuts** (F1).
- Status bar with counts, user interface in English, Slovak, Czech and
  German (more languages without recompiling – see `lang/en.lang`), dark / light theme.
- Settings in `%APPDATA%\MMPortLens\config.json`.
- Update check at start (or daily / never) with the release notes of the missed versions and
  in-place installation with restart (**Help → Check for updates**).

Planned features are tracked as [GitHub issues](https://github.com/MiroM73/MMPortLens/issues)
(drafts in [`docs/issues/`](docs/issues/README.md)).

## Download

Ready-made Windows builds are on the [Releases](https://github.com/MiroM73/MMPortLens/releases)
page, in two forms with the same content:

- `MMPortLens-<version>-windows-x64.exe` – self-extracting archive: run it and choose a destination folder,
- `MMPortLens-<version>-windows-x64.zip` – plain ZIP to extract yourself (e.g. Explorer → Extract all).

Both create:

```
MMPortLens\
    mmportlens.exe    the program (no installation, no Visual C++ runtime needed)
    lang\*.lang       language files – edit or add translations here
    LICENSE           the Unlicense (public domain)
```

The programs are not code-signed, so Windows SmartScreen may ask for confirmation
("More info" → "Run anyway"). The `.sha256` files contain the checksums.

## Build

```
cargo build --release      → target\release\mmportlens.exe
cargo test
```

Run it **as administrator** to see the paths of all processes (without admin rights Windows does
not give the path of other users' and system processes; the PID is always shown): **File → Restart
as administrator** (or the "Not administrator" button in the status bar) restarts it through UAC
with the same filters, **File → Always start as administrator** does it at every start.

## Documentation

- [`docs/architecture.md`](docs/architecture.md) – how it works inside, source layout.
- [`docs/history.md`](docs/history.md) – what was done when.
- [`docs/roadmap.md`](docs/roadmap.md) – the order in which the issues are solved.
- [`docs/keyboard-shortcuts.md`](docs/keyboard-shortcuts.md) – keyboard and mouse shortcuts.
- [`CLAUDE.md`](CLAUDE.md) – working notes for Claude.

## License

[The Unlicense](LICENSE) – public domain.
