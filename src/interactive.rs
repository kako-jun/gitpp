use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{Context, Editor, Helper};
use std::borrow::Cow;

pub struct GitppHelper {
    commands: Vec<String>,
}

impl GitppHelper {
    pub fn new() -> Self {
        GitppHelper {
            commands: vec![
                "clone".to_string(),
                "pull".to_string(),
                "push".to_string(),
                "help".to_string(),
                "exit".to_string(),
                "quit".to_string(),
            ],
        }
    }
}

impl Completer for GitppHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let input = &line[..pos];
        let mut candidates = Vec::new();

        for cmd in &self.commands {
            if cmd.starts_with(input) {
                candidates.push(Pair {
                    display: cmd.clone(),
                    replacement: cmd.clone(),
                });
            }
        }

        Ok((0, candidates))
    }
}

impl Hinter for GitppHelper {
    type Hint = String;

    fn hint(&self, line: &str, pos: usize, _ctx: &Context<'_>) -> Option<String> {
        if pos < line.len() {
            return None;
        }

        for cmd in &self.commands {
            if cmd.starts_with(line) && cmd != line {
                return Some(cmd[line.len()..].to_string());
            }
        }

        None
    }
}

impl Highlighter for GitppHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        if self.commands.iter().any(|c| c == line) {
            Cow::Owned(format!("\x1b[1;36m{line}\x1b[0m"))
        } else {
            Cow::Borrowed(line)
        }
    }

    fn highlight_char(&self, _line: &str, _pos: usize, _forced: bool) -> bool {
        true
    }
}

impl Validator for GitppHelper {}

impl Helper for GitppHelper {}

pub fn run_interactive_mode() -> rustyline::Result<Vec<String>> {
    let helper = GitppHelper::new();
    let mut rl = Editor::new()?;
    rl.set_helper(Some(helper));

    let history_file = dirs::home_dir()
        .map(|mut path| {
            path.push(".gitpp_history");
            path
        })
        .unwrap_or_else(|| std::path::PathBuf::from(".gitpp_history"));

    let _ = rl.load_history(&history_file);

    println!("\x1b[1;36mgitpp\x1b[0m - Git Personal Parallel Manager");
    println!(
        "Type '\x1b[1;33mhelp\x1b[0m' for available commands, '\x1b[1;33mexit\x1b[0m' to quit\n"
    );

    loop {
        let readline = rl.readline("\x1b[1;36mgitpp>\x1b[0m ");
        match readline {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                rl.add_history_entry(trimmed)?;

                match trimmed {
                    "exit" | "quit" => {
                        println!("Goodbye!");
                        break;
                    }
                    "help" | "?" => {
                        show_help();
                    }
                    cmd => {
                        let parts: Vec<String> = cmd.split_whitespace().map(String::from).collect();
                        let _ = rl.save_history(&history_file);
                        return Ok(parts);
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("^C");
                continue;
            }
            Err(ReadlineError::Eof) => {
                println!("^D");
                break;
            }
            Err(err) => {
                eprintln!("Error: {err:?}");
                break;
            }
        }
    }

    let _ = rl.save_history(&history_file);
    Ok(vec![])
}

fn show_help() {
    println!("\n\x1b[1;36mAvailable Commands:\x1b[0m");
    println!("  \x1b[1;33mclone\x1b[0m              Clone all enabled repositories");
    println!("  \x1b[1;33mpull\x1b[0m               Pull all enabled repositories");
    println!("  \x1b[1;33mpush\x1b[0m               Push all enabled repositories");
    println!("  \x1b[1;33mhelp\x1b[0m, \x1b[1;33m?\x1b[0m            Show this help message");
    println!("  \x1b[1;33mexit\x1b[0m, \x1b[1;33mquit\x1b[0m         Exit interactive mode");
    println!("\n\x1b[1;36mOptions:\x1b[0m");
    println!("  \x1b[1;33m-j N\x1b[0m, \x1b[1;33m--jobs N\x1b[0m     Max parallel jobs (default: from gitpp.yaml, or 20)");
    println!("\n\x1b[1;36mTips:\x1b[0m");
    println!("  - Use \x1b[1;33mTab\x1b[0m for auto-completion");
    println!("  - Use \x1b[1;33m↑/↓\x1b[0m arrows for command history");
    println!();
}
