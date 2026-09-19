# gitpp Specification

Last updated: 2026-06-05

## Commands

| Command | Aliases | Behavior |
|---|---|---|
| `clone` | `clo`, `cl` | Clone all enabled repositories in parallel |
| `pull` | `pul`, `pl` | Pull all enabled repositories in parallel |
| `push` | `pus`, `ps` | Add → commit → push all enabled repositories in parallel |
| `status` | `st` | Show uncommitted changes (`git status --porcelain`) |
| `diff` | `di` | Show diff summary, staged + unstaged (`git diff --stat HEAD`) |
| `fetch` | `fe` | Fetch from remote (`git fetch`) |
| `branch` | `br` | Show current branch (`git rev-parse --abbrev-ref HEAD`) |
| `switch` | `sw` | Switch to default branch (requires Git 2.23+) |
| `stash list` | `sl` | List stashed changes (`git stash list`) |
| `gc` | — | Run garbage collection (`git gc`). I/O heavy — use `-j` to limit parallelism |
| `help` | `?` | Print command list (no TUI) |

### Options

| Option | Description |
|---|---|
| `-c PATH` / `--config PATH` | Path to config file (default: `gitpp.yaml` / `gitpp.yml` in the current directory) |
| `-r PATH` / `--root PATH` | Root directory for repository checkout (default: current directory) |
| `-j N` / `--jobs N` | Concurrency limit (default: `jobs` value in `gitpp.yaml`, or 20 if unset) |
| `-q` / `--quiet` | No-TUI mode. Summary to stdout, progress to stderr. Intended for scripts and CI. |

`-c`, `-r`, `-j`, and `-q` are global options and may be placed before or after the subcommand.

### Git Operations per Command

| Command | Git commands executed | Notes |
|---|---|---|
| clone | `git clone <remote> -b <branch> --recurse-submodules` | Run inside the group subdirectory; submodules are fetched too |
| pull | `git fetch --prune` → `git merge --ff-only @{u}` → `git submodule update --init --recursive` | See "Robust Pull" below. Only a failed fetch is reported as Failed |
| push | `git add -A` → `git commit -m "<msg>"` → `git push` | Commit message is fixed to `comments.default` |
| status | `git status --porcelain` | Read-only; no config applied |
| diff | `git diff --stat HEAD` | Read-only; staged + unstaged; no config applied |
| fetch | `git fetch` | Read-only; no config applied |
| branch | `git rev-parse --abbrev-ref HEAD` | Read-only; no config applied |
| switch | `git rev-parse --verify refs/heads/main` → `git switch main` (or master) | Requires Git 2.23+; detects default branch via local refs only |
| stash list | `git stash list` | Read-only; no config applied |
| gc | `git gc` | No config applied |

### Robust Pull

Pull is designed never to abort a batch run on recoverable repository states. It runs as a sequence:

1. `git fetch --prune` updates remote-tracking refs and removes stale ones.
   A failure here (network or authentication error) is the **only** path that marks pull as Failed.
2. The current branch (`git rev-parse --abbrev-ref HEAD`) and its upstream
   (`git rev-parse --abbrev-ref --symbolic-full-name @{u}`) are inspected.
3. Merge step (always keeps pull `success: true`):
   - **Detached HEAD or no upstream** → merge is skipped; the output notes "fetched only (no upstream)".
   - **Upstream present** → `git merge --ff-only @{u}` is attempted.
     - Success → reported as Updated when the tree actually moved (not "Already up to date").
     - Failure (diverged / non-fast-forward, or local changes blocking the update) → **not** an error.
       The working tree is never reset and local changes are never discarded; the reason is reported and
       the repo is marked **Blocked** — distinct from Unchanged, so it can be told apart from a repo that
       truly had nothing new to pull.
4. `git submodule update --init --recursive` synchronizes submodules.
   A failure here is recorded as a warning in the output but never changes the pull result.

As a result, only a failed fetch produces Failed. Detached HEAD and no-upstream branches (nothing to merge)
complete as Unchanged. A diverged branch or a dirty working tree that blocks the fast-forward completes as
Blocked, with an explanatory message in the output, so a batch pull never collapses into "lots of errors"
while still flagging the repos that need manual follow-up (commit/stash locally, or resolve the divergence).

### Push Opt-In Design

Push is a destructive operation — it runs `git add -A` and commits with a fixed message across all repos —
and therefore requires explicit opt-in.
If `comments.default` is not set or is an empty string, the push command aborts with an error before running `git add -A`.
To enable push, set `comments.default` in `gitpp.yaml`:

```yaml
comments:
  default: update.
```

Omitting the `comments` section entirely also disables push. Clone and pull are unaffected.

Before every operation, the git config key-value pairs defined in the YAML `config:` section
are applied to each repository's `.git/config` via `git config --local`.
Removing a key from the YAML does not remove it from existing repositories (overwrite only, no deletion).

### Duplicate Detection for Clone

If the target directory already contains a `.git` folder, gitpp fetches the actual remote URL via
`git remote get-url origin` and compares it against the remote specified in the YAML.

