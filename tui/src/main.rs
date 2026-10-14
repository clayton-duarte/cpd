//! cpd-tui: the cockpit's views.
//!
//! One binary, four views, each meant to run in its own tmux pane of the cockpit (see `scripts/cockpit.sh`):
//! - `context`:   left bar. Context → the Lead sessions running in the cockpit (discovered from tmux, named by their pi
//!              names); Enter/click switches the centre (tmux swap-pane of the session's chat pane).
//! - `plan`:    top centre. The Jobs of the Workflow of the session in the centre; tabs are the Context's sessions.
//!              Enter/click on a Job opens the Worker's chat read-only beside the chat.
//! - `dispatch`: right bar placeholder.
//! - `worker <session.jsonl> <job>`: read-only chat view of a pi session file (follows it live).
//!
//! Data comes only from the cpd CLI (`cpd … --wf`), fetched by the context view into a cache the other views read.

use std::{
    collections::HashSet,
    env, fs,
    path::PathBuf,
    process::Command,
    thread,
    time::{Duration, Instant},
};

use ratatui::{
    crossterm::{
        event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind},
        execute,
    },
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{List, ListItem, ListState, Paragraph, Wrap},
    DefaultTerminal,
};
use serde_json::Value;

mod sessions;
mod context;

use sessions::Session;

/// The cockpit's tmux server (`tmux -L <socket>`), its session, and the views' state dir; env overrides for the spike.
fn socket() -> String {
    env::var("CPD_TUI_SOCKET").unwrap_or_else(|_| "cpd".into())
}
fn session() -> String {
    env::var("CPD_TUI_SESSION").unwrap_or_else(|_| "cpd".into())
}
fn state_dir() -> String {
    env::var("CPD_TUI_STATE").unwrap_or_else(|_| "/tmp/cpd-tui".into())
}
/// cpd's home (`CPD_HOME`, default `~/.cpd`): where cpd records a session's attached Workflow.
fn cpd_home() -> PathBuf {
    env::var_os("CPD_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".cpd"))
}

/// The chat panes running in the cockpit, straight from tmux.
fn discover() -> Vec<Session> {
    sessions::parse_panes(&tmux(&["list-panes", "-a", "-F", sessions::PANE_FORMAT]))
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let view = args.get(1).map(String::as_str).unwrap_or("");
    let mut terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableMouseCapture);
    let result = match view {
        "context" => run_context(&mut terminal),
        "plan" => run_plan(&mut terminal),
        "dispatch" => run_dispatch(&mut terminal),
        "worker" => run_worker(&mut terminal, args.get(2).cloned().unwrap_or_default()),
        _ => Err(format!("usage: {} context|plan|dispatch|worker <session.jsonl> <job>", args[0])),
    };
    let _ = execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1);
    }
}

// ---------- helpers ----------

fn cpd(args: &[&str]) -> Option<Value> {
    let out = Command::new("cpd").args(args).output().ok()?;
    let v: Value = serde_json::from_slice(&out.stdout).ok()?;
    (v["ok"] != false).then_some(v) // an error envelope is not data: never cache it
}

fn tmux(args: &[&str]) -> String {
    let out = Command::new("tmux").arg("-L").arg(socket()).args(args).output();
    let (stdout, stderr, code) = match &out {
        Ok(o) => (String::from_utf8_lossy(&o.stdout).into_owned(), String::from_utf8_lossy(&o.stderr).into_owned(), o.status.code()),
        Err(e) => (String::new(), e.to_string(), None),
    };
    let log = format!("tmux {:?} -> code={:?} out={:?} err={:?}\n", args, code, stdout.trim(), stderr.trim());
    let _ = fs::OpenOptions::new().create(true).append(true).open(format!("{}/debug.log", state_dir())).and_then(|mut f| std::io::Write::write_all(&mut f, log.as_bytes()));
    stdout
}

/// Bring the chat pane of session `id` into the centre slot (the chat pane in window `main`). swap-pane keeps both
/// processes running and the slot keeps its size (verified).
fn switch_to(id: &str) {
    for line in tmux(&["list-panes", "-t", &format!("{}:main", session()), "-F", "#{pane_id} #{@view}"]).lines() {
        if line.ends_with(" worker") {
            tmux(&["kill-pane", "-t", line.split(' ').next().unwrap_or("")]);
        }
    }
    let all = discover();
    let current = all.iter().find(|s| s.is_active());
    let target = all.iter().find(|s| s.id == id);
    if let (Some(c), Some(t)) = (current, target) {
        if c.pane_id != t.pane_id {
            tmux(&["swap-pane", "-d", "-s", &t.pane_id, "-t", &c.pane_id]);
        }
    }
}

