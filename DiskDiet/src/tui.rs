//! Interactive picker. Results stream in while the scan runs; deletions run in
//! the background so the UI never freezes.

use std::collections::HashMap;
use std::io;
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Cell, Clear, Paragraph, Row, Table, TableState, Wrap};
use ratatui::{DefaultTerminal, Frame};

use crate::output::tilde;
use crate::scan::{self, Entry, Options, Progress, Source};
use crate::units::count as fmt_count;
use crate::{Filter, delete, units};

/// A finished deletion: item index and outcome.
type DelMsg = (usize, Result<(), String>);

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const AGES: [(&str, u64); 4] = [
    ("any age", 0),
    ("> 30 days", 30),
    ("> 90 days", 90),
    ("> 1 year", 365),
];
const ACCENT: Color = Color::Rgb(134, 239, 172);

#[derive(Clone, Copy, PartialEq)]
enum Sort {
    Size,
    Age,
    Path,
}

impl Sort {
    fn next(self) -> Self {
        match self {
            Sort::Size => Sort::Age,
            Sort::Age => Sort::Path,
            Sort::Path => Sort::Size,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Sort::Size => "size",
            Sort::Age => "age",
            Sort::Path => "path",
        }
    }
}

#[derive(Clone, PartialEq)]
enum State {
    Ready,
    Deleting,
    Deleted,
    Failed(String),
}

struct Item {
    entry: Entry,
    shown_path: String,
    selected: bool,
    state: State,
}

enum Mode {
    Browse,
    Search,
    Confirm(Vec<usize>),
}

struct App {
    items: Vec<Item>,
    view: Vec<usize>,
    table: TableState,
    sort: Sort,
    age_idx: usize,
    query: String,
    mode: Mode,
    filter: Filter,
    progress: Arc<Progress>,
    started: Instant,
    scan_time: Option<Duration>,
    roots_label: String,
    freed: u64,
    deleting: usize,
    tick: usize,
    flash: Option<(String, Instant)>,
    dirty: bool,
    /// Until the user moves, the cursor stays on the top row while results
    /// stream in, instead of following whichever folder arrived first.
    moved: bool,
    quit: bool,
    del_tx: mpsc::Sender<DelMsg>,
}

/// Runs the UI until the user quits. Returns the number of bytes freed.
pub fn run(opts: Options, filter: Filter) -> io::Result<u64> {
    let (tx, rx) = mpsc::channel();
    let (del_tx, del_rx) = mpsc::channel();
    let progress = Arc::new(Progress::default());
    let roots_label = if opts.projects {
        opts.roots
            .iter()
            .map(|p| tilde(p))
            .collect::<Vec<_>>()
            .join(", ")
    } else {
        "global caches".into()
    };
    {
        let progress = progress.clone();
        thread::spawn(move || scan::run(&opts, &progress, &tx));
    }

    let mut app = App::new(filter, progress, roots_label, del_tx);

    let mut terminal = ratatui::try_init()?;
    let result = app.event_loop(&mut terminal, &rx, &del_rx);
    ratatui::restore();
    result?;

    if app.deleting > 0 {
        eprintln!("Finishing {} deletion(s)…", app.deleting);
        while app.deleting > 0 {
            match del_rx.recv() {
                Ok(msg) => app.finish_delete(msg),
                Err(_) => break,
            }
        }
    }
    for it in app.items.iter() {
        if let State::Failed(err) = &it.state {
            eprintln!("  could not delete {}: {err}", it.shown_path);
        }
    }
    Ok(app.freed)
}

impl App {
    fn new(
        filter: Filter,
        progress: Arc<Progress>,
        roots_label: String,
        del_tx: mpsc::Sender<DelMsg>,
    ) -> Self {
        App {
            items: Vec::new(),
            view: Vec::new(),
            table: TableState::default().with_selected(Some(0)),
            sort: Sort::Size,
            age_idx: 0,
            query: String::new(),
            mode: Mode::Browse,
            filter,
            progress,
            started: Instant::now(),
            scan_time: None,
            roots_label,
            freed: 0,
            deleting: 0,
            tick: 0,
            flash: None,
            dirty: false,
            moved: false,
            quit: false,
            del_tx,
        }
    }

    fn push(&mut self, entry: Entry) {
        if self.filter.keep(&entry) {
            let shown_path = tilde(&entry.path);
            self.items.push(Item {
                entry,
                shown_path,
                selected: false,
                state: State::Ready,
            });
            self.dirty = true;
        }
    }

