mod git_controller;
mod interactive;
mod setting_util;
mod tui;

use git_controller::GitController;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use tui::{append_repo_output, update_repo_status, RepoStatus, TuiApp};

#[derive(Debug)]
struct GlobalOptions {
    jobs: Option<usize>,
    config_path: Option<PathBuf>,
    root_path: Option<PathBuf>,
    quiet: bool,
    rest: Vec<String>,
}

struct Semaphore {
    state: Mutex<usize>,
    condvar: Condvar,
}

impl Semaphore {
    fn new(permits: usize) -> Self {
        Semaphore {
            state: Mutex::new(permits),
            condvar: Condvar::new(),
        }
    }

    fn acquire(self: &Arc<Self>) -> SemaphoreGuard {
        let mut count = self.state.lock().unwrap_or_else(|e| e.into_inner());
        while *count == 0 {
            count = self.condvar.wait(count).unwrap_or_else(|e| e.into_inner());
        }
        *count -= 1;
        SemaphoreGuard {
            sem: Arc::clone(self),
        }
    }
}

struct SemaphoreGuard {
    sem: Arc<Semaphore>,
}

impl Drop for SemaphoreGuard {
    fn drop(&mut self) {
        let mut count = self.sem.state.lock().unwrap_or_else(|e| e.into_inner());
        *count += 1;
        self.sem.condvar.notify_one();
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();

    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("gitpp {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    if args.iter().any(|a| a == "--help" || a == "-h") {
        show_help();
        return;
    }

    let global_opts = parse_global_options(&args);

    let setting = match setting_util::load(global_opts.config_path.as_deref()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    let base_dir = match &global_opts.root_path {
        Some(p) => p.clone(),
        None => match env::current_dir() {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Error: Cannot get current directory: {e}");
                std::process::exit(1);
            }
        },
    };

    let jobs_override = global_opts.jobs;
    let quiet = global_opts.quiet;

    if global_opts.rest.is_empty() {
        if quiet {
            eprintln!("Error: --quiet cannot be used with interactive mode. Specify a command.");
            std::process::exit(1);
        }
        loop {
            match interactive::run_interactive_mode() {
                Ok(cmd_args) => {
                    if cmd_args.is_empty() {
                        break;
                    }
                    if let Err(e) =
                        execute_command(&setting, &cmd_args, &base_dir, jobs_override, quiet)
                    {
                        eprintln!("Error: {e}");
                    }
                }
                Err(e) => {
                    eprintln!("Interactive mode error: {e:?}");
                    break;
                }
            }
        }
    } else if let Err(e) =
        execute_command(&setting, &global_opts.rest, &base_dir, jobs_override, quiet)
    {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

fn parse_global_options(args: &[String]) -> GlobalOptions {
    let mut jobs: Option<usize> = None;
    let mut config_path: Option<PathBuf> = None;
    let mut root_path: Option<PathBuf> = None;
    let mut quiet = false;
    let mut rest = Vec::new();
    let mut skip_next = false;

    for (i, arg) in args.iter().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }

        if arg == "-q" || arg == "--quiet" {
            quiet = true;
        } else if (arg == "-j" || arg == "--jobs") && i + 1 < args.len() {
            if let Ok(n) = args[i + 1].parse::<usize>() {
                jobs = Some(n.max(1));
            }
            skip_next = true;
        } else if arg.starts_with("-j") && arg.len() > 2 {
            if let Ok(n) = arg[2..].parse::<usize>() {
                jobs = Some(n.max(1));
            }
        } else if arg == "-c" || arg == "--config" {
            if i + 1 >= args.len() {
                eprintln!("Error: {arg} requires a value");
                std::process::exit(1);
            }
            config_path = Some(
                fs::canonicalize(&args[i + 1]).unwrap_or_else(|_| PathBuf::from(&args[i + 1])),
            );
            skip_next = true;
        } else if arg == "-r" || arg == "--root" {
            if i + 1 >= args.len() {
                eprintln!("Error: {arg} requires a value");
                std::process::exit(1);
            }
            root_path = Some(
                fs::canonicalize(&args[i + 1]).unwrap_or_else(|_| PathBuf::from(&args[i + 1])),
            );
            skip_next = true;
        } else {
            rest.push(arg.clone());
        }
    }

    GlobalOptions {
        jobs,
        config_path,
        root_path,
        quiet,
        rest,
    }
}

fn parse_jobs(args: &[String], default: usize) -> (usize, Vec<String>) {
    let mut jobs = default;
    let mut filtered = Vec::new();
    let mut skip_next = false;

    for (i, arg) in args.iter().enumerate() {
        if skip_next {
            skip_next = false;
            continue;
        }

        if (arg == "-j" || arg == "--jobs") && i + 1 < args.len() {
            if let Ok(n) = args[i + 1].parse::<usize>() {
                jobs = n.max(1);
            }
            skip_next = true;
        } else if arg.starts_with("-j") && arg.len() > 2 {
            if let Ok(n) = arg[2..].parse::<usize>() {
                jobs = n.max(1);
            }
        } else {
            filtered.push(arg.clone());
        }
    }

    (jobs, filtered)
}

fn detect_untracked_repos(
    base_dir: &Path,
    all_repos: &[&setting_util::Repos],
) -> Vec<(String, String)> {
    // Build a set of canonical paths for all YAML-defined repos (including disabled ones)
    let mut known_paths: HashSet<PathBuf> = HashSet::new();
    for repo in all_repos {
        let repo_name = extract_repo_name(&repo.remote);
        let repo_path = base_dir.join(&repo.group).join(&repo_name);
        if let Ok(canonical) = fs::canonicalize(&repo_path) {
            known_paths.insert(canonical);
        } else {
            // Repo dir may not exist yet (not cloned); store the logical path
            known_paths.insert(repo_path);
        }
    }

    let base_canonical = fs::canonicalize(base_dir).unwrap_or_else(|_| base_dir.to_path_buf());

    let mut untracked: Vec<(String, String)> = Vec::new();

    let entries = match fs::read_dir(base_dir) {
        Ok(e) => e,
        Err(_) => return untracked,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        // Skip hidden directories (.git, .cache, .github, etc.)
        if path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        {
            continue;
        }

        // Check if this direct child is a git repo
        if path.join(".git").exists() {
            let canonical = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());
            // Skip base_dir itself (shouldn't happen since we're reading children)
            if canonical == base_canonical {
                continue;
            }
            if !known_paths.contains(&canonical) {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let display_name = format!("(root)/{name}");
                untracked.push((display_name, canonical.to_string_lossy().to_string()));
            }
            continue;
        }

        // Otherwise, scan one level deeper (group directories)
        let sub_entries = match fs::read_dir(&path) {
            Ok(e) => e,
            Err(_) => continue,
        };

        for sub_entry in sub_entries.flatten() {
            let sub_path = sub_entry.path();
            if !sub_path.is_dir() {
                continue;
            }
            if sub_path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with('.'))
            {
                continue;
            }
            if !sub_path.join(".git").exists() {
                continue;
            }
            let canonical = fs::canonicalize(&sub_path).unwrap_or_else(|_| sub_path.clone());
            if !known_paths.contains(&canonical) {
                let group_name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let repo_name = sub_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let display_name = format!("{group_name}/{repo_name}");
                untracked.push((display_name, canonical.to_string_lossy().to_string()));
            }
        }
    }

    untracked.sort_by(|a, b| a.0.cmp(&b.0));
    untracked
}

