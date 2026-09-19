use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use jiwa::{RevealHandle, RevealOpts, Rgb};
use ratatui::{
    backend::{Backend, CrosstermBackend},
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Padding, Paragraph, Wrap},
    Frame, Terminal,
};
use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

const PANE_PADDING: u16 = 1;
const PANE_BORDER_SIZE: u16 = 2;
const PANE_CONTENT_OVERHEAD: u16 = PANE_BORDER_SIZE + PANE_PADDING * 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepoStatus {
    Waiting,
    Running,
    Updated,
    Unchanged,
    /// pull-only: the ff-only merge was skipped (diverged branch or dirty
    /// working tree). Distinct from Unchanged so it can be flagged for
    /// manual follow-up. Never produced by other commands.
    Blocked,
    Failed,
    Untracked,
}

#[derive(Debug)]
struct NameReveal {
    handle: RevealHandle,
    status: RepoStatus,
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
    /// repo path → 完了瞬間に名前を bloom させる reveal。
    ///
    /// The map is kept bounded to the current repository rows. Waiting/Running
    /// rows remove their entry, arming the next terminal transition to bloom
    /// again. Keeping the terminal status with the handle also handles a direct
    /// terminal-to-terminal update without stale animation state.
    name_reveals: HashMap<String, NameReveal>,
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
            auto_exit_hint: true,
            name_reveals: HashMap::new(),
        }
    }

    /// リポ名を端末セル幅36に収める。切り詰めは拡張書記素単位で行うため、
    /// 非ASCII名・結合文字・絵文字シーケンスの途中で文字列を切らない。
    fn format_repo_name(name: &str) -> String {
        const MAX_WIDTH: usize = 36;
        let width = UnicodeWidthStr::width(name);
        if width <= MAX_WIDTH {
            return format!("{name}{}", " ".repeat(MAX_WIDTH - width));
        }

        let ellipsis = "…";
        let ellipsis_width = UnicodeWidthStr::width(ellipsis);
        let mut display = String::new();
        let mut display_width = 0;
        for grapheme in name.graphemes(true) {
            let grapheme_width = UnicodeWidthStr::width(grapheme);
            if display_width + grapheme_width + ellipsis_width > MAX_WIDTH {
                break;
            }
            display.push_str(grapheme);
            display_width += grapheme_width;
        }
        display.push_str(ellipsis);
        display_width += ellipsis_width;
        display.push_str(&" ".repeat(MAX_WIDTH - display_width));
        display
    }

    /// 完了 bloom 用 reveal の色。Updated は green、Failed は red、Unchanged/Untracked は
    /// それぞれの状態色へ。Blocked は要確認を示す orange。fade_from はその色の暗いシェード
    /// にして、bloom が「同色から浮かび上がる」読み心地になるよう揃えている。
    fn completion_reveal_opts(status: &RepoStatus) -> Option<RevealOpts> {
        let (from, to) = match status {
            RepoStatus::Updated => (Rgb(20, 60, 20), Rgb(120, 220, 120)),
            RepoStatus::Failed => (Rgb(60, 20, 20), Rgb(220, 80, 80)),
            RepoStatus::Untracked => (Rgb(60, 20, 60), Rgb(220, 120, 220)),
            RepoStatus::Unchanged => (Rgb(50, 50, 50), Rgb(150, 150, 150)),
            RepoStatus::Blocked => (Rgb(60, 40, 10), Rgb(230, 160, 40)),
            RepoStatus::Waiting | RepoStatus::Running => return None,
        };
        Some(RevealOpts {
            char_interval: Duration::from_millis(18),
            fade_duration: Duration::from_millis(180),
            fade_from: from,
            fade_to: to,
        })
    }

    fn repo_name_modifier(is_selected: bool) -> Modifier {
        if is_selected {
            Modifier::BOLD | Modifier::REVERSED
        } else {
            Modifier::BOLD
        }
    }

    pub fn get_repos_handle(&self) -> Arc<Mutex<Vec<RepoProgress>>> {
        Arc::clone(&self.repos)
    }

    pub fn add_untracked(&self, name: String, path: String) {
        let mut repos = self.repos.lock().unwrap_or_else(|e| e.into_inner());
        repos.push(RepoProgress {
            name,
            path,
            status: RepoStatus::Untracked,
            message: "Not in gitpp.yaml".to_string(),
            progress: 100,
            output: String::new(),
        });
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
                        RepoStatus::Blocked => {
                            eprintln!("[{}] {}... blocked", self.command, repo.name);
                            reported.insert(repo.name.clone());
                        }
                        RepoStatus::Failed => {
                            eprintln!("[{}] {}... FAILED", self.command, repo.name);
                            reported.insert(repo.name.clone());
                        }
                        RepoStatus::Untracked => {
                            eprintln!("[{}] {}... untracked", self.command, repo.name);
                            reported.insert(repo.name.clone());
                        }
                        _ => {}
                    }
                }

                let all_done = repos.iter().all(|r| {
                    matches!(
                        r.status,
                        RepoStatus::Updated
                            | RepoStatus::Unchanged
                            | RepoStatus::Blocked
                            | RepoStatus::Failed
                            | RepoStatus::Untracked
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
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let res = self.run_app(&mut terminal);

        // Drain once before teardown, then return control to the shell and
        // drain again to drop any last queued events near the exit boundary.
        Self::drain_pending_events();
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        Write::flush(terminal.backend_mut())?;
        Self::drain_pending_events();
        disable_raw_mode()?;
        terminal.show_cursor()?;

        if let Err(err) = res {
            println!("{err:?}");
        }

        self.print_summary();
        io::stdout().flush()?;
        Self::drain_pending_events_for(Duration::from_millis(25));

        Ok(())
    }

    fn drain_pending_events() {
        while event::poll(Duration::ZERO).unwrap_or(false) {
            let _ = event::read();
        }
    }

    fn drain_pending_events_for(timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if !event::poll(remaining.min(Duration::from_millis(5))).unwrap_or(false) {
                break;
            }
            let _ = event::read();
        }
    }

    fn run_app<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> io::Result<()> {
        'main: loop {
            terminal.draw(|f| self.ui(f))?;

            let repos = self.repos.lock().unwrap_or_else(|e| e.into_inner());
            let all_done = repos.iter().all(|r| {
                matches!(
                    r.status,
                    RepoStatus::Updated
                        | RepoStatus::Unchanged
                        | RepoStatus::Blocked
                        | RepoStatus::Failed
                        | RepoStatus::Untracked
                )
            });
            drop(repos);

            if all_done {
                let deadline = Instant::now() + Duration::from_secs(3);
                loop {
                    // Keep ticking the renderer during the grace period so an
                    // in-flight completion bloom reaches its final frame.
                    terminal.draw(|f| self.ui(f))?;
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        break;
                    }
                    if let Some(code) =
                        Self::poll_key_press(remaining.min(Duration::from_millis(100)))?
                    {
                        match code {
                            KeyCode::Char('q') | KeyCode::Esc => break 'main,
                            _ => {
                                // User interacted, switch to browse mode
                                self.auto_exit_hint = false;
                                self.handle_key(code);
                                self.browse_mode(terminal)?;
                                break 'main;
                            }
                        }
                    }
                }
                break 'main;
            }

            if let Some(code) = Self::poll_key_press(Duration::from_millis(100))? {
                match code {
                    KeyCode::Char('q') => break,
                    KeyCode::Esc if !self.show_detail => break,
                    _ => self.handle_key(code),
                }
            }
        }

        Ok(())
    }

    fn browse_mode<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> io::Result<()> {
        loop {
            terminal.draw(|f| self.ui(f))?;

            if let Some(code) = Self::poll_key_press(Duration::from_millis(100))? {
                match code {
                    KeyCode::Char('q') => break,
                    KeyCode::Esc if !self.show_detail => break,
                    _ => self.handle_key(code),
                }
            }
        }
        Ok(())
    }

    /// Poll for a key press/repeat event, ignoring release events.
    /// Returns `Some(KeyCode)` on press/repeat, `None` on timeout or non-key event.
    fn poll_key_press(timeout: Duration) -> io::Result<Option<KeyCode>> {
        if event::poll(timeout)? {
            if let Event::Key(key) = event::read()? {
                if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                    return Ok(Some(key.code));
                }
            }
        }
        Ok(None)
    }

    fn handle_key(&mut self, key: KeyCode) {
        let repo_count = self.repos.lock().unwrap_or_else(|e| e.into_inner()).len();
        if repo_count == 0 {
            return;
        }

        match key {
            KeyCode::Char('j') | KeyCode::Down if self.selected + 1 < repo_count => {
                self.selected += 1;
                self.detail_scroll = 0;
            }
            KeyCode::Char('k') | KeyCode::Up if self.selected > 0 => {
                self.selected -= 1;
                self.detail_scroll = 0;
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
            Self::detail_text(repo)
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

    /// A repo needs the user's manual attention: either the command failed
    /// outright, or (pull-only) the ff-only merge was skipped and the user
    /// has to resolve a divergence/dirty tree themselves.
    fn needs_manual_attention(status: &RepoStatus) -> bool {
        matches!(status, RepoStatus::Failed | RepoStatus::Blocked)
    }

    fn jump_to_next_failed(&mut self, repo_count: usize) {
        let found = {
            let repos = self.repos.lock().unwrap_or_else(|e| e.into_inner());
            (1..repo_count)
                .map(|offset| (self.selected + offset) % repo_count)
                .find(|&idx| Self::needs_manual_attention(&repos[idx].status))
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
                .find(|&idx| Self::needs_manual_attention(&repos[idx].status))
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
        let blocked_count = repos
            .iter()
            .filter(|r| r.status == RepoStatus::Blocked)
            .count();
        let failed: Vec<_> = repos
            .iter()
            .filter(|r| r.status == RepoStatus::Failed)
            .collect();
        let untracked_count = repos
            .iter()
            .filter(|r| r.status == RepoStatus::Untracked)
            .count();

        let done = updated_count + unchanged_count + blocked_count + failed.len() + untracked_count;

        if failed.is_empty() {
            println!("Total: {total} | Done: {done} (Updated: {updated_count} / Unchanged: {unchanged_count} / Blocked: {blocked_count} / Failed: 0 / Untracked: {untracked_count})");
            return;
        }

        // Plain text, no ANSI codes — clipboard-friendly
        let failed_count = failed.len();
        println!(
            "Total: {total} | Done: {done} (Updated: {updated_count} / Unchanged: {unchanged_count} / Blocked: {blocked_count} / Failed: {failed_count} / Untracked: {untracked_count})\n"
        );
        for repo in &failed {
            println!("--- {} ({}) ---", repo.name, repo.path);
            let output = Self::detail_text(repo);
            for line in output.lines() {
                println!("  {line}");
            }
            println!();
        }
    }

    fn detail_text(repo: &RepoProgress) -> String {
        let raw = if repo.output.is_empty() {
            repo.message.as_str()
        } else {
            repo.output.as_str()
        };
        Self::sanitize_summary_text(raw)
    }

    fn sanitize_summary_text(text: &str) -> String {
        #[derive(Clone, Copy)]
        enum State {
            Text,
            Escape,
            Csi,
            Osc,
            OscEscape,
        }

        let mut state = State::Text;
        let mut out = String::with_capacity(text.len());
        let mut pending_cr = false;

        for ch in text.chars() {
            if pending_cr {
                if ch == '\n' {
                    out.push('\n');
                    pending_cr = false;
                    continue;
                }
                out.push('\n');
                pending_cr = false;
            }

            state = match state {
                State::Text => {
                    if ch == '\u{1b}' {
                        State::Escape
                    } else {
                        match ch {
                            '\r' => pending_cr = true,
                            '\n' | '\t' => out.push(ch),
                            _ if !ch.is_control() => out.push(ch),
                            _ => {}
                        }
                        State::Text
                    }
                }
                State::Escape => match ch {
                    '[' => State::Csi,
                    ']' => State::Osc,
                    _ => {
                        if !ch.is_control() || matches!(ch, '\n' | '\t') {
                            out.push(ch);
                        }
                        State::Text
                    }
                },
                State::Csi => {
                    if ('@'..='~').contains(&ch) {
                        State::Text
                    } else {
                        State::Csi
                    }
                }
                State::Osc => match ch {
                    '\u{7}' => State::Text,
                    '\u{1b}' => State::OscEscape,
                    _ => State::Osc,
                },
                State::OscEscape => match ch {
                    '\\' => State::Text,
                    '\u{1b}' => State::OscEscape,
                    _ => State::Osc,
                },
            };
        }

        if pending_cr {
            out.push('\n');
        }

        out
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

        // Status message (auto-expires after 3 seconds) — appended to line 1
        let show_msg = match &self.status_message {
            Some((msg, at)) if at.elapsed() < Duration::from_secs(3) => Some(msg.clone()),
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
        let blocked = repos
            .iter()
            .filter(|r| r.status == RepoStatus::Blocked)
            .count();
        let failed = repos
            .iter()
            .filter(|r| r.status == RepoStatus::Failed)
            .count();
        let untracked = repos
            .iter()
            .filter(|r| r.status == RepoStatus::Untracked)
            .count();
        let done = updated + unchanged + blocked + failed + untracked;
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
            Span::styled("Blocked: ", Style::default().fg(Color::White)),
            Span::styled(
                format!("{blocked}"),
                Style::default().fg(Color::Rgb(230, 160, 40)),
            ),
            Span::raw(" / "),
            Span::styled("Failed: ", Style::default().fg(Color::White)),
            Span::styled(format!("{failed}"), Style::default().fg(Color::Red)),
            Span::raw(" / "),
            Span::styled("Untracked: ", Style::default().fg(Color::White)),
            Span::styled(format!("{untracked}"), Style::default().fg(Color::Magenta)),
            Span::raw(")"),
        ]);

        let mut footer_lines = vec![stats_line];
        if self.auto_exit_hint {
            footer_lines.push(Line::from(Span::styled(
                "Will auto-exit 3s after completion — press any key to browse",
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

        // Sync the completion bloom lifecycle before rendering. We anchor every
        // new reveal at the same `now` so co-completing repos bloom in unison
        // rather than staggered.
        let now = Instant::now();
        let current_paths: HashSet<String> = repos.iter().map(|repo| repo.path.clone()).collect();
        self.name_reveals
            .retain(|path, _| current_paths.contains(path));
        for repo in repos.iter() {
            let Some(opts) = Self::completion_reveal_opts(&repo.status) else {
                // A new operation starts from Waiting/Running. Remove the old
                // handle so its next terminal state can bloom again.
                self.name_reveals.remove(&repo.path);
                continue;
            };
            let needs_new_reveal = self
                .name_reveals
                .get(&repo.path)
                .map_or(true, |reveal| reveal.status != repo.status);
            if needs_new_reveal {
                let display = Self::format_repo_name(&repo.name);
                self.name_reveals.insert(
                    repo.path.clone(),
                    NameReveal {
                        handle: RevealHandle::start_at(display.trim_end(), opts, now),
                        status: repo.status,
                    },
                );
            }
        }

        // Each repo takes 2 lines (status + progress bar), no blank line between
        let lines_per_repo = 2;
        // A pane has two border cells and one padding cell on each vertical side.
        // Keep this calculation in sync with the Block below; saturating_sub keeps
        // a tiny terminal from underflowing while the pane has no content area.
        let visible_height = area.height.saturating_sub(PANE_CONTENT_OVERHEAD) as usize;
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
                RepoStatus::Blocked => ("⚠", Color::Rgb(230, 160, 40)),
                RepoStatus::Failed => ("✗", Color::Red),
                RepoStatus::Untracked => ("?", Color::Magenta),
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

            let mut spans = vec![Span::styled(
                format!("{selector}{status_icon} "),
                Style::default()
                    .fg(status_color)
                    .add_modifier(Modifier::BOLD),
            )];
            let display_name = Self::format_repo_name(&repo.name);
            let bloom = self
                .name_reveals
                .get(&repo.path)
                .filter(|reveal| !reveal.handle.is_done(now));
            if let Some(reveal) = bloom {
                // Bloom in progress: per-grapheme colored spans, then pad to width.
                let snap = reveal.handle.snapshot(now);
                let trimmed = display_name.trim_end();
                let consumed = snap.len();
                for g in &snap {
                    spans.push(Span::styled(
                        g.text.clone(),
                        Style::default()
                            .fg(Color::Rgb(g.color.0, g.color.1, g.color.2))
                            .add_modifier(Self::repo_name_modifier(is_selected)),
                    ));
                }
                let graphemes: Vec<&str> = trimmed.graphemes(true).collect();
                if consumed < graphemes.len() {
                    // Graphemes not yet revealed — keep their slots so width doesn't
                    // shift between frames. Render as dim placeholders.
                    let placeholder: String = graphemes.iter().skip(consumed).copied().collect();
                    spans.push(Span::styled(
                        placeholder,
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Self::repo_name_modifier(is_selected)),
                    ));
                }
                let trailing_width = UnicodeWidthStr::width(display_name.as_str())
                    .saturating_sub(UnicodeWidthStr::width(trimmed));
                if trailing_width > 0 {
                    spans.push(Span::styled(
                        " ".repeat(trailing_width),
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Self::repo_name_modifier(is_selected)),
                    ));
                }
            } else {
                spans.push(Span::styled(display_name, name_style));
            }
            spans.push(Span::styled(
                format!(" {}", repo.message),
                Style::default().fg(Color::White),
            ));
            lines.push(Line::from(spans));

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
                    RepoStatus::Blocked => Color::Rgb(230, 160, 40),
                    RepoStatus::Failed => Color::Red,
                    RepoStatus::Running => Color::Yellow,
                    RepoStatus::Waiting => Color::DarkGray,
                    RepoStatus::Untracked => Color::Magenta,
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
                .padding(Padding::uniform(PANE_PADDING))
                .title(scroll_info)
                .style(Style::default()),
        );

        f.render_widget(paragraph, area);
    }

    fn detail_line_count(&self) -> usize {
        let repos = self.repos.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(repo) = repos.get(self.selected) {
            let text = Self::detail_text(repo);
            text.lines().count()
        } else {
            0
        }
    }

    fn render_detail(&self, f: &mut Frame, area: Rect) {
        let repos = self.repos.lock().unwrap_or_else(|e| e.into_inner());

        let (title, content) = if let Some(repo) = repos.get(self.selected) {
            let title = format!(" {} ", repo.name);
            let text = Self::detail_text(repo);
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
                    .padding(Padding::uniform(PANE_PADDING))
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

#[cfg(test)]
mod tests {
    use super::{update_repo_status, RepoStatus, TuiApp};
    use jiwa::Rgb;
    use ratatui::{
        backend::TestBackend,
        style::{Color, Modifier},
        Terminal,
    };
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn sanitize_summary_text_removes_ansi_sequences() {
        let text = "fatal:\x1b[31m boom\x1b[0m\n";
        assert_eq!(TuiApp::sanitize_summary_text(text), "fatal: boom\n");
    }

    #[test]
    fn sanitize_summary_text_removes_osc_sequences() {
        let text = "before\x1b]8;;https://example.com\x07link\x1b]8;;\x07after";
        assert_eq!(TuiApp::sanitize_summary_text(text), "beforelinkafter");
    }

    #[test]
    fn sanitize_summary_text_keeps_whitespace_but_drops_other_controls() {
        let text = "line 1\u{8}\n\tline 2\r\n";
        assert_eq!(TuiApp::sanitize_summary_text(text), "line 1\n\tline 2\n");
    }

    #[test]
    fn sanitize_summary_text_normalizes_bare_carriage_returns() {
        let text = "step 1\rstep 2\rstep 3";
        assert_eq!(
            TuiApp::sanitize_summary_text(text),
            "step 1\nstep 2\nstep 3"
        );
    }

    #[test]
    fn sanitize_summary_text_keeps_plain_text_after_unknown_escape() {
        let text = "before\x1bXafter";
        assert_eq!(TuiApp::sanitize_summary_text(text), "beforeXafter");
    }

    #[test]
    fn pane_body_has_one_cell_inner_padding() {
        let mut app = TuiApp::new(vec!["repo".into()], vec!["/tmp/repo".into()], "status");
        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).expect("test terminal must be created");
        terminal
            .draw(|frame| app.ui(frame))
            .expect("TUI must render to a test backend");

        let buffer = terminal.backend().buffer();
        // The outer margin puts the main panes at x=1, y=5. The pane border is
        // at x=1 and the one-cell padding is x=2, so the first list glyph is x=3.
        assert_eq!(buffer.cell((1, 5)).unwrap().symbol(), "┌");
        assert_eq!(buffer.cell((2, 7)).unwrap().symbol(), " ");
        assert_eq!(buffer.cell((3, 7)).unwrap().symbol(), "▸");

        // The detail pane starts after the first 50% column. Its body follows
        // the same border + padding contract as the list pane.
        assert_eq!(buffer.cell((40, 5)).unwrap().symbol(), "┌");
        assert_eq!(buffer.cell((41, 7)).unwrap().symbol(), " ");
        assert_eq!(buffer.cell((42, 7)).unwrap().symbol(), "W");
    }

    #[test]
    fn narrow_terminal_renders_without_panicking() {
        let mut app = TuiApp::new(vec!["repo".into()], vec!["/tmp/repo".into()], "status");
        let backend = TestBackend::new(8, 7);
        let mut terminal = Terminal::new(backend).expect("test terminal must be created");
        terminal
            .draw(|frame| app.ui(frame))
            .expect("TUI must tolerate a terminal smaller than its pane content");
    }

    #[test]
    fn format_repo_name_truncates_non_ascii_without_panicking() {
        let formatted = TuiApp::format_repo_name("東京👨‍👩‍👧‍👦特許許可局の長いリポジトリ名さらに長い");

        assert_eq!(UnicodeWidthStr::width(formatted.as_str()), 36);
        assert!(formatted.trim_end().ends_with('…'));
        assert!(formatted
            .trim_end()
            .graphemes(true)
            .all(|grapheme| !grapheme.is_empty()));
    }

    #[test]
    fn bloom_map_is_bounded_and_rearmed_after_running() {
        let mut app = TuiApp::new(
            vec!["one".into(), "two".into()],
            vec!["/tmp/one".into(), "/tmp/two".into()],
            "status",
        );
        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).expect("test terminal must be created");
        let handle = app.get_repos_handle();

        update_repo_status(&handle, "one", RepoStatus::Updated, "Updated", 100);
        update_repo_status(&handle, "two", RepoStatus::Failed, "Failed", 100);
        terminal.draw(|frame| app.ui(frame)).unwrap();
        assert_eq!(app.name_reveals.len(), 2);

        update_repo_status(&handle, "one", RepoStatus::Running, "Running", 50);
        terminal.draw(|frame| app.ui(frame)).unwrap();
        assert_eq!(app.name_reveals.len(), 1);
        assert!(!app.name_reveals.contains_key("/tmp/one"));

        update_repo_status(&handle, "one", RepoStatus::Unchanged, "Unchanged", 100);
        terminal.draw(|frame| app.ui(frame)).unwrap();
        assert_eq!(app.name_reveals.len(), 2);
        assert_eq!(app.name_reveals["/tmp/one"].status, RepoStatus::Unchanged);

        let mut repos = handle.lock().unwrap();
        repos.retain(|repo| repo.path == "/tmp/one");
        drop(repos);
        terminal.draw(|frame| app.ui(frame)).unwrap();
        assert_eq!(app.name_reveals.len(), 1);
        assert!(app.name_reveals.contains_key("/tmp/one"));
    }

    #[test]
    fn selected_bloom_keeps_reversed_modifier() {
        let mut app = TuiApp::new(vec!["repo".into()], vec!["/tmp/repo".into()], "status");
        let handle = app.get_repos_handle();
        update_repo_status(&handle, "repo", RepoStatus::Updated, "Updated", 100);

        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).expect("test terminal must be created");
        terminal.draw(|frame| app.ui(frame)).unwrap();

        // Main pane starts at x=1; border + padding + selector/status prefix
        // place the first blooming name grapheme at x=6.
        let cell = terminal.backend().buffer().cell((6, 7)).unwrap();
        assert!(
            cell.modifier.contains(Modifier::REVERSED),
            "selected rows must stay reversed while their name blooms"
        );
    }

    #[test]
    fn completion_bloom_reaches_final_frame_when_redrawn() {
        let mut app = TuiApp::new(vec!["r".into()], vec!["/tmp/repo".into()], "status");
        let handle = app.get_repos_handle();
        update_repo_status(&handle, "r", RepoStatus::Updated, "Updated", 100);

        let backend = TestBackend::new(80, 20);
        let mut terminal = Terminal::new(backend).expect("test terminal must be created");
        terminal.draw(|frame| app.ui(frame)).unwrap();
        let initial_fg = terminal.backend().buffer().cell((6, 7)).unwrap().fg;
        assert_ne!(
            initial_fg,
            Color::Cyan,
            "first frame should be the bloom shade"
        );

        // The one-grapheme name has a 180 ms fade. A subsequent frame must
        // render the stable name style, matching the run_app grace-period loop.
        thread::sleep(Duration::from_millis(220));
        terminal.draw(|frame| app.ui(frame)).unwrap();
        assert_eq!(
            terminal.backend().buffer().cell((6, 7)).unwrap().fg,
            Color::Cyan
        );
    }

    // --- completion_reveal_opts: Blocked gets its own orange reveal ---------

    #[test]
    fn completion_reveal_opts_blocked_is_orange() {
        let opts = TuiApp::completion_reveal_opts(&RepoStatus::Blocked)
            .expect("Blocked must have a completion reveal, like every other terminal status");
        assert_eq!(
            opts.fade_to,
            Rgb(230, 160, 40),
            "Blocked's reveal must fade to the same orange used elsewhere in the UI"
        );
    }

    // --- run_quiet: Blocked counts as done, alone and mixed with Failed -----

    /// Build a `TuiApp` with the given (name, path, status) rows already set,
    /// bypassing the Waiting default `TuiApp::new` starts every repo at.
    fn app_with_statuses(rows: &[(&str, &str, RepoStatus)]) -> TuiApp {
        let names = rows.iter().map(|(n, _, _)| n.to_string()).collect();
        let paths = rows.iter().map(|(_, p, _)| p.to_string()).collect();
        let app = TuiApp::new(names, paths, "pull");
        let handle = app.get_repos_handle();
        for (name, _, status) in rows {
            update_repo_status(&handle, name, *status, "done", 100);
        }
        app
    }

    /// A regression here (Blocked dropped from run_quiet's `all_done` check)
    /// would make run_quiet spin forever instead of failing a single
    /// assertion, so arm a watchdog that force-interrupts the loop well
    /// after any correct run would already have finished.
    fn run_quiet_with_watchdog(app: &mut TuiApp) -> Duration {
        let interrupted = Arc::new(AtomicBool::new(false));
        let watchdog_flag = Arc::clone(&interrupted);
        thread::spawn(move || {
            thread::sleep(Duration::from_secs(2));
            watchdog_flag.store(true, Ordering::Relaxed);
        });

        let start = Instant::now();
        app.run_quiet(interrupted)
            .expect("run_quiet must not error");
        start.elapsed()
    }

    #[test]
    fn run_quiet_treats_blocked_only_repo_as_done() {
        let mut app = app_with_statuses(&[("a", "/tmp/a", RepoStatus::Blocked)]);
        let elapsed = run_quiet_with_watchdog(&mut app);
        assert!(
            elapsed < Duration::from_secs(1),
            "a Blocked-only repo must be recognized as done immediately, not require the watchdog: {elapsed:?}"
        );
    }

    #[test]
    fn run_quiet_treats_blocked_and_failed_mix_as_done() {
        let mut app = app_with_statuses(&[
            ("a", "/tmp/a", RepoStatus::Blocked),
            ("b", "/tmp/b", RepoStatus::Failed),
        ]);
        let elapsed = run_quiet_with_watchdog(&mut app);
        assert!(
            elapsed < Duration::from_secs(1),
            "a Blocked+Failed mix must be recognized as fully done, not require the watchdog: {elapsed:?}"
        );
    }

    // --- n/N jump: Blocked repos need manual follow-up too ------------------

    #[test]
    fn jump_to_next_failed_also_finds_blocked_only_repos() {
        let mut app = app_with_statuses(&[
            ("a", "/tmp/a", RepoStatus::Unchanged),
            ("b", "/tmp/b", RepoStatus::Blocked),
            ("c", "/tmp/c", RepoStatus::Unchanged),
        ]);
        app.selected = 0;
        app.jump_to_next_failed(3);
        assert_eq!(
            app.selected, 1,
            "n must jump to a Blocked-only repo, not just a Failed one"
        );
    }

    #[test]
    fn jump_to_prev_failed_also_finds_blocked_only_repos() {
        let mut app = app_with_statuses(&[
            ("a", "/tmp/a", RepoStatus::Unchanged),
            ("b", "/tmp/b", RepoStatus::Blocked),
            ("c", "/tmp/c", RepoStatus::Unchanged),
        ]);
        app.selected = 2;
        app.jump_to_prev_failed(3);
        assert_eq!(
            app.selected, 1,
            "N must jump backward to a Blocked-only repo too"
        );
    }

    #[test]
    fn jump_to_next_failed_finds_both_failed_and_blocked_in_mixed_list() {
        let mut app = app_with_statuses(&[
            ("a", "/tmp/a", RepoStatus::Unchanged),
            ("b", "/tmp/b", RepoStatus::Blocked),
            ("c", "/tmp/c", RepoStatus::Unchanged),
            ("d", "/tmp/d", RepoStatus::Failed),
        ]);
        app.selected = 0;

        app.jump_to_next_failed(4);
        assert_eq!(app.selected, 1, "first jump lands on the Blocked repo");

        app.jump_to_next_failed(4);
        assert_eq!(app.selected, 3, "second jump lands on the Failed repo");
    }
}
