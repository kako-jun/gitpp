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
        let result = self.exec_git(
            dir,
            &["clone", remote, "-b", branch, "--recurse-submodules"],
        );
        GitResult {
            had_changes: result.success,
            ..result
        }
    }

    pub fn git_pull(&self, dir: &Path) -> GitResult {
        let mut all_output = String::new();

        // 1. Fetch from remote. This is the only path that can mark pull as Failed
        //    (network / auth errors). Everything after this stays success:true.
        let fetch_result = self.exec_git(dir, &["fetch", "--prune"]);
        all_output.push_str(&fetch_result.output);
        if !fetch_result.success {
            return GitResult {
                output: all_output,
                success: false,
                had_changes: false,
            };
        }

        // 2. Determine current branch and upstream.
        let head = self.exec_git(dir, &["rev-parse", "--abbrev-ref", "HEAD"]);
        let detached = !head.success || head.output.trim() == "HEAD";
        let upstream = self.exec_git(
            dir,
            &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"],
        );
        let has_upstream = upstream.success;

        // 3. Merge branch. None of these mark pull as Failed.
        let mut ff_applied = false;
        if detached || !has_upstream {
            all_output.push_str("[gitpp] fetched only (no upstream)\n");
        } else {
            let merge_result = self.exec_git(dir, &["merge", "--ff-only", "@{u}"]);
            all_output.push_str(&merge_result.output);
            if merge_result.success {
                // Fast-forward succeeded. "changed" only when something actually moved.
                ff_applied = !merge_result.output.contains("Already up to date");
            } else {
                // Diverged (non-FF) or dirty working tree blocking the update.
                // Do not error and do not discard the user's changes — just report.
                all_output.push_str("[gitpp] fast-forward skipped (diverged or local changes)\n");
            }
        }

        // 4. Sync submodules. Failure here never affects the pull result.
        let sub_result = self.exec_git(dir, &["submodule", "update", "--init", "--recursive"]);
        all_output.push_str(&sub_result.output);
        let sub_changed = sub_result.success && !sub_result.output.trim().is_empty();
        if !sub_result.success {
            all_output.push_str("[gitpp] warning: submodule update failed\n");
        }

        GitResult {
            output: all_output,
            success: true,
            had_changes: ff_applied || sub_changed,
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

    pub fn is_valid_repo(&self, dir: &Path) -> bool {
        let result = self.exec_git(dir, &["rev-parse", "HEAD"]);
        result.success
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    /// Run a raw git command in `dir` with a deterministic identity / config so
    /// fixtures never depend on the host's global git settings, and never touch
    /// the test process's own working directory. Panics on failure because these
    /// calls only build fixtures — the code under test is `GitController`.
    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .current_dir(dir)
            .args([
                "-c",
                "user.name=gitpp-test",
                "-c",
                "user.email=gitpp-test@example.com",
                "-c",
                "init.defaultBranch=main",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "protocol.file.allow=always",
            ])
            .args(args)
            .output()
            .expect("failed to spawn git");
        assert!(
            status.status.success(),
            "fixture git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&status.stderr)
        );
    }

    fn head_sha(dir: &Path) -> String {
        let out = Command::new("git")
            .current_dir(dir)
            .args(["rev-parse", "HEAD"])
            .output()
            .expect("failed to spawn git rev-parse");
        assert!(out.status.success(), "rev-parse HEAD failed");
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Create a bare origin with one seed commit on `main`, returning its path.
    fn seed_origin(parent: &Path) -> std::path::PathBuf {
        let origin = parent.join("origin.git");
        git(parent, &["init", "--bare", origin.to_str().unwrap()]);

        let seed = parent.join("seed");
        git(
            parent,
            &["clone", origin.to_str().unwrap(), seed.to_str().unwrap()],
        );
        fs::write(seed.join("file.txt"), "line1\n").unwrap();
        git(&seed, &["add", "-A"]);
        git(&seed, &["commit", "-m", "seed"]);
        git(&seed, &["push", "origin", "main"]);
        origin
    }

    /// Clone `origin` into `parent/work` (tracking origin/main) and return the path.
    fn clone_work(parent: &Path, origin: &Path) -> std::path::PathBuf {
        let work = parent.join("work");
        git(
            parent,
            &["clone", origin.to_str().unwrap(), work.to_str().unwrap()],
        );
        work
    }

    /// Add a new commit on `main` to `origin` (via a throwaway clone) so a later
    /// `git_pull` of a work tree sees the branch advance.
    fn advance_origin(parent: &Path, origin: &Path) {
        let bump = parent.join(format!("bump-{}", head_sha_seed(parent)));
        git(
            parent,
            &["clone", origin.to_str().unwrap(), bump.to_str().unwrap()],
        );
        let content = fs::read_to_string(bump.join("file.txt")).unwrap_or_default();
        fs::write(bump.join("file.txt"), format!("{content}more\n")).unwrap();
        git(&bump, &["add", "-A"]);
        git(&bump, &["commit", "-m", "advance"]);
        git(&bump, &["push", "origin", "main"]);
    }

    // Tiny unique-ish suffix so repeated advances use distinct temp clone dirs.
    fn head_sha_seed(parent: &Path) -> String {
        let n = fs::read_dir(parent).map(|d| d.count()).unwrap_or(0);
        format!("{n}")
    }

    // --- D1: fetch failure is the only Failed outcome -----------------------

    #[test]
    fn pull_fetch_failure_is_only_failed() {
        let tmp = TempDir::new().unwrap();
        let origin = seed_origin(tmp.path());
        let work = clone_work(tmp.path(), &origin);

        // Make origin unreachable: remove the bare repo the work tree fetches from.
        fs::remove_dir_all(&origin).unwrap();

        let result = GitController::new().git_pull(&work);

        assert!(!result.success, "fetch failure must mark pull as Failed");
        assert!(!result.had_changes);
        let lower = result.output.to_lowercase();
        assert!(
            lower.contains("could not read from remote")
                || lower.contains("does not appear to be a git repository")
                || lower.contains("fatal"),
            "output should carry the fetch error: {}",
            result.output
        );
    }

    // --- D2: fast-forward applied is Updated --------------------------------

    #[test]
    fn pull_fast_forward_applied_is_updated() {
        let tmp = TempDir::new().unwrap();
        let origin = seed_origin(tmp.path());
        let work = clone_work(tmp.path(), &origin);
        advance_origin(tmp.path(), &origin);

        let result = GitController::new().git_pull(&work);

        assert!(result.success);
        assert!(result.had_changes, "an applied fast-forward is Updated");
        assert!(
            !result.output.contains("Already up to date"),
            "applied FF must not report up-to-date: {}",
            result.output
        );
        assert!(
            !result.output.contains("fast-forward skipped"),
            "applied FF must not report a skip: {}",
            result.output
        );
    }

    // --- D3: already up to date is Unchanged --------------------------------

    #[test]
    fn pull_already_up_to_date_is_unchanged() {
        let tmp = TempDir::new().unwrap();
        let origin = seed_origin(tmp.path());
        let work = clone_work(tmp.path(), &origin);

        let result = GitController::new().git_pull(&work);

        assert!(result.success);
        assert!(!result.had_changes, "no movement means Unchanged");
        assert!(
            result.output.contains("Already up to date"),
            "output should report up-to-date: {}",
            result.output
        );
    }

    // --- D4: diverged is Unchanged and non-destructive ----------------------

    #[test]
    fn pull_diverged_is_unchanged_and_nondestructive() {
        let tmp = TempDir::new().unwrap();
        let origin = seed_origin(tmp.path());
        let work = clone_work(tmp.path(), &origin);

        // origin gets commit A; local gets a different commit B -> non-FF divergence.
        advance_origin(tmp.path(), &origin);
        fs::write(work.join("local.txt"), "local change\n").unwrap();
        git(&work, &["add", "-A"]);
        git(&work, &["commit", "-m", "local commit B"]);
        let before = head_sha(&work);

        let result = GitController::new().git_pull(&work);

        assert!(result.success, "divergence is reported, not a hard failure");
        assert!(!result.had_changes);
        assert!(
            result
                .output
                .contains("fast-forward skipped (diverged or local changes)"),
            "output should carry the diverged marker: {}",
            result.output
        );
        assert!(
            !result.output.contains("fetched only (no upstream)"),
            "diverged branch has an upstream; must not claim fetch-only: {}",
            result.output
        );
        assert_eq!(
            before,
            head_sha(&work),
            "local commit B must survive a non-FF pull untouched"
        );
    }

    // --- D5: dirty working tree is Unchanged and preserves the edit ----------

    #[test]
    fn pull_dirty_tree_is_unchanged_and_preserves_edit() {
        let tmp = TempDir::new().unwrap();
        let origin = seed_origin(tmp.path());
        let work = clone_work(tmp.path(), &origin);

        // origin advances; locally we leave an uncommitted edit to the tracked file,
        // which blocks a fast-forward.
        advance_origin(tmp.path(), &origin);
        let dirty = "line1\nUNCOMMITTED LOCAL EDIT\n";
        fs::write(work.join("file.txt"), dirty).unwrap();

        let result = GitController::new().git_pull(&work);

        assert!(result.success);
        assert!(!result.had_changes);
        assert!(
            result.output.contains("fast-forward skipped"),
            "blocked FF should report a skip: {}",
            result.output
        );
        assert_eq!(
            fs::read(work.join("file.txt")).unwrap(),
            dirty.as_bytes(),
            "the uncommitted edit must remain byte-for-byte after pull"
        );
    }

    // --- D6: branch without upstream is fetched-only ------------------------

    #[test]
    fn pull_no_upstream_is_fetched_only() {
        let tmp = TempDir::new().unwrap();
        let origin = seed_origin(tmp.path());
        let work = clone_work(tmp.path(), &origin);

        // A fresh local branch has no tracking ref.
        git(&work, &["checkout", "-b", "no-upstream"]);

        let result = GitController::new().git_pull(&work);

        assert!(result.success);
        assert!(!result.had_changes);
        assert!(
            result.output.contains("fetched only (no upstream)"),
            "no-upstream branch should be fetched-only: {}",
            result.output
        );
        assert!(
            !result.output.contains("fast-forward skipped"),
            "fetch-only must not also claim a merge skip: {}",
            result.output
        );
    }

    // --- D7: detached HEAD is fetched-only ----------------------------------

    #[test]
    fn pull_detached_head_is_fetched_only() {
        let tmp = TempDir::new().unwrap();
        let origin = seed_origin(tmp.path());
        let work = clone_work(tmp.path(), &origin);

        let sha = head_sha(&work);
        git(&work, &["checkout", &sha]);

        let result = GitController::new().git_pull(&work);

        assert!(result.success);
        assert!(!result.had_changes);
        assert!(
            result.output.contains("fetched only (no upstream)"),
            "detached HEAD should be fetched-only: {}",
            result.output
        );
    }

    // --- D9: diverged output carries only the diverged marker ---------------

    #[test]
    fn pull_diverged_output_has_only_diverged_marker() {
        let tmp = TempDir::new().unwrap();
        let origin = seed_origin(tmp.path());
        let work = clone_work(tmp.path(), &origin);

        advance_origin(tmp.path(), &origin);
        fs::write(work.join("local.txt"), "local change\n").unwrap();
        git(&work, &["add", "-A"]);
        git(&work, &["commit", "-m", "local commit B"]);

        let result = GitController::new().git_pull(&work);

        assert!(
            result.output.contains("fast-forward skipped"),
            "diverged output must contain the skip marker: {}",
            result.output
        );
        assert!(
            !result.output.contains("fetched only (no upstream)"),
            "markers are exclusive: diverged must not also be fetch-only: {}",
            result.output
        );
    }

    // --- clone -------------------------------------------------------------

    #[test]
    fn clone_succeeds_sets_had_changes() {
        let tmp = TempDir::new().unwrap();
        let origin = seed_origin(tmp.path());

        // git_clone takes the *parent* dir and clones into a subdir named after
        // the remote (origin.git -> origin).
        let parent = tmp.path().join("dest");
        fs::create_dir_all(&parent).unwrap();
        let result = GitController::new().git_clone(&parent, origin.to_str().unwrap(), "main");

        assert!(
            result.success,
            "local bare clone should succeed: {}",
            result.output
        );
        assert!(result.had_changes, "a successful clone reports had_changes");
        assert!(
            parent.join("origin").join("file.txt").exists(),
            "cloned working tree should contain the seeded file"
        );
    }

    // --- submodule failure tolerance ---------------------------------------

    /// The pull path runs `git submodule update` with a plain git invocation
    /// (no `protocol.file.allow=always`), so a local-path submodule fails with
    /// `transport 'file' not allowed`. That failure must NOT fail the pull.
    #[test]
    fn pull_submodule_failure_still_succeeds() {
        let tmp = TempDir::new().unwrap();
        let origin = seed_origin(tmp.path());

        // Build a second bare repo to act as a submodule source.
        let sub_origin = tmp.path().join("sub-origin.git");
        git(
            tmp.path(),
            &["init", "--bare", sub_origin.to_str().unwrap()],
        );
        let sub_seed = tmp.path().join("sub-seed");
        git(
            tmp.path(),
            &[
                "clone",
                sub_origin.to_str().unwrap(),
                sub_seed.to_str().unwrap(),
            ],
        );
        fs::write(sub_seed.join("sub.txt"), "sub\n").unwrap();
        git(&sub_seed, &["add", "-A"]);
        git(&sub_seed, &["commit", "-m", "sub seed"]);
        git(&sub_seed, &["push", "origin", "main"]);

        // Register the submodule in origin via a throwaway clone.
        let setup = tmp.path().join("setup");
        git(
            tmp.path(),
            &["clone", origin.to_str().unwrap(), setup.to_str().unwrap()],
        );
        git(
            &setup,
            &["submodule", "add", sub_origin.to_str().unwrap(), "mysub"],
        );
        git(&setup, &["commit", "-m", "add submodule"]);
        git(&setup, &["push", "origin", "main"]);

        // Work tree on the commit that introduces the submodule.
        let work = clone_work(tmp.path(), &origin);

        let result = GitController::new().git_pull(&work);

        assert!(
            result.success,
            "submodule update failure must not fail the pull: {}",
            result.output
        );
        assert!(
            result.output.contains("warning: submodule update failed"),
            "a failed submodule sync should be reported as a warning: {}",
            result.output
        );
    }
}