fn execute_command(
    setting: &setting_util::GitppSetting,
    args: &[String],
    base_dir: &Path,
    jobs_override: Option<usize>,
    quiet: bool,
) -> Result<(), String> {
    if args.is_empty() {
        return Ok(());
    }

    let (parsed_jobs, filtered_args) = parse_jobs(args, setting.jobs);
    let jobs = jobs_override.unwrap_or(parsed_jobs);

    if filtered_args.is_empty() {
        return Ok(());
    }

    let command = &filtered_args[0];

    let enabled_repos: Vec<_> = setting.repos.iter().filter(|r| r.enabled).collect();

    if enabled_repos.is_empty() {
        println!("No enabled repositories found in configuration.");
        return Ok(());
    }

    let repo_names: Vec<String> = enabled_repos
        .iter()
        .map(|r| extract_repo_name(&r.remote))
        .collect();

    let repo_paths: Vec<String> = enabled_repos
        .iter()
        .map(|r| {
            base_dir
                .join(&r.group)
                .join(extract_repo_name(&r.remote))
                .to_string_lossy()
                .to_string()
        })
        .collect();

    // Handle 2-word "stash list" and its shortcut "sl"
    let canonical_cmd = if command == "sl"
        || (command == "stash" && filtered_args.get(1).map(|s| s.as_str()) == Some("list"))
    {
        "stash list"
    } else {
        match command.as_str() {
            "clone" | "clo" | "cl" => "clone",
            "pull" | "pul" | "pl" => "pull",
            "push" | "pus" | "ps" => "push",
            "status" | "st" => "status",
            "diff" | "di" => "diff",
            "fetch" | "fe" => "fetch",
            "branch" | "br" => "branch",
            "switch" | "sw" => "switch",
            "gc" => "gc",
            other => other,
        }
    };

    // Detect untracked repos (all YAML-defined repos, including disabled)
    let all_repos: Vec<_> = setting.repos.iter().collect();
    let untracked_repos = detect_untracked_repos(base_dir, &all_repos);

    let mut tui_app = TuiApp::new(repo_names, repo_paths, canonical_cmd);

    // Add untracked repos to TUI
    for (name, path) in &untracked_repos {
        tui_app.add_untracked(name.clone(), path.clone());
    }

    let repos_handle = tui_app.get_repos_handle();
    let semaphore = Arc::new(Semaphore::new(jobs));

    match canonical_cmd {
        "clone" => {
            spawn_clone_workers(setting, &enabled_repos, repos_handle, &semaphore, base_dir);
        }
        "pull" => {
            spawn_pull_workers(setting, &enabled_repos, repos_handle, &semaphore, base_dir);
        }
        "push" => {
            let msg = setting
                .comments
                .get("default")
                .map(|s| s.as_str())
                .unwrap_or("");
            if msg.is_empty() {
                return Err("Push is disabled: comments.default is empty or not set in gitpp.yaml. Set a commit message to enable push.".to_string());
            }
            spawn_push_workers(setting, &enabled_repos, repos_handle, &semaphore, base_dir);
        }
        "status" => {
            spawn_generic_workers(
                &enabled_repos,
                repos_handle,
                &semaphore,
                base_dir,
                |git, dir| git.git_status(dir),
                "Checking status...",
            );
        }
        "diff" => {
            spawn_generic_workers(
                &enabled_repos,
                repos_handle,
                &semaphore,
                base_dir,
                |git, dir| git.git_diff_stat(dir),
                "Diffing...",
            );
        }
        "fetch" => {
            spawn_generic_workers(
                &enabled_repos,
                repos_handle,
                &semaphore,
                base_dir,
                |git, dir| git.git_fetch(dir),
                "Fetching...",
            );
        }
        "branch" => {
            spawn_generic_workers(
                &enabled_repos,
                repos_handle,
                &semaphore,
                base_dir,
                |git, dir| git.git_branch(dir),
                "Checking branch...",
            );
        }
        "switch" => {
            spawn_generic_workers(
                &enabled_repos,
                repos_handle,
                &semaphore,
                base_dir,
                |git, dir| git.git_switch_default(dir),
                "Switching...",
            );
        }
        "stash list" => {
            spawn_generic_workers(
                &enabled_repos,
                repos_handle,
                &semaphore,
                base_dir,
                |git, dir| git.git_stash_list(dir),
                "Checking stash...",
            );
        }
        "gc" => {
            spawn_generic_workers(
                &enabled_repos,
                repos_handle,
                &semaphore,
                base_dir,
                |git, dir| git.git_gc(dir),
                "Running gc...",
            );
        }
        "help" | "?" => {
            show_help();
            return Ok(());
        }
        "stash" => {
            return Err("Unknown command: stash. Did you mean 'stash list' (or 'sl')?".to_string());
        }
        _ => {
            return Err(format!("Unknown command: {command}"));
        }
    }

    if quiet {
        let interrupted = Arc::new(AtomicBool::new(false));
        let interrupted_clone = Arc::clone(&interrupted);
        ctrlc::set_handler(move || {
            interrupted_clone.store(true, Ordering::Relaxed);
        })
        .ok();
        if let Err(e) = tui_app.run_quiet(interrupted) {
            return Err(format!("Quiet mode error: {e:?}"));
        }
    } else if let Err(e) = tui_app.run() {
        return Err(format!("TUI error: {e:?}"));
    }

    Ok(())
}