// ---------- cache (one fetcher writes, every view reads) ----------

fn cache_file(name: &str) -> String {
    format!("{}/cache/{name}.json", state_dir())
}

fn write_atomic(path: &str, data: &str) {
    let tmp = format!("{path}.tmp");
    if fs::write(&tmp, data).is_ok() {
        let _ = fs::rename(&tmp, path);
    }
}

fn read_json(path: &str) -> Option<Value> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

/// The only place that calls the slow CLI (13–60 s per call under load). Views read the cache files instead,
/// so navigating never waits on cpd. Session paths never change once set, so each is fetched once.
fn fetch_loop() {
    let _ = fs::create_dir_all(format!("{}/cache", state_dir()));
    let mut sessions: serde_json::Map<String, Value> = read_json(&cache_file("sessions")).and_then(|v| v.as_object().cloned()).unwrap_or_default();
    loop {
        // the Workflows of the running sessions: discovered each round, so a new fork's Workflow is picked up
        let mut plans: Vec<String> = discover().iter().filter_map(|s| sessions::workflow_of(&cpd_home(), &s.id)).collect();
        plans.sort();
        plans.dedup();
        for plan in &plans {
            let Some(v) = cpd(&["workflow", "show", "--wf", plan]) else { continue };
            write_atomic(&cache_file(plan), &v.to_string());
            let latest: Vec<String> = v["jobs"]
                .as_array()
                .map(|a| a.iter().filter_map(|j| j["attempts"].as_array()?.last()?["id"].as_str().map(String::from)).collect())
                .unwrap_or_default();
            for id in latest {
                if sessions.contains_key(&id) {
                    continue;
                }
                if let Some(path) = cpd(&["attempt", "show", &id, "--wf", plan]).and_then(|a| a["attempt"]["metadata"]["session_path"].as_str().map(String::from)) {
                    sessions.insert(id, Value::String(path));
                    write_atomic(&cache_file("sessions"), &Value::Object(sessions.clone()).to_string());
                }
            }
        }
        thread::sleep(Duration::from_secs(5));
    }
}

fn cache_age(name: &str) -> Option<u64> {
    let m = fs::metadata(cache_file(name)).ok()?.modified().ok()?;
    m.elapsed().ok().map(|d| d.as_secs())
}

fn status_style(status: &str) -> Style {
    match status {
        "closed" => Style::new().fg(Color::Green),
        "running" | "in_review" => Style::new().fg(Color::Yellow),
        "failed" => Style::new().fg(Color::Red),
        "cancelled" => Style::new().fg(Color::DarkGray),
        _ => Style::new().fg(Color::Gray),
    }
}

/// Row under a mouse click inside a borderless list, given its scroll offset.
fn clicked_row(area: Rect, row: u16, offset: usize) -> Option<usize> {
    (row >= area.y && row < area.y + area.height).then(|| (row - area.y) as usize + offset)
}

// ---------- context (left bar) ----------

/// A running session as the views show it.
#[derive(Clone)]
struct Entry {
    session: Session,
    label: String,
    workflow: Option<String>,
}

/// The running sessions, grouped by Context, each labelled by its pi name, else its Workflow id, else its short id.
fn snapshot() -> Vec<(String, Vec<Entry>)> {
    let home = cpd_home();
    sessions::group(discover())
        .into_iter()
        .map(|(ctx, list)| {
            let entries = list
                .into_iter()
                .map(|session| {
                    let workflow = sessions::workflow_of(&home, &session.id);
                    let label = sessions::label(&sessions::display_name(&session), workflow.as_deref(), &session.id);
                    Entry { session, label, workflow }
                })
                .collect();
            (ctx, entries)
        })
        .collect()
}

#[derive(Clone, Default)]
struct PlanSummary {
    phase: String,
    merged: usize,
    total: usize,
    failed: usize,
}

