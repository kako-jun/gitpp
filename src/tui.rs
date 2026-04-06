use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame, Terminal,
};
use std::collections::HashSet;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub enum RepoStatus {
    Waiting,
    Running,
    Updated,
    Unchanged,
    Failed,
}

#[derive(Debug, Clone)]
pub struct RepoProgress {
    pub name: String,
    pub path: String,
    pub status: RepoStatus,
    pub message: String,
    pub progress: u16,
    pub output: String,
}

pub struct TuiApp {
    repos: Arc<Mutex<Vec<RepoProgress>>>,
    command: String,
    selected: usize,
    scroll_offset: usize,
    show_detail: bool,
    detail_scroll: u16,
    status_message: Option<(String, Instant)>,
    clipboard: Option<arboard::Clipboard>,
    auto_exit_hint: bool,
}

impl TuiApp {
    pub fn new(repo_names: Vec<String>, repo_paths: Vec<String>, command: &str) -> Self {
        let repos = repo_names
            .into_iter()
            .zip(repo_paths)
            .map(|(name, path)| RepoProgress {
                name,
                path,
                status: RepoStatus::Waiting,
                message: "Waiting...".to_string(),
                progress: 0,
                output: String::new(),
            })
            .collect();

        TuiApp {
            repos: Arc::new(Mutex::new(repos)),
            command: command.to_string(),
            selected: 0,
            scroll_offset: 0,
            show_detail: true,
            detail_scroll: 0,
            status_message: None,
            clipboard: arboard::Clipboard::new().ok(),
            auto_exit_hint: false,
        }
    }

    pub fn get_repos_handle(&self) -> Arc<Mutex<Vec<RepoProgress>>> {
        Arc::clone(&self.repos)
    }

    pub fn run_quiet(&mut self, interrupted: Arc<AtomicBool>) -> Result<(), io::Error> {
        let mut reported: HashSet<String> = HashSet::new();

        {
            let repos = self.repos.lock().unwrap_or_else(|e| e.into_inner());
            if repos.is_empty() {
                return Ok(());
            }
        }

        loop {
            if interrupted.load(Ordering::Relaxed) {
                eprintln!("\nInterrupted. Reporting current status...");
                let repos = self.repos.lock().unwrap_or_else(|e| e.into_inner());
                let running: Vec<_> = repos
                    .iter()
                    .filter(|r| r.status == RepoStatus::Waiting || r.status == RepoStatus::Running)
                    .collect();
                if !running.is_empty() {
                    eprintln!("{} repositories still in progress:", running.len());
                    for repo in &running {
                        eprintln!("  {} ({:?})", repo.name, repo.status);
                    }
                }
                break;
            }

            {
                let repos = self.repos.lock().unwrap_or_else(|e| e.into_inner());

                for repo in repos.iter() {
                    if reported.contains(&repo.name) {
                        continue;
                    }
                    match repo.status {
                        RepoStatus::Updated => {
                            eprintln!("[{}] {}... updated", self.command, repo.name);
                            reported.insert(repo.name.clone());
                        }
                        RepoStatus::Unchanged => {
                            eprintln!("[{}] {}... unchanged", self.command, repo.name);
                            reported.insert(repo.name.clone());
                        }
                        RepoStatus::Failed => {
                            eprintln!("[{}] {}... FAILED", self.command, repo.name);
                            reported.insert(repo.name.clone());
                        }
                        _ => {}
                    }
                }

                let all_done = repos.iter().all(|r| {
                    matches!(
                        r.status,
                        RepoStatus::Updated | RepoStatus::Unchanged | RepoStatus::Failed
                    )
                });
                if all_done {
                    break;
                }
            }

            std::thread::sleep(Duration::from_millis(200));
        }

        self.print_summary();
        Ok(())
    }

    pub fn run(&mut self) -> Result<(), io::Error> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let res = self.run_app(&mut terminal);

        disable_raw_mode()?;
        execute!(
            terminal.backend_mut(),
            LeaveAlternateScreen,
            DisableMouseCapture
        )?;
        terminal.show_cursor()?;

        if let Err(err) = res {
            println!("{err:?}");
        }

        self.print_summary();