fn show_help() {
    println!("\n\x1b[1;36mgitpp\x1b[0m - Git Personal Parallel Manager\n");
    println!("\x1b[1;36mUsage:\x1b[0m");
    println!("  gitpp [global options] <command> [options]");
    println!("  gitpp                      Start interactive mode\n");
    println!("\x1b[1;36mCommands:\x1b[0m");
    println!("  \x1b[1;33mclone\x1b[0m                Clone all enabled repositories");
    println!("  \x1b[1;33mpull\x1b[0m                 Pull all enabled repositories");
    println!("  \x1b[1;33mpush\x1b[0m                 Push all enabled repositories");
    println!(
        "  \x1b[1;33mstatus\x1b[0m               Show uncommitted changes (git status --porcelain)"
    );
    println!("  \x1b[1;33mdiff\x1b[0m                 Show diff summary (staged + unstaged)");
    println!("  \x1b[1;33mfetch\x1b[0m                Fetch from remote");
    println!("  \x1b[1;33mbranch\x1b[0m               Show current branch");
    println!(
        "  \x1b[1;33mswitch\x1b[0m               Switch to default branch (requires Git 2.23+)"
    );
    println!("  \x1b[1;33mstash list\x1b[0m           List stashed changes");
    println!("  \x1b[1;33mgc\x1b[0m                   Run garbage collection (I/O heavy, use -j to limit)");
    println!("  \x1b[1;33mhelp\x1b[0m                 Show this help message\n");
    println!("\x1b[1;36mGlobal Options:\x1b[0m");
    println!(
        "  \x1b[1;33m-c PATH\x1b[0m, \x1b[1;33m--config PATH\x1b[0m  Config file path (default: ./gitpp.yaml or ./gitpp.yml)"
    );
    println!(
        "  \x1b[1;33m-r PATH\x1b[0m, \x1b[1;33m--root PATH\x1b[0m    Repository root directory (default: current directory)"
    );
    println!(
        "  \x1b[1;33m-j N\x1b[0m, \x1b[1;33m--jobs N\x1b[0m           Max parallel jobs (default: from gitpp.yaml, or 20)"
    );
    println!(
        "  \x1b[1;33m-q\x1b[0m, \x1b[1;33m--quiet\x1b[0m              No TUI; progress on stderr, summary on stdout"
    );
    println!("  \x1b[1;33m-V\x1b[0m, \x1b[1;33m--version\x1b[0m            Show version\n");
    println!("\x1b[1;36mShortcuts:\x1b[0m");
    println!("  clo, cl  → clone      st → status");
    println!("  pul, pl  → pull       di → diff");
    println!("  pus, ps  → push       fe → fetch");
    println!("  br → branch           sw → switch");
    println!("  sl → stash list       gc → gc\n");
}