fn summarize(plan: &str) -> PlanSummary {
    let v = read_json(&cache_file(plan)).unwrap_or(Value::Null);
    let jobs: Vec<&Value> = v["jobs"].as_array().map(|a| a.iter().filter(|j| j["status"] != "cancelled").collect()).unwrap_or_default();
    PlanSummary {
        phase: v["phase"].as_str().unwrap_or("?").to_string(),
        merged: jobs.iter().filter(|j| j["status"] == "closed").count(),
        total: jobs.len(),
        failed: jobs.iter().filter(|j| j["status"] == "failed").count(),
    }
}

/// One session's line: branch, state icon, label, then its Workflow's phase, merged/total and failed (none when unattached).
fn session_line(e: &Entry, last: bool, selected: bool) -> Line<'static> {
    let (icon, color) = context::state_icon(&e.session.state);
    let label = if e.session.is_active() { e.label.clone().cyan().bold() } else { Span::from(e.label.clone()) };
    let mut spans = vec![format!(" {} ", if last { "└" } else { "├" }).into(), icon.fg(color), " ".into(), label];
    if let Some(wf) = &e.workflow {
        let p = summarize(wf);
        spans.push(format!("  {}", p.phase).dim());
        spans.push(format!("  {}/{}", p.merged, p.total).fg(Color::Green));
        if p.failed > 0 {
            spans.push(format!(" ✗{}", p.failed).fg(Color::Red));
        }
    }
    let line = Line::from(spans);
    if selected { line.reversed() } else { line }
}

fn context_line(name: &str, collapsed: bool, selected: bool) -> Line<'static> {
    let line = Line::from(vec![format!("{} ", if collapsed { "▸" } else { "▾" }).into(), name.to_string().bold(), "  (Context)".dim()]);
    if selected { line.reversed() } else { line }
}

fn run_context(terminal: &mut DefaultTerminal) -> Result<(), String> {
    thread::spawn(fetch_loop);
    let mut collapsed: HashSet<String> = HashSet::new();
    let mut selected = 0usize; // index into the visible rows
    let mut area = Rect::default();
    let mut groups = snapshot();
    let mut last_refresh = Instant::now();
    loop {
        if last_refresh.elapsed() > Duration::from_secs(1) {
            groups = snapshot();
            last_refresh = Instant::now();
        }
        let shapes: Vec<context::Shape> = groups.iter().map(|(c, l)| context::Shape { sessions: l.len(), collapsed: collapsed.contains(c) }).collect();
        let flat: Vec<&Entry> = groups.iter().flat_map(|(_, l)| l).collect();
        let rows = context::visible_rows(&shapes);
        selected = selected.min(rows.len().saturating_sub(1));
        terminal
            .draw(|f| {
                area = f.area();
                let lines: Vec<Line> = rows
                    .iter()
                    .enumerate()
                    .map(|(i, row)| match *row {
                        context::Row::Context(g) => context_line(&groups[g].0, shapes[g].collapsed, i == selected),
                        context::Row::Session(n) => {
                            let last = flat.get(n + 1).is_none_or(|next| next.session.context() != flat[n].session.context());
                            session_line(flat[n], last, i == selected)
                        }
                    })
                    .collect();
                if lines.is_empty() {
                    f.render_widget(Paragraph::new(Line::from(" no Lead sessions in the cockpit".dim())), area);
                } else {
                    f.render_widget(Paragraph::new(lines), area);
                }
            })
            .map_err(|e| e.to_string())?;
        if !event::poll(Duration::from_millis(250)).map_err(|e| e.to_string())? {
            continue;
        }
        let mut activate = None;
        match event::read().map_err(|e| e.to_string())? {
            Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Down | KeyCode::Char('j') => selected = (selected + 1).min(rows.len().saturating_sub(1)),
                KeyCode::Up | KeyCode::Char('k') => selected = selected.saturating_sub(1),
                KeyCode::Enter => activate = rows.get(selected).copied(),
                KeyCode::Char(' ') if matches!(rows.get(selected), Some(context::Row::Context(_))) => activate = rows.get(selected).copied(),
                _ => {}
            },
            Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                if let Some(i) = context::row_at(&shapes, area.y, m.row) {
                    selected = i;
                    activate = Some(rows[i]);
                }
            }
            _ => {}
        }
        match activate {
            Some(context::Row::Context(g)) => {
                let name = groups[g].0.clone();
                if !collapsed.remove(&name) {
                    collapsed.insert(name);
                }
            }
            Some(context::Row::Session(n)) => {
                switch_to(&flat[n].session.id);
                groups = snapshot();
                last_refresh = Instant::now();
            }
            None => {}
        }
    }
}