        Ok(())
    }

    fn run_app<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> io::Result<()> {
        loop {
            terminal.draw(|f| self.ui(f))?;

            let repos = self.repos.lock().unwrap_or_else(|e| e.into_inner());
            let all_done = repos.iter().all(|r| {
                matches!(
                    r.status,
                    RepoStatus::Updated | RepoStatus::Unchanged | RepoStatus::Failed
                )
            });
            drop(repos);

            if all_done {
                self.auto_exit_hint = true;
                terminal.draw(|f| self.ui(f))?;
                if event::poll(Duration::from_secs(2))? {
                    if let Event::Key(key) = event::read()? {
                        match key.code {
                            KeyCode::Char('q') | KeyCode::Esc => break,
                            _ => {
                                // User interacted, switch to browse mode
                                self.handle_key(key.code);
                                self.browse_mode(terminal)?;
                                break;
                            }
                        }
                    }
                }
                break;
            }

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if let KeyCode::Char('q') = key.code {
                        break;
                    }
                    self.handle_key(key.code);
                }
            }
        }

        Ok(())
    }

    fn browse_mode<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> io::Result<()> {
        loop {
            terminal.draw(|f| self.ui(f))?;

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Char('q') => break,
                        KeyCode::Esc if !self.show_detail => break,
                        _ => self.handle_key(key.code),
                    }
                }
            }
        }
        Ok(())
    }

    fn handle_key(&mut self, key: KeyCode) {
        let repo_count = self.repos.lock().unwrap_or_else(|e| e.into_inner()).len();
        if repo_count == 0 {
            return;
        }

        match key {
            KeyCode::Char('j') | KeyCode::Down => {
                if self.selected + 1 < repo_count {
                    self.selected += 1;
                    self.detail_scroll = 0;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                if self.selected > 0 {
                    self.selected -= 1;
                    self.detail_scroll = 0;
                }
            }
            KeyCode::Char('g') => {
                self.selected = 0;
                self.detail_scroll = 0;
            }
            KeyCode::Char('G') => {
                self.selected = repo_count.saturating_sub(1);
                self.detail_scroll = 0;
            }
            KeyCode::Char('l') | KeyCode::Right if self.show_detail => {
                let max_scroll = self.detail_line_count().saturating_sub(1) as u16;
                self.detail_scroll = self.detail_scroll.saturating_add(3).min(max_scroll);
            }
            KeyCode::Char('h') | KeyCode::Left if self.show_detail => {
                self.detail_scroll = self.detail_scroll.saturating_sub(3);
            }
            KeyCode::Enter => {
                self.show_detail = !self.show_detail;
                self.detail_scroll = 0;
            }
            KeyCode::Esc => {
                self.show_detail = false;
            }
            KeyCode::Char('y') => {
                self.copy_detail_to_clipboard();
            }
            KeyCode::Char('n') => {
                self.jump_to_next_failed(repo_count);
            }
            KeyCode::Char('N') => {
                self.jump_to_prev_failed(repo_count);
            }
            _ => {}
        }
    }

    fn copy_detail_to_clipboard(&mut self) {
        let repos = self.repos.lock().unwrap_or_else(|e| e.into_inner());
        let text = if let Some(repo) = repos.get(self.selected) {
            if repo.output.is_empty() {
                repo.message.clone()
            } else {
                repo.output.clone()
            }
        } else {
            return;
        };
        drop(repos);

        match &mut self.clipboard {
            Some(cb) => match cb.set_text(text) {
                Ok(()) => {
                    self.status_message = Some(("Copied!".to_string(), Instant::now()));
                }
                Err(e) => {
                    self.status_message = Some((format!("Copy failed: {e}"), Instant::now()));
                }
            },
            None => {
                self.status_message = Some(("Clipboard unavailable".to_string(), Instant::now()));
            }
        }
    }

    fn jump_to_next_failed(&mut self, repo_count: usize) {
        let found = {
            let repos = self.repos.lock().unwrap_or_else(|e| e.into_inner());
            (1..repo_count)
                .map(|offset| (self.selected + offset) % repo_count)
                .find(|&idx| repos[idx].status == RepoStatus::Failed)
        };
        if let Some(idx) = found {
            self.selected = idx;
            self.detail_scroll = 0;
        } else {
            self.status_message = Some(("No errors".to_string(), Instant::now()));
        }
    }

    fn jump_to_prev_failed(&mut self, repo_count: usize) {
        let found = {
            let repos = self.repos.lock().unwrap_or_else(|e| e.into_inner());
            (1..repo_count)
                .map(|offset| (self.selected + repo_count - offset) % repo_count)
                .find(|&idx| repos[idx].status == RepoStatus::Failed)
        };
        if let Some(idx) = found {
            self.selected = idx;
            self.detail_scroll = 0;
        } else {
            self.status_message = Some(("No errors".to_string(), Instant::now()));
        }
    }

    fn print_summary(&self) {
        let repos = self.repos.lock().unwrap_or_else(|e| e.into_inner());
        let total = repos.len();
        let updated_count = repos
            .iter()
            .filter(|r| r.status == RepoStatus::Updated)
            .count();
        let unchanged_count = repos
            .iter()
            .filter(|r| r.status == RepoStatus::Unchanged)
            .count();
        let failed: Vec<_> = repos
            .iter()
            .filter(|r| r.status == RepoStatus::Failed)
            .collect();

        let done = updated_count + unchanged_count + failed.len();

        if failed.is_empty() {
            println!("Total: {total} | Done: {done} (Updated: {updated_count} / Unchanged: {unchanged_count} / Failed: 0)");
            return;
        }

        // Plain text, no ANSI codes — clipboard-friendly
        let failed_count = failed.len();
        println!(
            "Total: {total} | Done: {done} (Updated: {updated_count} / Unchanged: {unchanged_count} / Failed: {failed_count})\n"
        );
        for repo in &failed {
            println!("--- {} ({}) ---", repo.name, repo.path);
            if repo.output.is_empty() {
                println!("  {}", repo.message);
            } else {
                for line in repo.output.lines() {
                    println!("  {line}");
                }
            }
            println!();
        }
    }

    fn ui(&mut self, f: &mut Frame) {
        let footer_height = if self.auto_exit_hint { 4 } else { 3 };
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(4),
                Constraint::Min(0),
                Constraint::Length(footer_height),
            ])
            .split(f.area());

        // Header (2 lines)
        let header_line2 = Line::from(Span::styled(
            "       Enter:detail  h/l:scroll  y:copy  Esc:close  q:quit",
            Style::default().fg(Color::Gray),
        ));
        let mut header_line1_spans = vec![
            Span::styled(
                "gitpp",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  "),
            Span::styled(
                "j/k:move  g/G:top/end  n/N:next/prev error",
                Style::default().fg(Color::Gray),
            ),
        ];

        // Status message (auto-expires after 2 seconds) — appended to line 1
        let show_msg = match &self.status_message {
            Some((msg, at)) if at.elapsed() < Duration::from_secs(2) => Some(msg.clone()),
            Some(_) => None,
            None => None,
        };
        if show_msg.is_none() && self.status_message.is_some() {
            self.status_message = None;
        }
        if let Some(msg) = show_msg {
            header_line1_spans.push(Span::raw("  "));
            header_line1_spans.push(Span::styled(
                msg,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));
        }

        let header = Paragraph::new(vec![Line::from(header_line1_spans), header_line2])
            .block(Block::default().borders(Borders::ALL));
        f.render_widget(header, chunks[0]);

        // Main area: repo list (+ optional detail pane)
        if self.show_detail {
            let main_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(chunks[1]);

            self.render_repos(f, main_chunks[0]);
            self.render_detail(f, main_chunks[1]);
        } else {
            self.render_repos(f, chunks[1]);
        }

        // Footer
        let repos = self.repos.lock().unwrap_or_else(|e| e.into_inner());
        let total = repos.len();
        let updated = repos
            .iter()
            .filter(|r| r.status == RepoStatus::Updated)
            .count();
        let unchanged = repos
            .iter()
            .filter(|r| r.status == RepoStatus::Unchanged)
            .count();
        let failed = repos
            .iter()
            .filter(|r| r.status == RepoStatus::Failed)
            .count();
        let done = updated + unchanged + failed;
        drop(repos);

        let stats_line = Line::from(vec![
            Span::styled("Total: ", Style::default().fg(Color::White)),
            Span::styled(format!("{total} "), Style::default().fg(Color::Cyan)),
            Span::raw("| "),
            Span::styled("Done: ", Style::default().fg(Color::White)),
            Span::styled(format!("{done} "), Style::default().fg(Color::Yellow)),
            Span::raw("("),
            Span::styled("Updated: ", Style::default().fg(Color::White)),
            Span::styled(format!("{updated}"), Style::default().fg(Color::Green)),
            Span::raw(" / "),
            Span::styled("Unchanged: ", Style::default().fg(Color::White)),
            Span::styled(format!("{unchanged}"), Style::default().fg(Color::DarkGray)),
            Span::raw(" / "),
            Span::styled("Failed: ", Style::default().fg(Color::White)),
            Span::styled(format!("{failed}"), Style::default().fg(Color::Red)),
            Span::raw(")"),
        ]);

        let mut footer_lines = vec![stats_line];
        if self.auto_exit_hint {
            footer_lines.push(Line::from(Span::styled(
                "Auto-exit in 2s — press any key to browse",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
        }

        let footer = Paragraph::new(footer_lines).block(Block::default().borders(Borders::ALL));
        f.render_widget(footer, chunks[2]);
    }

    fn render_repos(&mut self, f: &mut Frame, area: Rect) {
        let repos = self.repos.lock().unwrap_or_else(|e| e.into_inner());

        // Each repo takes 2 lines (status + progress bar), no blank line between
        let lines_per_repo = 2;
        let visible_height = area.height.saturating_sub(2) as usize; // subtract borders
        let visible_repos = visible_height / lines_per_repo;

        // Adjust scroll_offset to keep selected visible
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        }
        if visible_repos > 0 && self.selected >= self.scroll_offset + visible_repos {
            self.scroll_offset = self.selected - visible_repos + 1;
        }
        let scroll_offset = self.scroll_offset;

        let mut lines = vec![];
        let end = (scroll_offset + visible_repos).min(repos.len());

        for (i, repo) in repos
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(end - scroll_offset)
        {
            let is_selected = i == self.selected;
            let (status_icon, status_color) = match repo.status {
                RepoStatus::Waiting => ("⏸", Color::DarkGray),
                RepoStatus::Running => ("▶", Color::Yellow),
                RepoStatus::Updated => ("✓", Color::Green),
                RepoStatus::Unchanged => ("─", Color::DarkGray),
                RepoStatus::Failed => ("✗", Color::Red),
            };

            let name_style = if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
            } else {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            };

            let selector = if is_selected { "▸" } else { " " };

            lines.push(Line::from(vec![
                Span::styled(
                    format!("{selector}{status_icon} "),
                    Style::default()
                        .fg(status_color)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    if repo.name.len() > 36 {
                        format!("{}…", &repo.name[..35])
                    } else {
                        format!("{:36}", repo.name)
                    },
                    name_style,
                ),
                Span::styled(
                    format!(" {}", repo.message),
                    Style::default().fg(Color::White),
                ),
            ]));

            // Progress bar
            let bar_width = 40;
            let filled = (bar_width as f32 * repo.progress as f32 / 100.0) as usize;
            let empty = bar_width - filled;
            let bar = format!(
                "  [{}{}] {:>3}%",
                "█".repeat(filled),
                "░".repeat(empty),
                repo.progress
            );

            lines.push(Line::from(Span::styled(
                bar,
                Style::default().fg(match repo.status {
                    RepoStatus::Updated => Color::Green,
                    RepoStatus::Unchanged => Color::DarkGray,
                    RepoStatus::Failed => Color::Red,
                    RepoStatus::Running => Color::Yellow,
                    RepoStatus::Waiting => Color::DarkGray,
                }),
            )));
        }

        // Scroll indicator in title
        let scroll_info = if repos.len() > visible_repos && visible_repos > 0 {
            format!(
                " Repositories [{}-{}/{}] ",
                scroll_offset + 1,
                end,
                repos.len()
            )
        } else {
            format!(" Repositories ({}) ", repos.len())
        };

        let paragraph = Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(scroll_info)
                .style(Style::default()),
        );

        f.render_widget(paragraph, area);
    }

    fn detail_line_count(&self) -> usize {
        let repos = self.repos.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(repo) = repos.get(self.selected) {
            let text = if repo.output.is_empty() {
                &repo.message
            } else {
                &repo.output
            };
            text.lines().count()
        } else {
            0
        }
    }

    fn render_detail(&self, f: &mut Frame, area: Rect) {
        let repos = self.repos.lock().unwrap_or_else(|e| e.into_inner());

        let (title, content) = if let Some(repo) = repos.get(self.selected) {
            let title = format!(" {} ", repo.name);
            let text = if repo.output.is_empty() {
                repo.message.clone()
            } else {
                repo.output.clone()
            };
            (title, text)
        } else {
            (
                " Detail ".to_string(),
                "No repository selected.".to_string(),
            )
        };

        let lines: Vec<Line> = content.lines().map(|l| Line::from(l.to_string())).collect();

        let paragraph = Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .style(Style::default()),
            )
            .wrap(Wrap { trim: false })
            .scroll((self.detail_scroll, 0));

        f.render_widget(paragraph, area);
    }
}

pub fn update_repo_status(
    repos: &Arc<Mutex<Vec<RepoProgress>>>,
    repo_name: &str,
    status: RepoStatus,
    message: &str,
    progress: u16,
) {
    let mut repos = repos.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(repo) = repos.iter_mut().find(|r| r.name == repo_name) {
        repo.status = status;
        repo.message = message.to_string();
        repo.progress = progress;
    }
}

pub fn append_repo_output(repos: &Arc<Mutex<Vec<RepoProgress>>>, repo_name: &str, output: &str) {
    let mut repos = repos.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(repo) = repos.iter_mut().find(|r| r.name == repo_name) {
        if !repo.output.is_empty() {
            repo.output.push('\n');
        }
        repo.output.push_str(output);
    }
}