| Situation | Result |
|---|---|
| No `.git` present | Proceed with normal clone |
| `.git` exists, valid repo, remote matches | Display "Already cloned" (Unchanged). Apply config only. |
| `.git` exists, valid repo, remote mismatch | Display "Remote mismatch" (Failed). Print expected vs actual remote. |
| `.git` exists, invalid repo (incomplete clone) | Remove directory and re-clone |

A repo is considered valid when `git rev-parse HEAD` succeeds (e.g., at least one commit exists).
Note: an empty repository with zero commits will be treated as invalid and re-cloned.
For non-clone commands (pull, push, etc.), an invalid repo results in "Incomplete clone. Run `gitpp clone` to fix" (Failed).

## Operating Modes

| Mode | How to start | Description |
|---|---|---|
| One-shot | `gitpp <command>` | Execute the command once and exit |
| Interactive | `gitpp` (no arguments) | REPL — run commands repeatedly |

### Interactive Mode

- Prompt: `gitpp> ` (cyan, bold)
- Tab completion: `clone`, `pull`, `push`, `status`, `diff`, `fetch`, `branch`, `switch`, `stash list`, `gc`, `help`, `exit`, `quit`
- Hint: inline suggestion while typing (prefix match)
- History: saved to `~/.gitpp_history`
- Exit: `exit` / `quit` / Ctrl+D

## Configuration File

### Resolution Logic

1. If `--config` is given, use that path.
2. Otherwise, look for `gitpp.yaml` in the current directory.
3. If not found, look for `gitpp.yml`.
4. If neither exists, exit with an error.

### Format

```yaml
config:
  <git-config-key>: <string>  # Any key applied via git config --local
comments:
  default: <string>           # Commit message used for push
jobs: <number>                # Max concurrency (default: 20)
repos:
  - enabled: <bool>           # Excluded from all operations when false
    remote: <string>          # Git remote URL
    branch: <string>          # Branch passed to -b on clone
    group: <string>           # Subdirectory name under the root
```

### Field Reference

| Field | Type | Required | Description |
|---|---|---|---|
| `config` | HashMap | no | Arbitrary key-value pairs applied via `git config --local` (e.g., `user.name`, `pull.rebase`) |
| `comments.default` | String | yes* | Fixed commit message used by push. *Required to enable push. |
| `jobs` | usize | no | Max concurrent operations. Overridable via `-j`. Default: 20. |
| `repos[].enabled` | bool | yes | When false, the repository is excluded from all operations. |
| `repos[].remote` | String | yes | Remote URL (SSH or HTTPS). |
| `repos[].branch` | String | yes | Branch passed to `-b` on clone. |
| `repos[].group` | String | yes | Repository is cloned into `{group}/{repo_name}`. |

The repository name is derived automatically from the trailing path segment of the remote URL (`.git` suffix stripped).

## TUI

A fullscreen TUI built with ratatui and crossterm.
gitpp does not enable terminal mouse capture.
On exit, it drains pending terminal events before returning control to the shell,
reducing the chance of stray input bytes appearing at the prompt.

### Layout

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
│ Total: 103 | Done: 52 | OK: 48 | Fail: 2 | Untracked: 2    │
└──────────────────────────────────────────────────────────────┘
```

**Detail mode (shown by default, toggled with Enter):**

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
│ Total: 103 | Done: 52 | OK: 48 | Fail: 2 | Untracked: 2    │
└──────────────────────────────────────────────────────────────┘
```

### Key Bindings

| Key | Action |
|---|---|
| j / k / ↑ / ↓ | Move selection / scroll list |
| g | Jump to first item |
| G | Jump to last item |
| Enter | Toggle detail pane (shown by default) |
| h / l / ← / → | Scroll detail pane vertically (3 lines at a time) |
| n | Jump to next Failed item (wraps around) |
| N | Jump to previous Failed item (wraps around) |
| y | Copy detail pane content to clipboard |
| Esc | Close detail pane (or exit browse mode when pane is already closed) |
| q | Force quit |

### Behavior After Completion

When all repositories have finished processing:
1. Wait for a keypress for **3 seconds**.
2. No input → exit automatically and print a summary to stdout.
3. Any key pressed → enter browse mode (navigate results with j/k; quit with q or Esc).

Pressing `q` at any time during execution exits immediately.

### Status Transitions

| Status | Icon | Color | Meaning |
|---|---|---|---|
| Waiting | ⏸ | DarkGray | Waiting, including semaphore queue |
| Running | ▶ | Yellow | In progress |
| Updated | ✓ | Green | Completed with changes |
| Unchanged | ─ | DarkGray | Completed with no changes |
| Blocked | ⚠ | Orange | pull only: ff-only merge skipped (diverged branch or dirty working tree) |
| Failed | ✗ | Red | Encountered an error |

### Error Detection

Exit code of the git subprocess determines the result. Any non-zero exit code → Failed.
Pull is an exception: only a failed `git fetch --prune` marks it Failed (see "Robust Pull").
Diverged, detached, no-upstream, and dirty repositories complete with `success: true`.

