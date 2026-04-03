mod git_controller;
mod interactive;
mod setting_util;
mod tui;

use git_controller::GitController;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
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

    let canonical_cmd = match command.as_str() {
        "clone" | "clo" | "cl" => "clone",
        "pull" | "pul" | "pu" => "pull",
        "push" | "pus" | "ps" => "push",
        other => other,
    };

    let mut tui_app = TuiApp::new(repo_names, repo_paths, canonical_cmd);
    let repos_handle = tui_app.get_repos_handle();
    let semaphore = Arc::new(Semaphore::new(jobs));

    match command.as_str() {
        "clone" | "clo" | "cl" => {
            spawn_clone_workers(setting, &enabled_repos, repos_handle, &semaphore, base_dir);
        }
        "pull" | "pul" | "pu" => {
            spawn_pull_workers(setting, &enabled_repos, repos_handle, &semaphore, base_dir);
        }
        "push" | "pus" | "ps" => {
            spawn_push_workers(setting, &enabled_repos, repos_handle, &semaphore, base_dir);
        }
        "help" | "?" => {
            show_help();
            return Ok(());
        }
        _ => {
            return Err(format!("Unknown command: {command}"));
        }
    }

    if quiet {
        if let Err(e) = tui_app.run_quiet() {
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
        "  \x1b[1;33m-q\x1b[0m, \x1b[1;33m--quiet\x1b[0m              No TUI; progress on stderr, summary on stdout\n"
    );
    println!("\x1b[1;36mShortcuts:\x1b[0m");
    println!("  clo, cl  → clone");
    println!("  pul, pu  → pull");
    println!("  pus, ps  → push\n");
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
        let user_name = setting.user.name.clone();
        let user_email = setting.user.email.clone();
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
                let actual_remote = git.git_remote_url(&repo_dir);
                if actual_remote == repo_data.remote {
                    update_repo_status(
                        &repos_handle,
                        &repo_name,
                        RepoStatus::Success,
                        "Already cloned",
                        100,
                    );
                    git.git_config(&repo_dir, &user_name, &user_email);
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
                git.git_config(&repo_dir, &user_name, &user_email);
            }

            if result.success {
                update_repo_status(&repos_handle, &repo_name, RepoStatus::Success, "Done", 100);
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
        let user_name = setting.user.name.clone();
        let user_email = setting.user.email.clone();
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

            if !repo_dir.exists() {
                update_repo_status(
                    &repos_handle,
                    &repo_name,
                    RepoStatus::Failed,
                    &format!("Directory not found: {}", repo_dir.display()),
                    100,
                );
                return;
            }

            update_repo_status(
                &repos_handle,
                &repo_name,
                RepoStatus::Running,
                "Configuring...",
                30,
            );
            git.git_config(&repo_dir, &user_name, &user_email);

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
                update_repo_status(&repos_handle, &repo_name, RepoStatus::Success, "Done", 100);
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
    let default_msg = "update.".to_string();
    let commit_message = setting
        .comments
        .get("default")
        .unwrap_or(&default_msg)
        .clone();

    for repo in repos {
        let repo_data = (*repo).clone();
        let user_name = setting.user.name.clone();
        let user_email = setting.user.email.clone();
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

            if !repo_dir.exists() {
                update_repo_status(
                    &repos_handle,
                    &repo_name,
                    RepoStatus::Failed,
                    &format!("Directory not found: {}", repo_dir.display()),
                    100,
                );
                return;
            }

            update_repo_status(
                &repos_handle,
                &repo_name,
                RepoStatus::Running,
                "Configuring...",
                20,
            );
            git.git_config(&repo_dir, &user_name, &user_email);

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
                update_repo_status(&repos_handle, &repo_name, RepoStatus::Success, "Done", 100);
            } else {
                update_repo_status(&repos_handle, &repo_name, RepoStatus::Failed, "Failed", 100);
            }
        });
    }
}

fn extract_repo_name(remote_url: &str) -> String {
    let url = remote_url.trim_end_matches('/');
    let parts: Vec<&str> = url.split('/').collect();
    let last_part = parts.last().unwrap_or(&"");
    last_part.trim_end_matches(".git").to_string()
}
