# Changelog

## Unreleased

### Added

- **Explicit recovery for blocked pulls.** `gitpp pull --stash-local` temporarily stashes
  recoverable local changes and restores only a stash entry created by that invocation. A stash
  pop conflict is reported as Failed while the stash is retained. `gitpp pull --discard-local
  <repo...>` provides a confirmation-gated, target-limited route for discarding
  uncommitted changes; ambiguous repository basenames require `group/repo`.

## v0.7.7 — 2026-06-05

### Added

- **Submodule sync on every pull.** `gitpp pull` now runs
  `git submodule update --init --recursive` after fetching, so repos with
  `vendor/*` submodules no longer drift into a dirty state that silently
  blocks future pulls. `clone` now uses `--recurse-submodules` so a freshly
  cloned repo starts clean.

### Changed

- **Robust pull that never gets stuck.** `pull` is now
  `git fetch --prune` → fast-forward the current branch (`merge --ff-only`)
  → submodule sync. A dirty working tree, a missing upstream, a detached
  HEAD, or a diverged branch no longer aborts the pull — only a failed
  fetch is reported as Failed, and local changes are never discarded. The
  YAML `branch:` field stays clone/switch-only; pull and push both act on
  the current branch.

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