`GitResult` includes a `had_changes` field that distinguishes Updated from Unchanged:
- **pull**: `had_changes` is true when a fast-forward actually moved the branch, or when the submodule update checked something out. A no-op fast-forward ("Already up to date") and an empty submodule update yield Unchanged. A skipped merge due to no upstream or a detached HEAD also yields Unchanged (nothing to merge). A skipped merge due to divergence or a dirty working tree yields **Blocked** instead (see `blocked` below), never Unchanged.
- **clone**: `had_changes` is true when the clone succeeds (already-cloned repos are detected before calling git). Submodules are fetched via `--recurse-submodules`.
- **push**: `had_changes` is true when `git commit` succeeds (i.e. there was something to commit and push)
- **status**: `had_changes` is true when there are uncommitted changes (non-empty output)
- **diff**: `had_changes` is true when there are staged or unstaged differences (non-empty output)
- **fetch**: `had_changes` is true when new data was fetched from remote (non-empty output)
- **branch**: `had_changes` is true when the current branch is not main or master (highlights non-default branches)
- **switch**: `had_changes` is true when the branch was actually changed; false if already on main/master
- **stash list**: `had_changes` is true when stash entries exist
- **gc**: `had_changes` is always false

`GitResult` also includes a `blocked` field, which is true only for **pull**, and only when the
ff-only merge was skipped because the branch diverged or the working tree had uncommitted local
changes (see "Robust Pull"). Every other pull outcome, and every other command, always reports
`blocked: false`. This field drives the `Blocked` status shown in the TUI and summaries.

For push, the steps run in sequence: add → commit → push. A failure at any step skips the remaining steps.
If `git commit` exits with a non-zero code due to "nothing to commit", it is treated as a success
(`had_changes: false`) and push is skipped.

If `git commit` fails for other reasons (e.g. a pre-commit hook that modifies files), gitpp retries
once: re-add all changes and commit again. This handles the common case where a formatter hook
modifies files during the first commit attempt. If the retry also fails, the repository is marked Failed.

### Summary Output After TUI Exit

After the TUI closes, a plain-text summary (no ANSI escape codes) is printed to stdout.
The format is suitable for pasting directly into a chat with an AI agent.

**All succeeded:**
```
Total: 101 | Done: 101 (Updated: 3 / Unchanged: 98 / Blocked: 0 / Failed: 0 / Untracked: 0)
```

**With failures:**
```
Total: 101 | Done: 101 (Updated: 95 / Unchanged: 3 / Blocked: 0 / Failed: 3 / Untracked: 0)

--- freeza (/Users/kako-jun/repos/private/freeza) ---
  error: Your local changes to the following files would be overwritten by merge:
    src/main.rs

--- sss (/Users/kako-jun/repos/2025/sss) ---
  fatal: refusing to merge unrelated histories
```

For each failed repository, the name, full path, and git output are shown.
For push failures, the combined output of all steps (add, commit, push) is included.

**pull, with a blocked repo:**
```
Total: 155 | Done: 155 (Updated: 12 / Unchanged: 141 / Blocked: 2 / Failed: 0 / Untracked: 0)
```
A `Blocked` count greater than zero means at least one repository has upstream commits that
could not be fast-forwarded (diverged branch or uncommitted local changes). Nothing was
discarded — the repo needs a manual look (`git status` / `git pull` inside it). Blocked repos
are also reported individually as they finish, both in the TUI and in `--quiet` mode
(`[pull] {name}... blocked`), so they can be identified without waiting for the summary.

### Untracked Repository Detection

Before any command runs, gitpp scans `base_dir` for git repositories not listed in the YAML config.

**Scan scope (2 levels):**
1. Direct children of `base_dir` that contain `.git` — displayed as `(root)/name`
2. Grandchildren of `base_dir` (i.e. `base_dir/group/repo`) that contain `.git` — displayed as `group/name`

**Rules:**
- Hidden directories (names starting with `.`) are skipped at both levels
- Directories without `.git` are ignored (not git repos, outside gitpp's scope)
- Disabled repos (`enabled: false`) are still considered YAML-defined (not Untracked)
- Paths are canonicalized before comparison to handle symlinks
- No git operations are performed on Untracked repos — they are display-only
- Untracked repos appear with `?` icon in magenta, status "Not in gitpp.yaml", progress 100%

## Parallel Execution

- One thread per repository (`std::thread`)
- A semaphore (`Mutex<usize>` + `Condvar`) limits concurrent operations
- Default concurrency: `jobs` setting (20 if not specified)
- Override at runtime with `-j N`
- Shared state: `Arc<Mutex<Vec<RepoProgress>>>`
- TUI polls for updates every 100ms

## OS Support

| OS | Encoding | Notes |
|---|---|---|
| Windows | Shift_JIS | Used for decoding git output |
| Linux / macOS | UTF-8 | |

## Technology Stack

| Crate | Version | Purpose |
|---|---|---|
| ratatui | 0.28 | TUI framework |
| crossterm | 0.28 | Terminal control |
| rustyline | 14.0 | Interactive mode (REPL) |
| serde + serde_yaml | 1.0 / 0.9 | YAML config parsing |
| encoding_rs | 0.8 | Per-OS character encoding conversion |
| dirs | 5.0 | Home directory resolution |

Rust edition 2021.