// ---------- plan view (top centre) ----------

#[derive(Clone)]
struct JobRow {
    key: String,
    status: String,
    title: String,
    attempt: Option<String>,
    attempt_label: String,
}

fn jobs_of(plan: &str) -> (String, Vec<JobRow>) {
    let v = read_json(&cache_file(plan)).unwrap_or(Value::Null);
    let rows = v["jobs"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|j| j["status"] != "cancelled")
                .map(|j| JobRow {
                    key: j["key"].as_str().unwrap_or("?").into(),
                    status: j["status"].as_str().unwrap_or("?").into(),
                    title: j["title"].as_str().unwrap_or("").into(),
                    attempt: j["attempts"].as_array().and_then(|a| a.last()).and_then(|a| a["id"].as_str()).map(String::from),
                    attempt_label: j["attempts"]
                        .as_array()
                        .and_then(|a| a.last())
                        .map(|a| {
                            let id = a["id"].as_str().unwrap_or("");
                            let n = id.rsplit("-a").next().unwrap_or("?");
                            format!("a{n} {}", a["tier"].as_str().unwrap_or(""))
                        })
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();
    let title = v["workflow"]["title"].as_str().map(String::from).unwrap_or_else(|| "loading… (first fetch)".into());
    (title, rows)
}

/// The pi session file of an Attempt, from the fetcher's cache (no CLI call on click).
fn session_of(attempt: &str) -> Option<String> {
    read_json(&cache_file("sessions"))?[attempt].as_str().map(String::from)
}

/// Open (or retarget, or close when it already shows this Job) the read-only Worker pane beside the Lead chat.
fn toggle_worker_view(exe: &str, job: &str, path: &str) {
    let panes = tmux(&["list-panes", "-t", &format!("{}:main", session()), "-F", "#{pane_id} #{@session_id} #{@view} #{@job}"]);
    let mut chat = None;
    let mut worker: Option<(String, String)> = None;
    for line in panes.lines() {
        let f: Vec<&str> = line.split(' ').collect();
        if f.len() >= 2 && !f[1].is_empty() {
            chat = Some(f[0].to_string());
        }
        if f.len() >= 3 && f[2] == "worker" {
            worker = Some((f[0].to_string(), f.get(3).unwrap_or(&"").to_string()));
        }
    }
    let cmd = format!("{exe} worker '{path}' '{job}'");
    match (worker, chat) {
        (Some((pane, shown)), _) if shown == job => {
            tmux(&["kill-pane", "-t", &pane]);
        }
        (Some((pane, _)), _) => {
            tmux(&["respawn-pane", "-k", "-t", &pane, &cmd]);
            tmux(&["set", "-p", "-t", &pane, "@job", job]);
        }
        (None, Some(chat)) => {
            let pane = tmux(&["split-window", "-h", "-d", "-t", &chat, "-l", "50%", "-P", "-F", "#{pane_id}", &cmd]);
            let pane = pane.trim();
            tmux(&["set", "-p", "-t", pane, "@view", "worker"]);
            tmux(&["set", "-p", "-t", pane, "@job", job]);
        }
        (None, None) => {}
    }
}

/// What the plan view shows for the session in the centre: its Context, the Context's sessions (the tabs) and the active one.
struct Centre {
    context: String,
    tabs: Vec<Entry>,
    active: Entry,
}

fn centre() -> Option<Centre> {
    snapshot().into_iter().find_map(|(context, tabs)| {
        let active = tabs.iter().find(|e| e.session.is_active())?.clone();
        Some(Centre { context, tabs, active })
    })
}

fn run_plan(terminal: &mut DefaultTerminal) -> Result<(), String> {
    let exe = env::current_exe().map_err(|e| e.to_string())?.display().to_string();
    let (mut centre_now, mut title, mut jobs) = (None::<Centre>, String::from("loading…"), Vec::<JobRow>::new());
    let mut state = ListState::default().with_selected(Some(0));
    let mut list_area = Rect::default();
    let mut head_area = Rect::default();
    let mut tab_ranges: Vec<(u16, u16, String)> = Vec::new();
    let mut note = String::new();
    let mut last_check = Instant::now() - Duration::from_secs(1);
    loop {
        if last_check.elapsed() > Duration::from_millis(300) {
            let before = centre_now.as_ref().map(|c| c.active.session.id.clone());
            centre_now = centre();
            if before != centre_now.as_ref().map(|c| c.active.session.id.clone()) {
                note.clear();
                state.select(Some(0));
            }
            (title, jobs) = match centre_now.as_ref().and_then(|c| c.active.workflow.as_deref()) {
                Some(wf) => jobs_of(wf), // a cache read: instant
                None => ("no Workflow yet".into(), Vec::new()),
            };
            last_check = Instant::now();
        }
        terminal
            .draw(|f| {
                let [head, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(f.area());
                head_area = head;
                tab_ranges.clear();
                let mut spans: Vec<Span> = Vec::new();
                if let Some(c) = &centre_now {
                    spans.push(format!(" {} ▸ ", c.context).dim());
                    let mut x = head.x + (c.context.chars().count() as u16) + 4;
                    for e in &c.tabs {
                        let label = format!(" {} ", e.label);
                        let w = label.chars().count() as u16;
                        tab_ranges.push((x, x + w, e.session.id.clone()));
                        spans.push(if e.session.is_active() { label.reversed().bold() } else { label.into() });
                        spans.push(" ".into());
                        x += w + 1;
                    }
                    spans.push(format!("  {title}").into());
                    if let Some(wf) = &c.active.workflow {
                        let age = cache_age(wf).map(|a| format!("  updated {a}s ago")).unwrap_or_else(|| "  loading…".into());
                        spans.push(age.dim());
                    }
                } else {
                    spans.push(" no Lead session in the centre".dim());
                }
                if !note.is_empty() {
                    spans.push(format!("  {note}").yellow());
                }
                f.render_widget(Paragraph::new(Line::from(spans)), head);
                list_area = body;
                let items: Vec<ListItem> = jobs
                    .iter()
                    .map(|j| ListItem::new(Line::from(vec![Span::styled(format!("{:<12}", j.status), status_style(&j.status)), format!("{:<18}", j.key).bold(), format!("{:<8}", j.attempt_label).fg(Color::Cyan), j.title.clone().dim()])))
                    .collect();
                let list = List::new(items).highlight_style(Style::new().add_modifier(Modifier::REVERSED));
                f.render_stateful_widget(list, body, &mut state);
            })
            .map_err(|e| e.to_string())?;
        if !event::poll(Duration::from_millis(250)).map_err(|e| e.to_string())? {
            continue;
        }
        let mut open = None;
        match event::read().map_err(|e| e.to_string())? {
            Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Down | KeyCode::Char('j') => state.select(Some((state.selected().unwrap_or(0) + 1).min(jobs.len().saturating_sub(1)))),
                KeyCode::Up | KeyCode::Char('k') => state.select(Some(state.selected().unwrap_or(0).saturating_sub(1))),
                KeyCode::Enter => open = state.selected(),
                _ => {}
            },
            Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                if m.row == head_area.y {
                    if let Some((_, _, id)) = tab_ranges.iter().find(|(a, b, _)| m.column >= *a && m.column < *b) {
                        switch_to(id); // the same switch as the context view: swaps the chat, closes a Worker view
                        last_check = Instant::now() - Duration::from_secs(1);
                    }
                } else if let Some(row) = clicked_row(list_area, m.row, state.offset()) {
                    if row < jobs.len() {
                        state.select(Some(row));
                        open = Some(row);
                    }
                }
            }
            _ => {}
        }
        if let Some(i) = open {
            if let Some(job) = jobs.get(i).cloned() {
                note = match job.attempt.as_deref().map(|a| (a, session_of(a))) {
                    None => format!("{} has not started yet", job.key),
                    Some((a, None)) => format!("{a}: session path not cached yet, try again in a few seconds"),
                    Some((_, Some(path))) => {
                        toggle_worker_view(&exe, &job.key, &path);
                        String::new()
                    }
                };
            }
        }
    }
}

// ---------- dispatch (right bar) ----------

fn run_dispatch(terminal: &mut DefaultTerminal) -> Result<(), String> {
    loop {
        let load = fs::read_to_string("/proc/loadavg").unwrap_or_default();
        let load: Vec<&str> = load.split_whitespace().take(3).collect();
        terminal
            .draw(|f| {
                let text = vec![
                    Line::from(" homeserver".bold()),
                    Line::from(format!(" load {}", load.join(" "))),
                    Line::from(""),
                    Line::from(" PRs".bold()),
                    Line::from(" (future)".dim()),
                    Line::from(""),
                    Line::from(" git".bold()),
                    Line::from(" (future)".dim()),
                    Line::from(""),
                    Line::from(" Jira".bold()),
                    Line::from(" (future)".dim()),
                ];
                f.render_widget(Paragraph::new(text), f.area());
            })
            .map_err(|e| e.to_string())?;
        if event::poll(Duration::from_secs(2)).map_err(|e| e.to_string())? {
            if let Event::Key(k) = event::read().map_err(|e| e.to_string())? {
                if k.code == KeyCode::Char('q') {
                    return Ok(());
                }
            }
        }
    }
}

// ---------- worker (read-only chat) ----------

fn render_session(path: &str) -> Vec<Line<'static>> {
    let text = fs::read_to_string(path).unwrap_or_else(|e| format!("{{\"error\":\"{e}\"}}"));
    let mut lines = Vec::new();
    for raw in text.lines() {
        let Ok(v) = serde_json::from_str::<Value>(raw) else { continue };
        if v["type"] != "message" {
            continue;
        }
        let role = v["message"]["role"].as_str().unwrap_or("");
        let ts = v["timestamp"].as_str().unwrap_or("").get(11..19).unwrap_or("").to_string();
        let empty = Vec::new();
        let content = v["message"]["content"].as_array().unwrap_or(&empty);
        for part in content {
            match (role, part["type"].as_str().unwrap_or("")) {
                ("user", "text") => {
                    lines.push(Line::from(vec![format!("{ts} ").dim(), "you  ".fg(Color::Cyan).bold()]));
                    for l in part["text"].as_str().unwrap_or("").lines().take(12) {
                        lines.push(Line::from(format!("      {l}")).fg(Color::Cyan));
                    }
                }
                ("assistant", "text") => {
                    lines.push(Line::from(vec![format!("{ts} ").dim(), "agent".fg(Color::Green).bold()]));
                    for l in part["text"].as_str().unwrap_or("").lines() {
                        lines.push(Line::from(format!("      {l}")));
                    }
                }
                ("assistant", "toolCall") => {
                    let name = part["name"].as_str().unwrap_or("tool");
                    let arg = part["arguments"]["command"].as_str().or(part["arguments"]["path"].as_str()).unwrap_or("");
                    let arg: String = arg.lines().next().unwrap_or("").chars().take(160).collect();
                    lines.push(Line::from(vec![format!("{ts} ").dim(), format!("→ {name} ").fg(Color::Yellow), arg.into()]));
                }
                ("toolResult", "text") => {
                    let t = part["text"].as_str().unwrap_or("");
                    let last: String = t.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").chars().take(160).collect();
                    lines.push(Line::from(format!("        ← {last}")).dim());
                }
                _ => {}
            }
        }
    }
    lines
}

