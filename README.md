# gitpp

Git Personal Parallel Manager — manage 100+ Git repos with one command.

## What it does

Put a `gitpp.yaml` in your repos directory, then:

```bash
gitpp clone   # Clone all repos in parallel
gitpp pull    # Pull all repos in parallel
gitpp push    # Add, commit, push all repos in parallel
```

A full-screen TUI shows real-time progress for every repository.

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
user:
  name: your-name
  email: your-email@example.com
comments:
  default: update.
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
```

- **`user`** — Applied to each repo's `.git/config` automatically (no global gitconfig needed)
- **`jobs`** — Max parallel operations (default: 20)
- **`group`** — Subdirectory for clone (e.g., `projects/repo-a`)
- **`enabled`** — Set `false` to skip a repo

## Usage

```bash
# One-shot mode
gitpp pull              # Pull all enabled repos
gitpp push -j 10        # Push with max 10 parallel jobs
gitpp clone             # Clone (skips already-cloned repos)

# Interactive mode
gitpp                   # Enter REPL with tab completion
gitpp> pull
gitpp> exit
```

### TUI Controls

| Key | Action |
|-----|--------|
| j/k | Navigate repos |
| Enter | Toggle detail pane (shows git output) |
| h/l | Scroll detail pane |
| g/G | Jump to top/bottom |
| q | Quit |

After completion, a plain-text summary is printed to stdout — paste it directly to your AI assistant for diagnosis.

## Why gitpp?

When you work across multiple machines with 100+ repos, you need to pull everything before starting work. `gitpp pull` does that in one command with visual feedback.

Each `gitpp.yaml` defines its own `user.name`/`user.email`, so you can keep personal and work repos on the same machine without commit author mixups.

## License

MIT