fn spawn_clone_workers(
    setting: &setting_util::GitppSetting,
    repos: &[&setting_util::Repos],
    repos_handle: Arc<Mutex<Vec<tui::RepoProgress>>>,
    semaphore: &Arc<Semaphore>,
    base_dir: &Path,
) {
    for repo in repos {
        let repo_data = (*repo).clone();
        let config = setting.config.clone();
        let repos_handle = Arc::clone(&repos_handle);
        let repo_name = extract_repo_name(&repo.remote);
        let sem = Arc::clone(semaphore);
        let base = base_dir.to_path_buf();

        thread::spawn(move || {
            let _guard = sem.acquire();

            update_repo_status(
                &repos_handle,
                &repo_name,
                RepoStatus::Running,
                "Starting...",
                10,
            );

            let git = GitController::new();
            let group_dir = base.join(&repo_data.group);

            update_repo_status(
                &repos_handle,
                &repo_name,
                RepoStatus::Running,
                "Creating directory...",
                20,
            );
            if let Err(e) = fs::create_dir_all(&group_dir) {
                update_repo_status(
                    &repos_handle,
                    &repo_name,
                    RepoStatus::Failed,
                    &format!("Error: {e}"),
                    100,
                );
                return;
            }

            let repo_dir = group_dir.join(&repo_name);
            if repo_dir.join(".git").exists() {
                if git.is_valid_repo(&repo_dir) {
                    let actual_remote = git.git_remote_url(&repo_dir);
                    if actual_remote == repo_data.remote {
                        update_repo_status(
                            &repos_handle,
                            &repo_name,
                            RepoStatus::Unchanged,
                            "Already cloned",
                            100,
                        );
                        git.git_config(&repo_dir, &config);
                        return;
                    }
                    // Directory exists but remote doesn't match
                    update_repo_status(
                        &repos_handle,
                        &repo_name,
                        RepoStatus::Failed,
                        &format!(
                            "Remote mismatch: expected {}, found {}",
                            repo_data.remote, actual_remote
                        ),
                        100,
                    );
                    return;
                }
                // Incomplete clone detected — remove and re-clone
                if let Err(e) = std::fs::remove_dir_all(&repo_dir) {
                    update_repo_status(
                        &repos_handle,
                        &repo_name,
                        RepoStatus::Failed,
                        &format!("Failed to remove incomplete clone: {e}"),
                        100,
                    );
                    return;
                }
            }

            update_repo_status(
                &repos_handle,
                &repo_name,
                RepoStatus::Running,
                "Cloning...",
                40,
            );
            let result = git.git_clone(&group_dir, &repo_data.remote, &repo_data.branch);
            append_repo_output(&repos_handle, &repo_name, &result.output);

            update_repo_status(
                &repos_handle,
                &repo_name,
                RepoStatus::Running,
                "Configuring...",
                80,
            );

            if repo_dir.exists() {
                git.git_config(&repo_dir, &config);
            }

            if result.success {
                update_repo_status(
                    &repos_handle,
                    &repo_name,
                    RepoStatus::Updated,
                    "Cloned",
                    100,
                );
            } else {
                update_repo_status(&repos_handle, &repo_name, RepoStatus::Failed, "Failed", 100);
            }
        });
    }
}