    fn event_loop(
        &mut self,
        terminal: &mut DefaultTerminal,
        rx: &mpsc::Receiver<Entry>,
        del_rx: &mpsc::Receiver<DelMsg>,
    ) -> io::Result<()> {
        let mut redraw = true;
        while !self.quit {
            for entry in rx.try_iter() {
                self.push(entry);
                redraw = true;
            }
            for msg in del_rx.try_iter() {
                self.finish_delete(msg);
                redraw = true;
            }
            if self.scan_time.is_none() && self.progress.done.load(Ordering::Acquire) {
                self.scan_time = Some(self.started.elapsed());
                redraw = true;
            }
            if self.dirty {
                self.rebuild_view();
                redraw = true;
            }
            if self
                .flash
                .as_ref()
                .is_some_and(|(_, at)| at.elapsed() >= Duration::from_secs(3))
            {
                self.flash = None;
                redraw = true;
            }

            // While scanning or deleting, the spinner animates. Otherwise the
            // screen is only redrawn when something changes, so an idle
            // diskdiet uses no CPU.
            let busy = self.scan_time.is_none() || self.deleting > 0;
            if redraw || busy {
                terminal.draw(|f| self.render(f))?;
                self.tick = self.tick.wrapping_add(1);
                redraw = false;
            }

            let wait = if busy {
                80
            } else if self.flash.is_some() {
                250
            } else {
                1000
            };
            if event::poll(Duration::from_millis(wait))? {
                match event::read()? {
                    Event::Key(key) if key.kind != KeyEventKind::Release => {
                        self.on_key(key.code, key.modifiers);
                        redraw = true;
                    }
                    Event::Resize(..) => redraw = true,
                    _ => {}
                }
            }
        }
        Ok(())
    }

    fn finish_delete(&mut self, (idx, result): DelMsg) {
        self.deleting = self.deleting.saturating_sub(1);
        let it = &mut self.items[idx];
        match result {
            Ok(()) => {
                self.freed += it.entry.bytes;
                it.state = State::Deleted;
            }
            Err(e) => it.state = State::Failed(e),
        }
        it.selected = false;
    }

    // ── state ────────────────────────────────────────────────────────────

    fn current(&self) -> Option<usize> {
        self.table
            .selected()
            .and_then(|i| self.view.get(i).copied())
    }

    fn rebuild_view(&mut self) {
        self.dirty = false;
        let keep = if self.moved { self.current() } else { None };
        let min_age = Duration::from_secs(AGES[self.age_idx].1 * 86_400);
        let q = self.query.to_lowercase();
        let items = &self.items;
        let mut view: Vec<usize> = (0..items.len())
            .filter(|&i| {
                let it = &items[i];
                (min_age.is_zero()
                    || (it.entry.source == Source::Project
                        && units::age(it.entry.last_modified).is_some_and(|a| a >= min_age)))
                    && (q.is_empty()
                        || it.shown_path.to_lowercase().contains(&q)
                        || it.entry.ecosystem.contains(&q))
            })
            .collect();
        match self.sort {
            Sort::Size => view.sort_by(|&a, &b| items[b].entry.bytes.cmp(&items[a].entry.bytes)),
            Sort::Age => view.sort_by(|&a, &b| {
                items[a]
                    .entry
                    .last_modified
                    .cmp(&items[b].entry.last_modified)
            }),
            Sort::Path => view.sort_by(|&a, &b| items[a].shown_path.cmp(&items[b].shown_path)),
        }
        let pos = keep
            .and_then(|k| view.iter().position(|&i| i == k))
            .unwrap_or_else(|| {
                self.table
                    .selected()
                    .unwrap_or(0)
                    .min(view.len().saturating_sub(1))
            });
        self.view = view;
        self.table.select(Some(pos));
    }

    fn deletable(&self, i: usize) -> bool {
        matches!(self.items[i].state, State::Ready | State::Failed(_))
    }

    fn selection(&self) -> Vec<usize> {
        (0..self.items.len())
            .filter(|&i| self.items[i].selected && self.deletable(i))
            .collect()
    }

    fn total(&self, ids: impl Iterator<Item = usize>) -> u64 {
        ids.map(|i| self.items[i].entry.bytes).sum()
    }

    fn say(&mut self, msg: impl Into<String>) {
        self.flash = Some((msg.into(), Instant::now()));
    }

    fn move_by(&mut self, delta: isize) {
        if self.view.is_empty() {
            return;
        }
        self.moved = true;
        let cur = self.table.selected().unwrap_or(0) as isize;
        let next = (cur + delta).clamp(0, self.view.len() as isize - 1);
        self.table.select(Some(next as usize));
    }

    // ── input ────────────────────────────────────────────────────────────

