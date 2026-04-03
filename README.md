# gitpp

[日本語](README.ja.md)

[![CI](https://github.com/kako-jun/gitpp/actions/workflows/ci.yml/badge.svg)](https://github.com/kako-jun/gitpp/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/gitpp.svg)](https://crates.io/crates/gitpp)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

Git Personal Parallel Manager — manage 100+ Git repos with one command.

## Why gitpp?

When you maintain 100+ repos across multiple machines, the session start ritual gets old fast:

```bash
# Without gitpp — repeated for every repo
cd ~/repos/private/project-a && git pull
cd ~/repos/private/project-b && git pull
cd ~/repos/2025/tool-x       && git pull
# ... 97 more times
```

With gitpp — one command, all repos in parallel, with a live TUI:

```bash
gitpp pull
```

That covers the basic case. gitpp also solves a subtler problem: **commit author identity**.

If you have personal OSS repos, work repos, and hobby side-projects on the same machine,
setting `user.name`/`user.email` in `~/.gitconfig` means one identity wins and the others
get wrong attribution. gitpp takes the opposite approach — it sets `git config --local`
on each repo based on the YAML file in that directory tree, so identity follows location,
not a global config file.

## Features

- **Parallel clone/pull/push** with configurable concurrency (`jobs`, default 20)
- **Full-screen TUI** (ratatui) with real-time per-repo progress bars and status icons
- **Per-directory git config** — `user.name`, `pull.rebase`, and any other git config key,
  applied locally to every repo in the group
- **Push opt-in** — push is disabled unless `comments.default` is explicitly set; clone/pull
  work without it
- **AI-friendly summary** — plain-text output after completion, paste directly to your AI
  assistant for diagnosis
- **Interactive REPL** mode with tab completion and history
- **Quiet mode** for scripts and CI (no TUI, summary to stdout)
- Single binary, zero runtime dependencies

## What it does

Put a `gitpp.yaml` in your repos directory, then:

```bash
gitpp clone   # Clone all repos in parallel
gitpp pull    # Pull all repos in parallel
gitpp push    # Add, commit, push all repos in parallel
```

A full-screen TUI shows real-time progress for every repository:

**List mode (default):**

```
┌──────────────────────────────────────────────────────────────┐
│ gitpp  j/k:move  Enter:detail  h/l:scroll  q:quit           │
└──────────────────────────────────────────────────────────────┘
┌─ Repositories [1-20/101] ────────────────────────────────────┐
│▸✓ freeza                           Done                     │
│  [████████████████████████████████████████] 100%             │
│ ⚙ sss                              Pulling...               │
│  [████████████████████░░░░░░░░░░░░░░░░░░░░]  50%            │
│ ⏸ noun-gender                       Waiting...              │
│  [░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░]   0%           │
└──────────────────────────────────────────────────────────────┘
┌──────────────────────────────────────────────────────────────┐
│ Total: 101 | Done: 50 | OK: 48 | Fail: 2                   │
└──────────────────────────────────────────────────────────────┘
```

The detail pane is shown by default. Press Enter to toggle it off/on:

**Detail mode (default):**

```
┌──────────────────────────────────────────────────────────────┐
│ gitpp  j/k:move  Enter:detail  h/l:scroll  q:quit           │
└──────────────────────────────────────────────────────────────┘
┌─ Repositories [1-20/101] ──────┬─ sss ───────────────────────┐
│   ✓ freeza         Done  100% │ remote: Enumerating objects:  │
│▸⚙ sss           Pull..   50% │   12, done.                  │
│   ⏸ noun-gender   Wait..   0% │ Receiving objects:  60%      │
│                                │   (7/12) 1.2 MiB            │
└────────────────────────────────┴──────────────────────────────┘
┌──────────────────────────────────────────────────────────────┐
│ Total: 101 | Done: 50 | OK: 48 | Fail: 2                   │
└──────────────────────────────────────────────────────────────┘
```

After all repos finish, a plain-text summary is printed to stdout — paste it directly to
your AI assistant for diagnosis:

```
gitpp pull: 98/101 succeeded, 3 failed

--- freeza (/Users/you/repos/private/freeza) ---
  error: Your local changes to the following files would be overwritten by merge:
    src/main.rs

--- sss (/Users/you/repos/2025/sss) ---
  fatal: refusing to merge unrelated histories
```

## Install

```bash
cargo install gitpp
```

Or build from source:

```bash
git clone https://github.com/kako-jun/gitpp.git
cd gitpp
cargo install --path .
```

## Configuration

Create `gitpp.yaml` in the root of your repos directory:

```yaml
config:
  user.name: your-name
  user.email: your-email@example.com
  pull.rebase: "true"
comments:
  default: sync.
jobs: 20
repos:
  - enabled: true
    remote: git@github.com:user/repo-a.git
    branch: main
    group: "projects"
  - enabled: true
    remote: git@github.com:user/repo-b.git
    branch: main
    group: "projects"
  - enabled: false           # skip this repo
    remote: git@github.com:user/archived.git
    branch: main
    group: "archive"
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `config` | map | no | `git config --local` key-value pairs applied to every repo before and after each operation. Supports any valid git config key (`user.name`, `pull.rebase`, `core.autocrlf`, …). Removing a key from the YAML does **not** unset it from existing repos' `.git/config` — it only overwrites. |
| `comments.default` | string | no* | Commit message used by `push`. **Push is disabled unless this is set to a non-empty string.** Omit the `comments` section entirely if you only use clone/pull. |
| `jobs` | number | no | Max concurrent operations. Default: 20. Overridable with `-j N` on the CLI. |
| `repos[].enabled` | bool | yes | Set `false` to skip a repo without removing it from the file. |
| `repos[].remote` | string | yes | SSH or HTTPS remote URL. Repository name is extracted from the URL automatically (`.git` suffix stripped). |
| `repos[].branch` | string | yes | Branch passed to `git clone -b`. |
| `repos[].group` | string | yes | Subdirectory under the repo root where this repo is cloned (e.g., `group: "2025"` → `./2025/repo-name`). |

### Multiple identities on one machine

Create separate YAML files in separate directories, each with its own `config:` block:

```
~/repos/
  personal/
    gitpp.yaml   # user.email: me@personal.dev
  work/
    gitpp.yaml   # user.email: me@company.com
  hobby/
    gitpp.yaml   # user.email: me@hobbyaccount.io
```

No global `~/.gitconfig` user needed — missing global config is a feature, not a bug.
A repo without local config will refuse to commit, which is the correct fail-safe.

## Usage

```bash
# One-shot mode
gitpp pull              # Pull all enabled repos
gitpp push -j 10        # Push with max 10 parallel jobs
gitpp clone             # Clone (skips already-cloned repos)

# Use a config file from another location
gitpp pull --config ~/shared/gitpp.yaml

# Specify both config and repo root
gitpp clone -c /mnt/ssd/gitpp.yaml -r /mnt/ssd/repos

# Quiet mode (no TUI — summary to stdout, progress to stderr)
gitpp pull -q

# Interactive mode with tab completion
gitpp
gitpp> pull
gitpp> exit
```

### TUI Controls

| Key | Action |
|-----|--------|
| `j` / `k` / `↑` / `↓` | Navigate repos |
| `g` / `G` | Jump to top / bottom |
| `Enter` | Toggle detail pane (shown by default) |
| `h` / `l` / `←` / `→` | Scroll detail pane (3 lines at a time) |
| `Esc` | Close detail pane (or exit browse mode) |
| `q` | Quit immediately |

When all repos finish, gitpp waits 2 seconds. Press any key to enter browse mode and
inspect results; or do nothing and it exits automatically.

### Clone duplicate detection

If the target directory already contains a `.git`:

| Situation | Result |
|-----------|--------|
| No `.git` | Clone normally |
| `.git` present, remote matches | "Already cloned" (Success) — config still applied |
| `.git` present, remote mismatch | "Remote mismatch" (Failed) — shows expected vs actual URL |

## License

MIT