fn spawn_pull_workers(
    setting: &setting_util::GitppSetting,
    repos: &[&setting_util::Repos],
    repos_handle: Arc<Mutex<Vec<tui::RepoProgress>>>,
    semaphore: &Arc<Semaphore>,
    base_dir: &Path,
) {
    for repo in repos {
        let repo_data = (*repo).clone();
        let config = setting.config.clone();
        let repos_handle = Arc::clone(&repos_handle);
        let repo_name = extract_repo_name(&repo.remote);
        let sem = Arc::clone(semaphore);
        let base = base_dir.to_path_buf();

        thread::spawn(move || {
            let _guard = sem.acquire();

            update_repo_status(
                &repos_handle,
                &repo_name,
                RepoStatus::Running,
                "Starting...",
                10,
            );

            let git = GitController::new();
            let repo_dir = base.join(&repo_data.group).join(&repo_name);

            if !check_repo_ready(&git, &repo_dir, &repos_handle, &repo_name) {
                return;
            }

            update_repo_status(
                &repos_handle,
                &repo_name,
                RepoStatus::Running,
                "Configuring...",
                30,
            );
            git.git_config(&repo_dir, &config);

            update_repo_status(
                &repos_handle,
                &repo_name,
                RepoStatus::Running,
                "Pulling...",
                50,
            );
            let result = git.git_pull(&repo_dir);
            append_repo_output(&repos_handle, &repo_name, &result.output);

            if result.success {
                if result.had_changes {
                    update_repo_status(
                        &repos_handle,
                        &repo_name,
                        RepoStatus::Updated,
                        "Updated",
                        100,
                    );
                } else {
                    update_repo_status(
                        &repos_handle,
                        &repo_name,
                        RepoStatus::Unchanged,
                        "Unchanged",
                        100,
                    );
                }
            } else {
                update_repo_status(&repos_handle, &repo_name, RepoStatus::Failed, "Failed", 100);
            }
        });
    }
}