fn run_worker(terminal: &mut DefaultTerminal, path: String) -> Result<(), String> {
    let mut lines = render_session(&path);
    let mut scroll: usize = usize::MAX; // follow the end until the user scrolls
    let mut last_load = Instant::now();
    loop {
        if last_load.elapsed() > Duration::from_secs(2) {
            lines = render_session(&path);
            last_load = Instant::now();
        }
        let mut height = 0usize;
        terminal
            .draw(|f| {
                height = f.area().height as usize;
                let max = lines.len().saturating_sub(height);
                let top = scroll.min(max);
                let p = Paragraph::new(lines.clone())
                    .wrap(Wrap { trim: false })
                    .scroll((top as u16, 0));
                f.render_widget(p, f.area());
            })
            .map_err(|e| e.to_string())?;
        if !event::poll(Duration::from_millis(250)).map_err(|e| e.to_string())? {
            continue;
        }
        let max = lines.len().saturating_sub(height);
        let cur = scroll.min(max);
        match event::read().map_err(|e| e.to_string())? {
            Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                KeyCode::Up | KeyCode::Char('k') => scroll = cur.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => scroll = cur + 1,
                KeyCode::PageUp => scroll = cur.saturating_sub(height),
                KeyCode::PageDown => scroll = cur + height,
                KeyCode::End | KeyCode::Char('G') => scroll = usize::MAX,
                _ => {}
            },
            Event::Mouse(m) => match m.kind {
                MouseEventKind::ScrollUp => scroll = cur.saturating_sub(3),
                MouseEventKind::ScrollDown => scroll = cur + 3,
                _ => {}
            },
            _ => {}
        }
    }
}
