use std::path::Path;
use std::process::Command;

pub struct GitResult {
    pub output: String,
    pub success: bool,
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
        self.exec_git(dir, &["clone", remote, "-b", branch])
    }

    pub fn git_pull(&self, dir: &Path) -> GitResult {
        self.exec_git(dir, &["pull"])
    }

    pub fn git_push(&self, dir: &Path, commit_message: &str) -> GitResult {
        let add_result = self.exec_git(dir, &["add", "-A"]);
        if !add_result.success {
            return add_result;
        }

        let commit_result = self.exec_git(dir, &["commit", "-m", commit_message]);
        // "nothing to commit" exits non-zero but is not a real failure for our purposes
        if !commit_result.success && !commit_result.output.contains("nothing to commit") {
            return commit_result;
        }

        self.exec_git(dir, &["push"])
    }

    pub fn git_config(&self, dir: &Path, name: &str, email: &str) {
        self.exec_git(dir, &["config", "user.name", name]);
        self.exec_git(dir, &["config", "user.email", email]);
    }

    fn exec_git(&self, dir: &Path, args: &[&str]) -> GitResult {
        let output = match Command::new("git").current_dir(dir).args(args).output() {
            Ok(o) => o,
            Err(e) => {
                return GitResult {
                    output: format!("error: {e}"),
                    success: false,
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
        }
    }
}
