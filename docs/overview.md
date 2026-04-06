# gitpp Overview

Last updated: 2026-04-06

## What is gitpp?

**gitpp** = git + personal + parallel.
A tool for managing multiple Git repositories in parallel, driven by a single YAML configuration file.
It exposes ten commands — all parallel-capable operations that benefit from running across many repos at once.

Originally a Bash script (`gitp.sh`), rewritten in Rust.
The main improvements are a visual progress display via TUI and distribution as a single binary.

## Intended Use Case

When you maintain repositories in the hundreds across multiple machines (home PC, work, laptop, etc.),
starting work on a new machine without pulling everything first leads to constant conflicts.
The "pull everything first" workflow is solved by one YAML file and a single `gitpp pull`.

## Design Philosophy

### Do One Thing

gitpp provides ten subcommands: clone, pull, push, status, diff, fetch, branch, switch, stash list, and gc.
All share a single criterion for inclusion: they are git operations that benefit from parallel execution across many repositories.
Branch management, merging, rebasing, and opening PRs are out of scope — they require per-repo judgment.
Not "do anything", but "do exactly the things that scale across repos".

### YAML location is the repository root by default

By default, gitpp looks for its config file in the current directory. No fixed paths like `~/.config/`.
Placing separate YAML files in different locations lets you maintain independent repository trees.

The `--config` and `--root` options allow explicit override of the config file path and the repository root.
This is useful when running from cron or scripts where relying on CWD is undesirable.

### Multiple Commit Identities

One machine, multiple identities: personal open source work, job projects, or several hobby accounts.
Defining a user at the system level (`~/.gitconfig`) leads to commits going out under the wrong name —
the git equivalent of posting to the wrong social media account.

gitpp assumes no system-level git config is set. Instead, key-value pairs written in the `config:` section
of the YAML are applied to each repository's `.git/config` via `git config --local`.
Any git config key is supported — not just `user.name` / `user.email`, but also `pull.rebase`,
`core.autocrlf`, and so on.

- No system user defined → forgetting to configure a repo causes commit errors (a fail-safe)
- Different YAML files can carry different configs, so identity naturally follows location
- No "main" and "sub" identities — each location gets its own `gitpp.yaml` on equal footing

Config is not a standalone command; it is applied implicitly on every operation.

### Repository Classification via `group`

The `group` field in the YAML is both the subdirectory name for cloning and an implicit
description of a repository's character.

| group example | Meaning |
|---|---|
| `private` | Private repos; not shared externally |
| `2025`, `2026` | Public repos, organized by year of creation |
| `gitlab` | Repos hosted on GitLab rather than GitHub |

The directory structure alone communicates the nature of each repo to both humans and AI agents.

### Push is Opt-In by Design

Push is a destructive operation (`git add -A` on everything, followed by a fixed commit message),
so it is **disabled unless `comments.default` is explicitly set**.
If you only need clone and pull, the `comments` section can be omitted entirely.

When enabled:
`git add -A` stages all files unconditionally — because there is no opportunity to cherry-pick.
Use `.gitignore` to exclude files proactively. Accidents can be fixed by having an agent rewrite history.

The commit message is the fixed string from `comments.default` (e.g., `"sync."`).
For repos that matter, commit carefully by hand. gitpp push is for bulk-syncing private repos.

### Concurrency Control

Pulling 101 repos simultaneously saturates network bandwidth and the filesystem.
The `jobs` setting (default: 20) caps concurrent operations.
The CLI `-j N` / `--jobs N` flag overrides it at runtime — the same convention as `make -j` and `cargo build -j`.

## Competing Tools

| Tool | Language | Stars | Highlights |
|---|---|---|---|
| gita | Python | ~1.8k | Group management, delegate commands |
| mani | Go | ~640 | YAML + task runner + TUI |
| myrepos (mr) | Perl | Veteran | Multi-VCS support, no dependencies |
| gr | Node.js | - | Tag-based auto-discovery |
| git-xargs | Go | - | Bulk script execution, auto PR creation |

gitpp's differentiator: **Rust + ratatui TUI + single-purpose (parallel git operations only) + concurrency control** — no competing tool combines all four.
