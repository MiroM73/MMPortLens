# RusNet – history

## 0.1.0 (2026-11-07) – skeleton
- Project created in the structure of RusTrace: Cargo, eframe/egui 0.37, CI (Windows + Linux),
  i18n module and language files (en, sk, cs, de), settings.
- Reading TCP / UDP endpoints (IPv4 + IPv6) with PIDs: Windows iphlpapi, Linux `/proc/net`.
- Process name and path per PID (cached).
- Background refresh with pause, refresh now and interval; new / changed / closed rows highlighted.
- Table with sorting, columns on / off and their order (View → Columns, right click on a header),
  protocol switches, text filter, copy row, status bar.
- Planned features written as issue drafts in `docs/issues/`.
