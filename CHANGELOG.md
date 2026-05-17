# Changelog

## v0.7.6 — 2026-05-17

### Added

- **Completion bloom on the repo list.** When a parallel git job finishes,
  its repo name briefly fades from a dark shade of its status color to the
  bright status color over ~180 ms (Updated → green, Failed → red,
  Untracked → magenta, Unchanged → gray). Makes the "burst of completions"
  at the end of a `gitpp pull` over 60+ repos visually catchable. Powered
  by the [`jiwa`](https://crates.io/crates/jiwa) crate, shared with
  `type-globe` and `curion`.

### Fixed

- Pre-existing clippy `collapsible_match` warnings (j/k movement keys
  under `if self.selected ...` guards) rewritten as match guards so
  `cargo clippy -- -D warnings` is clean on current stable.

## v0.7.5 — 2026-04-13

Terminal control byte leak fix on exit (#29) — stop leaking stray mouse
bytes to the parent shell when the TUI tears down.

## v0.7.4 and earlier

GitHub Actions standardization (Node 22, action versions, branch
triggers, cargo flags). See git history for the full pre-v0.7.4 trail.