fn spawn_push_workers(
    setting: &setting_util::GitppSetting,
    repos: &[&setting_util::Repos],
    repos_handle: Arc<Mutex<Vec<tui::RepoProgress>>>,
    semaphore: &Arc<Semaphore>,
    base_dir: &Path,
) {
    let commit_message = setting
        .comments
        .get("default")
        .expect("comments.default verified in execute_command")
        .clone();

    for repo in repos {
        let repo_data = (*repo).clone();
        let config = setting.config.clone();
        let repos_handle = Arc::clone(&repos_handle);
        let repo_name = extract_repo_name(&repo.remote);
        let commit_msg = commit_message.clone();
        let sem = Arc::clone(semaphore);
        let base = base_dir.to_path_buf();

        thread::spawn(move || {
            let _guard = sem.acquire();

            update_repo_status(
                &repos_handle,
                &repo_name,
                RepoStatus::Running,
                "Starting...",
                10,
            );

            let git = GitController::new();
            let repo_dir = base.join(&repo_data.group).join(&repo_name);

            if !check_repo_ready(&git, &repo_dir, &repos_handle, &repo_name) {
                return;
            }

            update_repo_status(
                &repos_handle,
                &repo_name,
                RepoStatus::Running,
                "Configuring...",
                20,
            );
            git.git_config(&repo_dir, &config);

            update_repo_status(
                &repos_handle,
                &repo_name,
                RepoStatus::Running,
                "Pushing...",
                50,
            );

            let result = git.git_push(&repo_dir, &commit_msg);
            append_repo_output(&repos_handle, &repo_name, &result.output);

            if result.success {
                if result.had_changes {
                    update_repo_status(
                        &repos_handle,
                        &repo_name,
                        RepoStatus::Updated,
                        "Updated",
                        100,
                    );
                } else {
                    update_repo_status(
                        &repos_handle,
                        &repo_name,
                        RepoStatus::Unchanged,
                        "Unchanged",
                        100,
                    );
                }
            } else {
                update_repo_status(&repos_handle, &repo_name, RepoStatus::Failed, "Failed", 100);
            }
        });
    }
}

fn spawn_generic_workers(
    repos: &[&setting_util::Repos],
    repos_handle: Arc<Mutex<Vec<tui::RepoProgress>>>,
    semaphore: &Arc<Semaphore>,
    base_dir: &Path,
    operation: fn(&GitController, &Path) -> git_controller::GitResult,
    running_message: &str,
) {
    for repo in repos {
        let repo_data = (*repo).clone();
        let repos_handle = Arc::clone(&repos_handle);
        let repo_name = extract_repo_name(&repo.remote);
        let sem = Arc::clone(semaphore);
        let base = base_dir.to_path_buf();
        let msg = running_message.to_string();

        thread::spawn(move || {
            let _guard = sem.acquire();

            update_repo_status(
                &repos_handle,
                &repo_name,
                RepoStatus::Running,
                "Starting...",
                10,
            );

            let git = GitController::new();
            let repo_dir = base.join(&repo_data.group).join(&repo_name);

            if !check_repo_ready(&git, &repo_dir, &repos_handle, &repo_name) {
                return;
            }

            update_repo_status(&repos_handle, &repo_name, RepoStatus::Running, &msg, 50);
            let result = operation(&git, &repo_dir);
            append_repo_output(&repos_handle, &repo_name, &result.output);

            if result.success {
                if result.had_changes {
                    update_repo_status(
                        &repos_handle,
                        &repo_name,
                        RepoStatus::Updated,
                        "Updated",
                        100,
                    );
                } else {
                    update_repo_status(
                        &repos_handle,
                        &repo_name,
                        RepoStatus::Unchanged,
                        "Unchanged",
                        100,
                    );
                }
            } else {
                update_repo_status(&repos_handle, &repo_name, RepoStatus::Failed, "Failed", 100);
            }
        });
    }
}

/// Check that a repo directory exists and is a valid git repository.
/// Returns `true` if ready, or `false` after reporting the error via `update_repo_status`.
fn check_repo_ready(
    git: &GitController,
    repo_dir: &Path,
    repos_handle: &Arc<Mutex<Vec<tui::RepoProgress>>>,
    repo_name: &str,
) -> bool {
    if !repo_dir.exists() {
        update_repo_status(
            repos_handle,
            repo_name,
            RepoStatus::Failed,
            &format!("Directory not found: {}", repo_dir.display()),
            100,
        );
        return false;
    }

    if !git.is_valid_repo(repo_dir) {
        update_repo_status(
            repos_handle,
            repo_name,
            RepoStatus::Failed,
            "Incomplete clone. Run `gitpp clone` to fix",
            100,
        );
        return false;
    }

    true
}

