# Saved filter presets

## Summary
Save the current combination of filters (protocol switches, text filter, column filters) under a name and switch between presets quickly, e.g. "Only Established", "Listening ports", "Teams", "No loopback".

## Details
- Toolbar combo box with the presets, "Save current as…", "Manage presets…" (rename, delete, order).
- Stored in the settings; a few built-in presets on first run.
- Command line: `--preset <name>`.

## Acceptance
- Selecting a preset sets all filters at once; editing a filter afterwards shows the preset as modified (*).
