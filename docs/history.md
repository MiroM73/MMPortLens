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
