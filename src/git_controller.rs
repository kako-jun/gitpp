use std::path::Path;
use std::process::Command;

pub struct GitResult {
    pub output: String,
    pub success: bool,
    pub had_changes: bool,
}

pub struct GitController {
    encoding: &'static encoding_rs::Encoding,
}

impl GitController {
    pub fn new() -> Self {
        let encoding = if cfg!(target_os = "windows") {
            encoding_rs::SHIFT_JIS
        } else {
            encoding_rs::UTF_8
        };

        GitController { encoding }
    }

    pub fn git_clone(&self, dir: &Path, remote: &str, branch: &str) -> GitResult {
        let result = self.exec_git(dir, &["clone", remote, "-b", branch]);
        GitResult {
            had_changes: result.success,
            ..result
        }
    }

    pub fn git_pull(&self, dir: &Path) -> GitResult {
        let result = self.exec_git(dir, &["pull"]);
        let had_changes = result.success && !result.output.contains("Already up to date");
        GitResult {
            had_changes,
            ..result
        }
    }

    pub fn git_push(&self, dir: &Path, commit_message: &str) -> GitResult {
        let mut all_output = String::new();

        let add_result = self.exec_git(dir, &["add", "-A"]);
        all_output.push_str(&add_result.output);
        if !add_result.success {
            return GitResult {
                output: all_output,
                success: false,
                had_changes: false,
            };
        }

        let mut commit_result = self.exec_git(dir, &["commit", "-m", commit_message]);
        all_output.push_str(&commit_result.output);
        if !commit_result.success {
            // "nothing to commit" is not a failure — just skip push
            if commit_result.output.contains("nothing to commit") {
                return GitResult {
                    output: all_output,
                    success: true,
                    had_changes: false,
                };
            }
            // Pre-commit hook may have modified files (e.g. formatter).
            // Retry once: re-add changed files and commit again.
            all_output.push_str("[gitpp] pre-commit hook may have modified files, retrying...\n");
            let retry_add = self.exec_git(dir, &["add", "-A"]);
            all_output.push_str(&retry_add.output);
            if retry_add.success {
                commit_result = self.exec_git(dir, &["commit", "-m", commit_message]);
                all_output.push_str(&commit_result.output);
            }
            if !commit_result.success {
                return GitResult {
                    output: all_output,
                    success: false,
                    had_changes: false,
                };
            }
        }

        let push_result = self.exec_git(dir, &["push"]);
        all_output.push_str(&push_result.output);

        GitResult {
            output: all_output,
            success: push_result.success,
            had_changes: true,
        }
    }

    pub fn git_status(&self, dir: &Path) -> GitResult {
        let result = self.exec_git(dir, &["status", "--porcelain"]);
        let had_changes = result.success && !result.output.trim().is_empty();
        GitResult {
            had_changes,
            ..result
        }
    }

    pub fn git_diff_stat(&self, dir: &Path) -> GitResult {
        let result = self.exec_git(dir, &["diff", "--stat", "HEAD"]);
        let had_changes = result.success && !result.output.trim().is_empty();
        GitResult {
            had_changes,
            ..result
        }
    }

    pub fn git_fetch(&self, dir: &Path) -> GitResult {
        let result = self.exec_git(dir, &["fetch"]);
        let had_changes = result.success && !result.output.trim().is_empty();
        GitResult {
            had_changes,
            ..result
        }
    }

    pub fn git_branch(&self, dir: &Path) -> GitResult {
        let result = self.exec_git(dir, &["rev-parse", "--abbrev-ref", "HEAD"]);
        let branch_name = result.output.trim();
        let had_changes = result.success && branch_name != "main" && branch_name != "master";
        GitResult {
            had_changes,
            ..result
        }
    }

    pub fn git_switch_default(&self, dir: &Path) -> GitResult {
        // Detect current branch first
        let current = self.exec_git(dir, &["rev-parse", "--abbrev-ref", "HEAD"]);
        let current_branch = current.output.trim().to_string();

        // Try main first, then master (use refs/heads/ to match local branches only)
        let target = if self
            .exec_git(dir, &["rev-parse", "--verify", "refs/heads/main"])
            .success
        {
            "main"
        } else if self
            .exec_git(dir, &["rev-parse", "--verify", "refs/heads/master"])
            .success
        {
            "master"
        } else {
            return GitResult {
                output: "error: neither main nor master branch found".to_string(),
                success: false,
                had_changes: false,
            };
        };

        if current_branch == target {
            return GitResult {
                output: format!("Already on '{target}'\n"),
                success: true,
                had_changes: false,
            };
        }

        let result = self.exec_git(dir, &["switch", target]);
        GitResult {
            had_changes: result.success,
            ..result
        }
    }

    pub fn git_stash_list(&self, dir: &Path) -> GitResult {
        let result = self.exec_git(dir, &["stash", "list"]);
        let had_changes = result.success && !result.output.trim().is_empty();
        GitResult {
            had_changes,
            ..result
        }
    }

    pub fn git_gc(&self, dir: &Path) -> GitResult {
        let result = self.exec_git(dir, &["gc"]);
        GitResult {
            had_changes: false,
            ..result
        }
    }

    pub fn git_remote_url(&self, dir: &Path) -> String {
        let result = self.exec_git(dir, &["remote", "get-url", "origin"]);
        result.output.trim().to_string()
    }

    pub fn git_config(&self, dir: &Path, config: &std::collections::HashMap<String, String>) {
        for (key, value) in config {
            self.exec_git(dir, &["config", "--local", key, value]);
        }
    }

    fn exec_git(&self, dir: &Path, args: &[&str]) -> GitResult {
        let output = match Command::new("git").current_dir(dir).args(args).output() {
            Ok(o) => o,
            Err(e) => {
                return GitResult {
                    output: format!("error: {e}"),
                    success: false,
                    had_changes: false,
                }
            }
        };

        let (stdout_result, _, _) = self.encoding.decode(&output.stdout);
        let (stderr_result, _, _) = self.encoding.decode(&output.stderr);

        let mut text = stdout_result.to_string();
        text.push_str(stderr_result.as_ref());

        GitResult {
            success: output.status.success(),
            output: text,
            had_changes: false,
        }
    }
}
