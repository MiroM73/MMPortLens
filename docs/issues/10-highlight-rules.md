# Custom color rules for rows

## Summary
Color rows by user rules, as in RusTrace (Highlight rules): e.g. RDP (`3389`) red, connections of `powershell.exe` orange, everything to a VPN range blue.

## Details
- Reuse the rule model and editor from RusTrace (`HighlightRule`, per-theme colors, rules dialog) and match against RusNet columns (with the column filter syntax from the column-filters issue).
- Order of rules decides; the new / changed / closed marks have priority over rules (configurable).
- Built-in example rules on first run.

## Acceptance
- Rules editor with live preview; rules saved in the settings and exportable / importable.