fn extract_repo_name(remote_url: &str) -> String {
    let url = remote_url.trim_end_matches('/');
    let parts: Vec<&str> = url.split('/').collect();
    let last_part = parts.last().unwrap_or(&"");
    last_part.trim_end_matches(".git").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn make_repo(repos: &[(&str, &str, &str)]) -> Vec<setting_util::Repos> {
        repos
            .iter()
            .map(|(remote, branch, group)| setting_util::Repos {
                enabled: true,
                remote: remote.to_string(),
                branch: branch.to_string(),
                group: group.to_string(),
            })
            .collect()
    }

    fn init_git_dir(path: &Path) {
        fs::create_dir_all(path.join(".git")).unwrap();
    }

    #[test]
    fn detect_group_level_untracked() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        // YAML-defined repo
        let known = base.join("mygroup").join("known-repo");
        init_git_dir(&known);

        // Untracked repo in same group
        let unknown = base.join("mygroup").join("stray-repo");
        init_git_dir(&unknown);

        let yaml_repos = make_repo(&[("git@github.com:user/known-repo.git", "main", "mygroup")]);
        let refs: Vec<_> = yaml_repos.iter().collect();
        let result = detect_untracked_repos(base, &refs);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "mygroup/stray-repo");
    }

    #[test]
    fn detect_root_level_untracked() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        // Repo directly under base_dir (no group)
        let root_repo = base.join("orphan");
        init_git_dir(&root_repo);

        let yaml_repos: Vec<setting_util::Repos> = vec![];
        let refs: Vec<_> = yaml_repos.iter().collect();
        let result = detect_untracked_repos(base, &refs);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, "(root)/orphan");
    }

    #[test]
    fn known_repos_not_detected() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let repo = base.join("grp").join("myrepo");
        init_git_dir(&repo);

        let yaml_repos = make_repo(&[("git@github.com:user/myrepo.git", "main", "grp")]);
        let refs: Vec<_> = yaml_repos.iter().collect();
        let result = detect_untracked_repos(base, &refs);

        assert!(result.is_empty());
    }

    #[test]
    fn disabled_repos_not_detected_as_untracked() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        let repo = base.join("grp").join("disabled-repo");
        init_git_dir(&repo);

        let mut yaml_repos = make_repo(&[("git@github.com:user/disabled-repo.git", "main", "grp")]);
        yaml_repos[0].enabled = false;
        let refs: Vec<_> = yaml_repos.iter().collect();
        let result = detect_untracked_repos(base, &refs);

        assert!(result.is_empty());
    }

    #[test]
    fn hidden_directories_skipped() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        // Hidden dir at root level with .git
        init_git_dir(&base.join(".hidden-repo"));

        // Hidden dir inside group
        let grp = base.join("grp");
        init_git_dir(&grp.join(".secret"));

        let yaml_repos: Vec<setting_util::Repos> = vec![];
        let refs: Vec<_> = yaml_repos.iter().collect();
        let result = detect_untracked_repos(base, &refs);

        assert!(result.is_empty());
    }

    #[test]
    fn dirs_without_git_ignored() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        // Directory without .git (not a repo)
        fs::create_dir_all(base.join("grp").join("just-a-folder")).unwrap();
        // File (not a directory)
        fs::write(base.join("grp").join("readme.txt"), "hi").unwrap();

        let yaml_repos: Vec<setting_util::Repos> = vec![];
        let refs: Vec<_> = yaml_repos.iter().collect();
        let result = detect_untracked_repos(base, &refs);

        assert!(result.is_empty());
    }

    #[test]
    fn results_sorted_alphabetically() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path();

        init_git_dir(&base.join("z-group").join("repo-z"));
        init_git_dir(&base.join("a-group").join("repo-a"));
        init_git_dir(&base.join("alpha"));

        let yaml_repos: Vec<setting_util::Repos> = vec![];
        let refs: Vec<_> = yaml_repos.iter().collect();
        let result = detect_untracked_repos(base, &refs);

        assert_eq!(result.len(), 3);
        assert_eq!(result[0].0, "(root)/alpha");
        assert_eq!(result[1].0, "a-group/repo-a");
        assert_eq!(result[2].0, "z-group/repo-z");
    }
}
