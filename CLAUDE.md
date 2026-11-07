# MM PortLens – project memory for Claude

## Working with the owner
- The owner (Miroslav, GitHub `MiroM73`) talks to Claude in **Slovak, informal "ty" form**. Answer in Slovak.
- Everything inside the repository is in **English**: code, comments, README, commit messages,
  GitHub issues (with detailed descriptions) – except the translations in `lang/`.
- After every finished change: run the tests, verify the behaviour, update the documentation
  (`README.md` is a short overview; internals go into `docs/architecture.md`, a line per issue into
  `docs/history.md`), and make a commit with a **detailed English commit message**
  (what changed, why, how it was verified).
- When Claude runs into something broken, a performance problem or an improvement idea while
  working on the project, it creates a new GitHub issue for it.
- The **first block of the README** states that everything was created by Claude (Claude Opus 5.5,
  `claude-opus-5-5`, Anthropic) and the owner only specified requirements. Keep it at the top.
- License: **the Unlicense** (`LICENSE`, `license = "Unlicense"` in Cargo.toml).
- Do not push or rewrite published history without asking.

## Naming
- All programs of the owner start with **MM**: display name with a space (**MM PortLens**), technical
  name without (`MMPortLens` repository and settings folder, `mmportlens` crate / exe). The project
  was called RusNet before the first publication (renamed because of RustNet and the "Rus" prefix).

## What the application is
A native desktop viewer of TCP / UDP endpoints in Rust (eframe/egui 0.37) modelled on **TCPView**
from Sysinternals, with extra features (column filters, column layout, recording, …). Sibling of
**RusTrace** (`MiroM73/Rustrace`, a CMTrace-style log viewer) – same structure and conventions;
when a feature exists in RusTrace (i18n, column filters, auto refresh, export, highlight rules,
updates, CLI), take its code and adapt it rather than writing it again.

- **`docs/architecture.md`** – how every part works inside and the source layout.
- **`docs/history.md`** – what was done when.
- **`docs/roadmap.md`** – the agreed order of the issues in phases; "fáza N" from the owner means
  phase N there. Read it before starting work on issues and update its status column with every commit.
- **`docs/issues/`** – drafts of the planned GitHub issues (`scripts/create-issues.ps1` creates them).

## Code layout
- `src/net/` – platform backends (`windows.rs` iphlpapi, `procfs.rs` Linux) + pure decoding
  (`winrows.rs`). Keep decoding pure and unit-tested.
- `src/process.rs`, `src/poller.rs` (background thread), `src/model.rs` (`Tracker`: new / changed /
  closed marks), `src/columns.rs`, `src/config.rs`, `src/i18n.rs`, `src/timefmt.rs`.
- `src/app/` – the UI split by area: `mod.rs` holds `App` and the frame loop; every other file is an
  `impl App` block starting with `use crate::app::*;`; methods used outside their file are
  `pub(in crate::app) fn`. Keep files under ~800 lines; a new window → its own file.

## Conventions
- **Every user-visible string goes through `t!("key")`**; a new key goes into **all four**
  `lang/*.lang` files with the same placeholders (enforced by `i18n::tests::builtin_languages_are_complete`).
- Heavy work (reading tables, DNS, files) stays off the UI thread.
- New settings: add to `Config` with a default (`#[serde(default)]`).
- Prefer pure, unit-tested functions in modules, thin UI code in `src/app/`.
- egui 0.37 API notes: `eframe::App::ui(&mut self, ui, frame)`, `egui::Panel::top/bottom(..).show(ui, ..)`,
  `egui::MenuBar::new().ui(..)`, `ui.close()`.
- Win32 calls are declared directly (`unsafe extern "system"` + `#[link(name = "...")]`) with a
  `// SAFETY:` comment; no binding crate so far.

## Commands
- `cargo build --release` → `target\release\mmportlens.exe`; `cargo test`.
- `cargo fmt` + `cargo clippy --all-targets --locked -- -D warnings` – CI fails otherwise.
- Do not leave test runs touching `%APPDATA%\MMPortLens\config.json`.
- Line endings: `.gitattributes` forces LF (`* text=auto eol=lf`).
