# Icon, version resource, release workflow, update check

## Summary
Bring over the release infrastructure of RusTrace.

## Details
- Program icon drawn in code (`src/icon.rs`) + `build.rs` with `winresource` (icon + version info), start-up splash if useful.
- `release.yml` workflow and `scripts/release.ps1` / `package.ps1` (ZIP with `mmportlens.exe`, `LICENSE`, `lang/`).
- Update check and in-place update from GitHub releases (`src/update.rs`, `src/http.rs` of RusTrace) with the same settings (at start / never).
- Statically linked CRT is already set in `.cargo/config.toml`.
- Optional application manifest `requireAdministrator` variant (separate exe or setting "always start as admin").

## Acceptance
- `v0.2.0` release built by the workflow, update window shows the release notes.