    fn on_key(&mut self, code: KeyCode, mods: KeyModifiers) {
        if mods.contains(KeyModifiers::CONTROL) && code == KeyCode::Char('c') {
            self.quit = true;
            return;
        }
        match &self.mode {
            Mode::Search => match code {
                KeyCode::Enter => self.mode = Mode::Browse,
                KeyCode::Esc => {
                    self.query.clear();
                    self.mode = Mode::Browse;
                    self.dirty = true;
                }
                KeyCode::Backspace => {
                    self.query.pop();
                    self.dirty = true;
                }
                KeyCode::Char(c) => {
                    self.query.push(c);
                    self.dirty = true;
                }
                _ => {}
            },
            Mode::Confirm(ids) => match code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    let ids = ids.clone();
                    self.mode = Mode::Browse;
                    self.start_delete(ids);
                }
                _ => {
                    self.mode = Mode::Browse;
                    self.say("Cancelled — nothing deleted");
                }
            },
            Mode::Browse => self.on_browse_key(code),
        }
    }

    fn on_browse_key(&mut self, code: KeyCode) {
        let page = 10;
        match code {
            KeyCode::Char('q') | KeyCode::Esc => {
                if !self.query.is_empty() && code == KeyCode::Esc {
                    self.query.clear();
                    self.dirty = true;
                } else {
                    self.quit = true;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::PageDown | KeyCode::Char('J') => self.move_by(page),
            KeyCode::PageUp | KeyCode::Char('K') => self.move_by(-page),
            KeyCode::Home | KeyCode::Char('g') => {
                self.moved = true;
                self.table.select(Some(0));
            }
            KeyCode::End | KeyCode::Char('G') => {
                self.moved = true;
                self.table.select(Some(self.view.len().saturating_sub(1)));
            }
            KeyCode::Char(' ') => {
                if let Some(i) = self.current()
                    && self.deletable(i)
                {
                    self.items[i].selected = !self.items[i].selected;
                }
                self.move_by(1);
            }
            KeyCode::Char('a') => {
                let visible: Vec<usize> = self
                    .view
                    .iter()
                    .copied()
                    .filter(|&i| self.deletable(i))
                    .collect();
                let all = visible.iter().all(|&i| self.items[i].selected);
                for i in visible {
                    self.items[i].selected = !all;
                }
            }
            KeyCode::Char('s') => {
                self.sort = self.sort.next();
                self.dirty = true;
            }
            KeyCode::Char('t') => {
                self.age_idx = (self.age_idx + 1) % AGES.len();
                self.dirty = true;
            }
            KeyCode::Char('/') => self.mode = Mode::Search,
            KeyCode::Char('o') => self.open_current(),
            KeyCode::Char('d') | KeyCode::Delete | KeyCode::Backspace => {
                let mut ids = self.selection();
                if ids.is_empty()
                    && let Some(i) = self.current()
                    && self.deletable(i)
                {
                    ids.push(i);
                }
                if ids.is_empty() {
                    self.say("Nothing selected — press space to pick folders");
                } else {
                    self.mode = Mode::Confirm(ids);
                }
            }
            _ => {}
        }
    }

    fn start_delete(&mut self, ids: Vec<usize>) {
        let jobs: Vec<(usize, std::path::PathBuf)> = ids
            .iter()
            .map(|&i| {
                self.items[i].state = State::Deleting;
                (i, self.items[i].entry.path.clone())
            })
            .collect();
        self.deleting += jobs.len();
        let tx = self.del_tx.clone();
        thread::spawn(move || {
            use rayon::prelude::*;
            jobs.into_par_iter().for_each_with(tx, |tx, (i, path)| {
                let _ = tx.send((i, delete::remove(&path).map_err(|e| e.to_string())));
            });
        });
    }

    fn open_current(&mut self) {
        let Some(i) = self.current() else { return };
        let path = &self.items[i].entry.path;
        let target = path.parent().unwrap_or(path);
        let program = if cfg!(target_os = "macos") {
            "open"
        } else if cfg!(windows) {
            "explorer"
        } else {
            "xdg-open"
        };
        let ok = Command::new(program)
            .arg(target)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok();
        let shown = tilde(target);
        self.say(if ok {
            format!("Opened {shown}")
        } else {
            format!("Could not run `{program}`")
        });
    }

    // ── drawing ──────────────────────────────────────────────────────────

    fn render(&mut self, f: &mut Frame) {
        let [header, body, detail, footer] = Layout::vertical([
            Constraint::Length(2),
            Constraint::Fill(1),
            Constraint::Length(2),
            Constraint::Length(1),
        ])
        .areas(f.area());

        self.render_header(f, header);
        let side = 34;
        if body.width >= 100 {
            let [list, panel] =
                Layout::horizontal([Constraint::Fill(1), Constraint::Length(side)]).areas(body);
            self.render_table(f, list);
            self.render_breakdown(f, panel);
        } else {
            self.render_table(f, body);
        }
        self.render_detail(f, detail);
        self.render_footer(f, footer);
        if let Mode::Confirm(ids) = &self.mode {
            self.render_confirm(f, ids);
        }
    }

    fn render_header(&self, f: &mut Frame, area: Rect) {
        let dirs = self.progress.dirs.load(Ordering::Relaxed);
        let status = |label: &str| match self.scan_time {
            None => format!(
                " {} scanning {label} · {} folders",
                SPINNER[self.tick % SPINNER.len()],
                fmt_count(dirs)
            ),
            Some(t) => format!(
                " scanned {label} · {} folders in {:.1}s",
                fmt_count(dirs),
                t.as_secs_f64()
            ),
        };
        let status_style = if self.scan_time.is_none() {
            Style::new().fg(Color::Yellow)
        } else {
            Style::new().dim()
        };

        let live = (0..self.items.len()).filter(|&i| self.items[i].state != State::Deleted);
        let found = self.total(live);
        let sel = self.selection();
        let mut right = vec![
            Span::raw("found "),
            Span::styled(units::bytes(found), Style::new().bold()),
            Span::raw("  ·  selected "),
            Span::styled(
                format!(
                    "{} ({})",
                    units::bytes(self.total(sel.iter().copied())),
                    sel.len()
                ),
                Style::new().fg(Color::Cyan).bold(),
            ),
        ];
        if self.freed > 0 || self.deleting > 0 {
            right.push(Span::raw("  ·  freed "));
            right.push(Span::styled(
                units::bytes(self.freed),
                Style::new().fg(ACCENT).bold(),
            ));
            if self.deleting > 0 {
                right.push(Span::styled(
                    format!(
                        " {} {} left",
                        SPINNER[self.tick % SPINNER.len()],
                        self.deleting
                    ),
                    Style::new().fg(Color::Yellow),
                ));
            }
        }
        right.push(Span::raw(" "));

        // Shorten the scanned path so it never runs into the totals.
        const BADGE: &str = " diskdiet ";
        let right_w = Line::from(right.clone()).width();
        let room = (area.width as usize).saturating_sub(right_w + BADGE.len() + 2);
        let fixed = status("").chars().count();
        let text = if room >= fixed + 6 {
            status(&truncate_left(&self.roots_label, room - fixed))
        } else if room >= fixed {
            status("")
        } else {
            String::new()
        };
        let left = Line::from(vec![
            Span::styled(BADGE, Style::new().fg(Color::Black).bg(ACCENT).bold()),
            Span::styled(text, status_style),
        ]);
        f.render_widget(Paragraph::new(left), area);
        f.render_widget(
            Paragraph::new(Line::from(right)).alignment(Alignment::Right),
            area,
        );
    }

    fn render_table(&mut self, f: &mut Frame, area: Rect) {
        let fixed = 3 + 10 + 7 + 11 + 2 + 4; // columns + spacing + borders/highlight
        let path_width = (area.width as usize).saturating_sub(fixed).max(10);
        let rows: Vec<Row> = self
            .view
            .iter()
            .map(|&i| {
                let it = &self.items[i];
                let (mark, mark_style) = match &it.state {
                    State::Deleted => ("✓", Style::new().fg(ACCENT)),
                    State::Deleting => (
                        SPINNER[self.tick % SPINNER.len()],
                        Style::new().fg(Color::Yellow),
                    ),
                    State::Failed(_) => ("!", Style::new().fg(Color::Red).bold()),
                    State::Ready if it.selected => ("●", Style::new().fg(Color::Cyan).bold()),
                    State::Ready => ("○", Style::new().dim()),
                };
                let size_style = match it.entry.bytes {
                    b if b >= 1_000_000_000 => Style::new().fg(Color::LightRed).bold(),
                    b if b >= 100_000_000 => Style::new().fg(Color::Yellow),
                    _ => Style::new(),
                };
                let age = match it.entry.source {
                    Source::Project => units::short_age(it.entry.last_modified),
                    Source::Cache => "cache".into(),
                };
                let mut row = Row::new(vec![
                    Cell::from(Span::styled(mark, mark_style)),
                    Cell::from(
                        Line::from(Span::styled(units::bytes(it.entry.bytes), size_style))
                            .right_aligned(),
                    ),
                    Cell::from(Line::from(age).right_aligned()).style(Style::new().dim()),
                    Cell::from(it.entry.ecosystem)
                        .style(Style::new().fg(eco_color(it.entry.ecosystem))),
                    Cell::from(truncate_left(&it.shown_path, path_width)),
                ]);
                if it.state == State::Deleted {
                    row = row.style(Style::new().dim().add_modifier(Modifier::CROSSED_OUT));
                } else if it.selected {
                    row = row.style(Style::new().fg(Color::Cyan));
                }
                row
            })
            .collect();

        let title = {
            let mut t = format!(" sort: {} · {} ", self.sort.label(), AGES[self.age_idx].0);
            if !self.query.is_empty() || matches!(self.mode, Mode::Search) {
                t.push_str(&format!(
                    "· filter: {}{} ",
                    self.query,
                    if matches!(self.mode, Mode::Search) {
                        "▏"
                    } else {
                        ""
                    }
                ));
            }
            t
        };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().dim())
            .title(Line::from(title).bold())
            .title_bottom(
                Line::from(format!(" {} items ", self.view.len()))
                    .right_aligned()
                    .dim(),
            );

        if rows.is_empty() {
            let msg = if self.scan_time.is_none() {
                "Looking for clutter…"
            } else if self.items.is_empty() {
                "Nothing to clean up. Your disk is already on a diet 🥗"
            } else {
                "No matches — press t to change the age filter or Esc to clear search"
            };
            f.render_widget(
                Paragraph::new(msg)
                    .dim()
                    .alignment(Alignment::Center)
                    .block(block),
                area,
            );
            return;
        }

        let table = Table::new(
            rows,
            [
                Constraint::Length(1),
                Constraint::Length(10),
                Constraint::Length(6),
                Constraint::Length(10),
                Constraint::Fill(1),
            ],
        )
        .header(
            Row::new(["", "      SIZE", "   AGE", "TYPE", "PATH"]).style(Style::new().bold().dim()),
        )
        .column_spacing(1)
        .row_highlight_style(Style::new().bg(Color::Rgb(45, 50, 60)).bold())
        .highlight_symbol("▌")
        .block(block);
        f.render_stateful_widget(table, area, &mut self.table);
    }

    fn render_breakdown(&self, f: &mut Frame, area: Rect) {
        let mut by: HashMap<&str, u64> = HashMap::new();
        for &i in &self.view {
            let it = &self.items[i];
            if it.state != State::Deleted {
                *by.entry(it.entry.ecosystem).or_default() += it.entry.bytes;
            }
        }
        let mut rows: Vec<(&str, u64)> = by.into_iter().collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.1));
        let max = rows.first().map(|r| r.1).unwrap_or(1).max(1);
        let bar_w = (area.width as usize).saturating_sub(2 + 10 + 9 + 2);
        let lines: Vec<Line> = rows
            .iter()
            .take(area.height.saturating_sub(2) as usize)
            .map(|&(eco, bytes)| {
                let bar = bar(bytes as f64 / max as f64 * bar_w as f64);
                let pad = (bar_w + 1).saturating_sub(bar.chars().count());
                Line::from(vec![
                    Span::styled(format!("{eco:<9} "), Style::new().fg(eco_color(eco))),
                    Span::styled(bar, Style::new().fg(eco_color(eco))),
                    Span::raw(" ".repeat(pad)),
                    Span::styled(format!("{:>8}", units::bytes(bytes)), Style::new().bold()),
                ])
            })
            .collect();
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().dim())
            .title(Line::from(" by ecosystem ").bold());
        f.render_widget(Paragraph::new(lines).block(block), area);
    }

    fn render_detail(&self, f: &mut Frame, area: Rect) {
        if let Some((msg, at)) = &self.flash
            && at.elapsed() < Duration::from_secs(3)
        {
            f.render_widget(Paragraph::new(format!(" {msg}")).fg(Color::Yellow), area);
            return;
        }
        let Some(i) = self.current() else { return };
        let it = &self.items[i];
        let e = &it.entry;
        let when = match e.source {
            Source::Project => match units::short_age(e.last_modified).as_str() {
                "—" => "project age unknown".into(),
                "today" => "project last touched today".into(),
                age => format!("project last touched {age} ago"),
            },
            Source::Cache => "global cache".into(),
        };
        let mut spans = vec![
            Span::styled(format!(" {} ", e.what), Style::new().bold()),
            Span::styled(
                format!(
                    "· {} {} · {when} · restore: ",
                    fmt_count(e.files),
                    if e.files == 1 { "file" } else { "files" }
                ),
                Style::new().dim(),
            ),
            Span::styled(e.restore, Style::new().fg(ACCENT)),
        ];
        if let State::Failed(err) = &it.state {
            spans.push(Span::styled(
                format!("  ✗ {err}"),
                Style::new().fg(Color::Red),
            ));
        }
        f.render_widget(
            Paragraph::new(Line::from(spans)).wrap(Wrap { trim: true }),
            area,
        );
    }

    fn render_footer(&self, f: &mut Frame, area: Rect) {
        let keys: &[(&str, &str)] = match self.mode {
            Mode::Search => &[("type", "filter"), ("enter", "done"), ("esc", "clear")],
            // Most important first: on narrow terminals the tail is dropped.
            _ => &[
                ("↑↓", "move"),
                ("space", "select"),
                ("d", "delete"),
                ("q", "quit"),
                ("t", "age"),
                ("a", "all"),
                ("s", "sort"),
                ("/", "search"),
                ("o", "open"),
            ],
        };
        let mut spans = vec![Span::raw(" ")];
        let mut used = 1;
        for (k, v) in keys {
            let w = k.chars().count() + v.len() + 4;
            if used + w > area.width as usize + 3 {
                break;
            }
            used += w;
            spans.push(Span::styled(*k, Style::new().fg(ACCENT).bold()));
            spans.push(Span::styled(format!(" {v}   "), Style::new().dim()));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_confirm(&self, f: &mut Frame, ids: &[usize]) {
        let total = self.total(ids.iter().copied());
        let area = centered(f.area(), 58, 9);
        f.render_widget(Clear, area);
        let preview = if ids.len() == 1 {
            truncate_left(&self.items[ids[0]].shown_path, 50)
        } else {
            format!("{} folders", ids.len())
        };
        let text = vec![
            Line::from(""),
            Line::from(vec![
                Span::raw("Delete "),
                Span::styled(preview, Style::new().bold()),
            ])
            .centered(),
            Line::from(vec![
                Span::raw("and free "),
                Span::styled(units::bytes(total), Style::new().fg(ACCENT).bold()),
                Span::raw("?"),
            ])
            .centered(),
            Line::from(""),
            Line::from("This skips the trash and cannot be undone.")
                .dim()
                .centered(),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    " y ",
                    Style::new().fg(Color::Black).bg(Color::LightRed).bold(),
                ),
                Span::raw(" delete     "),
                Span::styled(
                    " any key ",
                    Style::new().fg(Color::Black).bg(Color::Gray).bold(),
                ),
                Span::raw(" cancel"),
            ])
            .centered(),
        ];
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(Color::LightRed))
            .title(Line::from(" confirm ").bold().centered());
        f.render_widget(Paragraph::new(text).block(block), area);
    }
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

/// Smooth horizontal bar using eighth-block characters.
fn bar(cells: f64) -> String {
    const PARTS: [&str; 8] = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];
    let full = cells.floor() as usize;
    let rem = ((cells - full as f64) * 8.0).round() as usize;
    let mut s = "█".repeat(full);
    if rem >= 8 {
        s.push('█');
    } else {
        s.push_str(PARTS[rem]);
    }
    if s.is_empty() {
        s.push('▏');
    }
    s
}

fn truncate_left(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let tail: String = s.chars().skip(n - max + 1).collect();
    format!("…{tail}")
}

fn eco_color(eco: &str) -> Color {
    match eco {
        "node" | "deno" => Color::Rgb(250, 204, 21),
        "rust" => Color::Rgb(251, 146, 60),
        "python" => Color::Rgb(96, 165, 250),
        "java" | "android" => Color::Rgb(248, 113, 113),
        "swift" | "xcode" => Color::Rgb(244, 114, 182),
        "go" => Color::Rgb(34, 211, 238),
        "dart" => Color::Rgb(56, 189, 248),
        ".net" => Color::Rgb(167, 139, 250),
        "homebrew" => Color::Rgb(253, 186, 116),
        _ => Color::Rgb(163, 230, 53),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use std::path::PathBuf;
    use std::time::SystemTime;

    fn entry(path: &str, eco: &'static str, bytes: u64, days: u64, source: Source) -> Entry {
        Entry {
            path: PathBuf::from(path),
            source,
            ecosystem: eco,
            what: "test artifact",
            restore: "make",
            bytes,
            files: 42,
            last_modified: Some(SystemTime::now() - Duration::from_secs(days * 86_400 + 60)),
        }
    }

    fn app() -> (App, mpsc::Receiver<DelMsg>) {
        let (tx, rx) = mpsc::channel();
        let mut app = App::new(
            Filter::default(),
            Arc::new(Progress::default()),
            "~/code".into(),
            tx,
        );
        app.push(entry(
            "/code/web/node_modules",
            "node",
            1_800_000_000,
            120,
            Source::Project,
        ));
        app.push(entry(
            "/code/api/target",
            "rust",
            9_700_000_000,
            14,
            Source::Project,
        ));
        app.push(entry(
            "/code/ml/.venv",
            "python",
            450_000_000,
            400,
            Source::Project,
        ));
        app.push(entry(
            "/home/.npm/_cacache",
            "node",
            3_100_000_000,
            0,
            Source::Cache,
        ));
        app.scan_time = Some(Duration::from_millis(2400));
        app.rebuild_view();
        (app, rx)
    }

    fn screen(app: &mut App, w: u16, h: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| app.render(f)).unwrap();
        let buf = term.backend().buffer().clone();
        (0..h)
            .map(|y| {
                (0..w)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn renders_sorted_by_size_with_breakdown() {
        let (mut app, _rx) = app();
        let out = screen(&mut app, 120, 16);
        if std::env::var_os("SHOW_TUI").is_some() {
            println!("{out}");
        }
        let target = out.find("/code/api/target").unwrap();
        let npm = out.find("_cacache").unwrap();
        let web = out.find("/code/web/node_modules").unwrap();
        assert!(target < npm && npm < web, "sorted by size");
        assert!(out.contains("by ecosystem"));
        assert!(out.contains("found 15.1 GB"));
        // narrow terminals drop the side panel but still render
        let narrow = screen(&mut app, 60, 12);
        assert!(!narrow.contains("by ecosystem"));

        // a long scan path is shortened instead of colliding with the totals
        app.roots_label = format!("/private/tmp/{}/sandbox", "x".repeat(80));
        let out = screen(&mut app, 84, 12);
        let header = out.lines().next().unwrap();
        assert!(header.contains("…"), "{header}");
        assert!(header.contains("found 15.1 GB"), "{header}");
        assert!(out.contains("q quit"), "footer keeps quit: {out}");
        assert!(out.contains("last touched 2w ago"));
        if std::env::var_os("SHOW_TUI").is_some() {
            println!("{out}");
        }
    }

    #[test]
    fn cursor_stays_on_top_until_the_user_moves() {
        let (mut app, _rx) = app();
        app.push(entry(
            "/huge/target",
            "rust",
            50_000_000_000,
            3,
            Source::Project,
        ));
        app.rebuild_view();
        assert_eq!(app.items[app.current().unwrap()].shown_path, "/huge/target");

        app.on_key(KeyCode::Down, KeyModifiers::NONE);
        let picked = app.current().unwrap();
        app.push(entry(
            "/huger/target",
            "rust",
            90_000_000_000,
            3,
            Source::Project,
        ));
        app.rebuild_view();
        assert_eq!(app.current(), Some(picked), "cursor follows the chosen row");
    }

    #[test]
    fn breakdown_bars_line_up() {
        let (mut app, _rx) = app();
        let out = screen(&mut app, 120, 16);
        let cols: Vec<usize> = out
            .lines()
            .filter_map(|l| {
                let panel: String = l.chars().skip(86).collect();
                if !panel.contains(['█', '▏', '▌']) {
                    return None;
                }
                let byte = panel.find(" GB").or_else(|| panel.find(" MB"))?;
                Some(panel[..byte].chars().count())
            })
            .collect();
        assert!(cols.len() >= 3, "{out}");
        assert!(cols.windows(2).all(|w| w[0] == w[1]), "{cols:?}\n{out}");
    }

    #[test]
    fn keys_select_filter_and_confirm() {
        let (mut app, _rx) = app();
        let none = KeyModifiers::NONE;
        app.on_key(KeyCode::Char(' '), none); // select biggest, move down
        assert_eq!(app.selection().len(), 1);
        app.on_key(KeyCode::Char('a'), none); // select all
        assert_eq!(app.selection().len(), 4);
        app.on_key(KeyCode::Char('a'), none); // toggle off
        assert!(app.selection().is_empty());

        app.on_key(KeyCode::Char('t'), none); // > 30 days hides caches and fresh projects
        app.rebuild_view();
        assert_eq!(app.view.len(), 2);
        app.on_key(KeyCode::Char('t'), none);
        app.on_key(KeyCode::Char('t'), none); // > 1 year
        app.rebuild_view();
        assert_eq!(app.view.len(), 1);
        app.on_key(KeyCode::Char('t'), none); // back to any age
        app.rebuild_view();

        app.on_key(KeyCode::Char('/'), none);
        for c in "venv".chars() {
            app.on_key(KeyCode::Char(c), none);
        }
        app.on_key(KeyCode::Enter, none);
        app.rebuild_view();
        assert_eq!(app.view.len(), 1);

        app.on_key(KeyCode::Char('d'), none);
        assert!(matches!(app.mode, Mode::Confirm(ref ids) if ids.len() == 1));
        let out = screen(&mut app, 120, 16);
        assert!(out.contains("cannot be undone"));
        app.on_key(KeyCode::Char('n'), none);
        assert!(matches!(app.mode, Mode::Browse));
        assert!(app.items.iter().all(|i| i.state == State::Ready));
    }

    #[test]
    fn deletes_for_real_in_background() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("node_modules");
        std::fs::create_dir_all(dir.join("pkg")).unwrap();
        std::fs::write(dir.join("pkg/index.js"), "x").unwrap();

        let (tx, rx) = mpsc::channel();
        let mut app = App::new(
            Filter::default(),
            Arc::new(Progress::default()),
            "t".into(),
            tx,
        );
        app.push(entry(
            dir.to_str().unwrap(),
            "node",
            1234,
            5,
            Source::Project,
        ));
        app.rebuild_view();
        app.on_key(KeyCode::Char('d'), KeyModifiers::NONE);
        app.on_key(KeyCode::Char('y'), KeyModifiers::NONE);
        assert_eq!(app.deleting, 1);
        let msg = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        app.finish_delete(msg);
        assert_eq!(app.freed, 1234);
        assert!(app.items[0].state == State::Deleted);
        assert!(!dir.exists());
    }

    #[test]
    fn helpers() {
        assert_eq!(
            truncate_left("~/code/project/node_modules", 13),
            "…node_modules"
        );
        assert_eq!(truncate_left("short", 12), "short");
        assert_eq!(fmt_count(1234567), "1,234,567");
        assert_eq!(fmt_count(12), "12");
        assert_eq!(bar(2.0), "██");
        assert_eq!(bar(0.0), "▏");
        assert_eq!(bar(1.5), "█▌");
    }

    /// Regenerates `assets/screenshot.svg` for the README:
    /// `cargo test screenshot -- --ignored`
    #[test]
    #[ignore]
    fn screenshot() {
        use ratatui::style::{Color as C, Modifier as M};
        let (tx, _rx) = mpsc::channel();
        let home = crate::caches::home_dir().unwrap();
        let h = |p: &str| home.join(p).to_string_lossy().into_owned();
        let mut app = App::new(
            Filter::default(),
            Arc::new(Progress::default()),
            "~".into(),
            tx,
        );
        let data: &[(&str, &'static str, u64, u64, Source)] = &[
            (
                "code/tauri-notes/src-tauri/target",
                "rust",
                9_780_000_000,
                16,
                Source::Project,
            ),
            (".npm/_cacache", "node", 9_630_000_000, 0, Source::Cache),
            (
                "Library/Caches/Homebrew",
                "homebrew",
                7_500_000_000,
                0,
                Source::Cache,
            ),
            (
                "code/old-game/Library",
                "unity",
                6_120_000_000,
                420,
                Source::Project,
            ),
            (".gradle/caches", "java", 5_280_000_000, 0, Source::Cache),
            (
                "code/acme-dashboard/node_modules",
                "node",
                1_870_000_000,
                95,
                Source::Project,
            ),
            (
                "Library/Developer/Xcode/DerivedData",
                "xcode",
                1_410_000_000,
                0,
                Source::Cache,
            ),
            (
                "code/thesis/.venv",
                "python",
                1_240_000_000,
                210,
                Source::Project,
            ),
            (
                "code/mobile-app/ios/Pods",
                "swift",
                812_000_000,
                63,
                Source::Project,
            ),
            (
                "code/landing/.next",
                "node",
                534_000_000,
                33,
                Source::Project,
            ),
            (
                "code/api-gateway/target",
                "rust",
                498_000_000,
                3,
                Source::Project,
            ),
            (
                "code/flutter_shop/build",
                "dart",
                316_000_000,
                18,
                Source::Project,
            ),
            (
                "code/infra/.terraform",
                "terraform",
                201_000_000,
                150,
                Source::Project,
            ),
            (
                "code/blog/node_modules",
                "node",
                187_000_000,
                400,
                Source::Project,
            ),
        ];
        for &(p, eco, b, d, src) in data {
            let mut e = entry(&h(p), eco, b, d, src);
            e.files = b / 21_000;
            e.what = match eco {
                "rust" => "Cargo build output",
                _ => "build output",
            };
            e.restore = "cargo build";
            app.push(e);
        }
        app.rebuild_view();
        for i in [3, 5, 7, 12] {
            app.items[i].selected = true;
        }
        app.items[13].state = State::Deleted;
        app.freed = 187_000_000;
        app.scan_time = Some(Duration::from_millis(2480));
        app.progress.dirs.store(184_213, Ordering::Relaxed);

        let (w, hgt) = (112u16, 24u16);
        let mut term = Terminal::new(TestBackend::new(w, hgt)).unwrap();
        term.draw(|f| app.render(f)).unwrap();
        let buf = term.backend().buffer().clone();

        let hex = |c: C, fg: bool| -> String {
            match c {
                C::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
                C::Black => "#0b0d12".into(),
                C::Red | C::LightRed => "#f87171".into(),
                C::Green | C::LightGreen => "#86efac".into(),
                C::Yellow | C::LightYellow => "#facc15".into(),
                C::Blue | C::LightBlue => "#60a5fa".into(),
                C::Cyan | C::LightCyan => "#67e8f9".into(),
                C::Magenta | C::LightMagenta => "#f472b6".into(),
                C::Gray => "#a1a1aa".into(),
                C::DarkGray => "#52525b".into(),
                C::White => "#fafafa".into(),
                _ if fg => "#e4e4e7".into(),
                _ => "none".into(),
            }
        };
        let esc = |s: &str| {
            s.replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
        };
        let (cw, ch, pad, bar) = (8.43_f64, 19.0_f64, 18.0_f64, 34.0_f64);
        let width = w as f64 * cw + pad * 2.0;
        let height = hgt as f64 * ch + pad * 2.0 + bar;
        let mut svg = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width:.0}" height="{height:.0}" viewBox="0 0 {width:.0} {height:.0}">
<rect width="100%" height="100%" rx="10" fill="#0f1117"/>
<circle cx="20" cy="17" r="6" fill="#ff5f57"/><circle cx="40" cy="17" r="6" fill="#febc2e"/><circle cx="60" cy="17" r="6" fill="#28c840"/>
<text x="{:.0}" y="22" fill="#71717a" font-family="ui-sans-serif,system-ui,sans-serif" font-size="13" text-anchor="middle">diskdiet — ~</text>
<g font-family="ui-monospace,SFMono-Regular,Menlo,Consolas,monospace" font-size="14" xml:space="preserve">
"##,
            width / 2.0
        );
        for y in 0..hgt {
            let ty = pad + bar + y as f64 * ch;
            let mut x = 0u16;
            while x < w {
                let cell = &buf[(x, y)];
                let style = (cell.fg, cell.bg, cell.modifier);
                let start = x;
                let mut text = String::new();
                while x < w && (buf[(x, y)].fg, buf[(x, y)].bg, buf[(x, y)].modifier) == style {
                    text.push_str(buf[(x, y)].symbol());
                    x += 1;
                }
                let px = pad + start as f64 * cw;
                let bg = hex(style.1, false);
                if bg != "none" {
                    svg += &format!(
                        r#"<rect x="{px:.1}" y="{:.1}" width="{:.1}" height="{ch}" fill="{bg}"/>"#,
                        ty,
                        (x - start) as f64 * cw
                    );
                }
                if text.trim().is_empty() {
                    continue;
                }
                let m = style.2;
                let mut attrs = format!(r#"fill="{}""#, hex(style.0, true));
                if m.contains(M::BOLD) {
                    attrs += r#" font-weight="700""#;
                }
                if m.contains(M::DIM) {
                    attrs += r#" opacity="0.5""#;
                }
                if m.contains(M::CROSSED_OUT) {
                    attrs += r#" text-decoration="line-through""#;
                }
                // Pin every glyph to the grid so box drawing lines up in any font.
                let xs: Vec<String> = (0..text.chars().count())
                    .map(|i| format!("{:.1}", px + i as f64 * cw))
                    .collect();
                svg += &format!(
                    "<text x=\"{}\" y=\"{:.1}\" {attrs}>{}</text>\n",
                    xs.join(" "),
                    ty + ch * 0.75,
                    esc(&text).replace(' ', "\u{a0}")
                );
            }
        }
        svg += "</g>\n</svg>\n";
        let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/screenshot.svg");
        std::fs::create_dir_all(out.parent().unwrap()).unwrap();
        std::fs::write(&out, svg).unwrap();
    }
}
