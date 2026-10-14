//! cpd-tui: the cockpit's views.
//!
//! One binary, four views, each meant to run in its own tmux pane of the cockpit (see `scripts/cockpit.sh`):
//! - `tree`:    left bar. Context → Plans; Enter/click switches the centre (tmux swap-pane of the chat panes).
//! - `plan`:    top centre. Jobs of the active plan; Enter/click opens the Worker's chat read-only in a tmux popup.
//! - `widgets`: right bar placeholder.
//! - `log <session.jsonl> <job>`: read-only chat view of a pi session file (follows it live).
//!
//! Data comes only from the cpd CLI (`cpd … --wf`), fetched by the tree into a cache the other views read.

use std::{
    env, fs,
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

mod tree;

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
/// The Context's name and its Plans (workflow ids), from `CPD_TUI_CONTEXT` and `CPD_TUI_PLANS` (space-separated).
struct Config {
    context: String,
    plans: Vec<String>,
}

fn config() -> Result<Config, String> {
    let var = |name: &str| env::var(name).ok().filter(|v| !v.trim().is_empty()).ok_or(format!("{name} is not set"));
    let context = var("CPD_TUI_CONTEXT")?.trim().to_string();
    let plans = var("CPD_TUI_PLANS")?.split_whitespace().map(String::from).collect();
    Ok(Config { context, plans })
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let view = args.get(1).map(String::as_str).unwrap_or("");
    let mut terminal = ratatui::init();
    let _ = execute!(std::io::stdout(), EnableMouseCapture);
    let result = match view {
        "tree" => run_tree(&mut terminal),
        "plan" => run_plan(&mut terminal),
        "widgets" => run_widgets(&mut terminal),
        "log" => run_log(&mut terminal, args.get(2).cloned().unwrap_or_default()),
        _ => Err(format!("usage: {} tree|plan|widgets|log <session.jsonl>", args[0])),
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
    serde_json::from_slice(&out.stdout).ok()
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

fn active_plan(cfg: &Config) -> String {
    fs::read_to_string(format!("{}/active", state_dir())).map(|s| s.trim().to_string()).unwrap_or_else(|_| cfg.plans[0].clone())
}

/// Bring `plan`'s chat pane into the centre slot. Chat panes carry the tmux user option `@plan`; the one in window
/// `main` is the centre slot. swap-pane keeps both processes running and the slot keeps its size (verified).
fn switch_to(plan: &str) {
    for line in tmux(&["list-panes", "-t", &format!("{}:main", session()), "-F", "#{pane_id} #{@view}"]).lines() {
        if line.ends_with(" worker") {
            tmux(&["kill-pane", "-t", line.split(' ').next().unwrap_or("")]);
        }
    }
    let panes = tmux(&["list-panes", "-a", "-F", "#{pane_id} #{window_name} #{@plan}"]);
    let (mut current, mut target) = (None, None);
    for line in panes.lines() {
        let f: Vec<&str> = line.split(' ').collect();
        if f.len() < 3 || f[2].is_empty() {
            continue;
        }
        if f[1] == "main" {
            current = Some(f[0].to_string());
        }
        if f[2] == plan {
            target = Some(f[0].to_string());
        }
    }
    if let (Some(c), Some(t)) = (current, target) {
        if c != t {
            tmux(&["swap-pane", "-d", "-s", &t, "-t", &c]);
        }
    }
    let _ = fs::create_dir_all(state_dir());
    let _ = fs::write(format!("{}/active", state_dir()), plan);
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
fn fetch_loop(plans: Vec<String>) {
    let _ = fs::create_dir_all(format!("{}/cache", state_dir()));
    let mut sessions: serde_json::Map<String, Value> = read_json(&cache_file("sessions")).and_then(|v| v.as_object().cloned()).unwrap_or_default();
    loop {
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

/// plan -> agent state, published into each chat pane by tmux-agent-state.ts (or the resume script).
fn agent_states() -> std::collections::HashMap<String, String> {
    tmux(&["list-panes", "-a", "-F", "#{@plan}\t#{@agent_state}"])
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() >= 2 && !f[0].is_empty()).then(|| (f[0].to_string(), f[1].to_string()))
        })
        .collect()
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

// ---------- tree (left bar) ----------

#[derive(Clone, Default)]
struct PlanSummary {
    key: String,
    phase: String,
    merged: usize,
    total: usize,
    failed: usize,
}

fn summarize(plan: &str) -> PlanSummary {
    let v = read_json(&cache_file(plan)).unwrap_or(Value::Null);
    let jobs: Vec<&Value> = v["jobs"].as_array().map(|a| a.iter().filter(|j| j["status"] != "cancelled").collect()).unwrap_or_default();
    PlanSummary {
        key: plan.to_string(),
        phase: v["phase"].as_str().unwrap_or("?").to_string(),
        merged: jobs.iter().filter(|j| j["status"] == "closed").count(),
        total: jobs.len(),
        failed: jobs.iter().filter(|j| j["status"] == "failed").count(),
    }
}

/// One Plan's line: branch, state icon, name, phase, merged/total, failed. No label text.
fn plan_line(p: &PlanSummary, last: bool, state: &str, active: bool, selected: bool) -> Line<'static> {
    let (icon, color) = tree::state_icon(state);
    let name = if active { p.key.clone().cyan().bold() } else { Span::from(p.key.clone()) };
    let mut spans = vec![format!(" {} ", if last { "└" } else { "├" }).into(), icon.fg(color), " ".into(), name, format!("  {}", p.phase).dim(), format!("  {}/{}", p.merged, p.total).fg(Color::Green)];
    if p.failed > 0 {
        spans.push(format!(" ✗{}", p.failed).fg(Color::Red));
    }
    let line = Line::from(spans);
    if selected { line.reversed() } else { line }
}

fn context_line(name: &str, collapsed: bool, selected: bool) -> Line<'static> {
    let line = Line::from(vec![format!("{} ", if collapsed { "▸" } else { "▾" }).into(), name.to_string().bold(), "  (Context)".dim()]);
    if selected { line.reversed() } else { line }
}

fn run_tree(terminal: &mut DefaultTerminal) -> Result<(), String> {
    let cfg = match config() {
        Ok(c) => c,
        Err(e) => return run_error(terminal, &e),
    };
    let fetch_plans = cfg.plans.clone();
    thread::spawn(move || fetch_loop(fetch_plans));
    let mut collapsed = false;
    let mut selected = 0usize; // index into the visible rows
    let mut area = Rect::default();
    loop {
        let plans: Vec<PlanSummary> = cfg.plans.iter().map(|p| summarize(p)).collect();
        let states = agent_states();
        let active = active_plan(&cfg);
        let rows = tree::visible_rows(plans.len(), collapsed);
        selected = selected.min(rows.len() - 1);
        terminal
            .draw(|f| {
                area = f.area();
                let lines: Vec<Line> = rows
                    .iter()
                    .enumerate()
                    .map(|(i, row)| match *row {
                        tree::Row::Context => context_line(&cfg.context, collapsed, i == selected),
                        tree::Row::Plan(n) => {
                            let p = &plans[n];
                            plan_line(p, n + 1 == plans.len(), states.get(&p.key).map(String::as_str).unwrap_or(""), p.key == active, i == selected)
                        }
                    })
                    .collect();
                f.render_widget(Paragraph::new(lines), area);
            })
            .map_err(|e| e.to_string())?;
        if !event::poll(Duration::from_millis(250)).map_err(|e| e.to_string())? {
            continue;
        }
        let mut activate = None;
        match event::read().map_err(|e| e.to_string())? {
            Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Down | KeyCode::Char('j') => selected = (selected + 1).min(rows.len() - 1),
                KeyCode::Up | KeyCode::Char('k') => selected = selected.saturating_sub(1),
                KeyCode::Enter => activate = Some(rows[selected]),
                KeyCode::Char(' ') if rows[selected] == tree::Row::Context => activate = Some(tree::Row::Context),
                _ => {}
            },
            Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                if let Some(i) = tree::row_at(plans.len(), collapsed, area.y, m.row) {
                    selected = i;
                    activate = Some(rows[i]);
                }
            }
            _ => {}
        }
        match activate {
            Some(tree::Row::Context) => {
                collapsed = !collapsed;
                selected = 0;
            }
            Some(tree::Row::Plan(n)) => switch_to(&plans[n].key),
            None => {}
        }
    }
}

/// A view that cannot start shows why on one line (no silent default); `q` quits.
fn run_error(terminal: &mut DefaultTerminal, message: &str) -> Result<(), String> {
    loop {
        terminal.draw(|f| f.render_widget(Paragraph::new(Line::from(format!("error: {message}").red())).wrap(Wrap { trim: false }), f.area())).map_err(|e| e.to_string())?;
        if event::poll(Duration::from_millis(500)).map_err(|e| e.to_string())? {
            if let Event::Key(k) = event::read().map_err(|e| e.to_string())? {
                if k.code == KeyCode::Char('q') {
                    return Ok(());
                }
            }
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
    let panes = tmux(&["list-panes", "-t", &format!("{}:main", session()), "-F", "#{pane_id} #{@plan} #{@view} #{@job}"]);
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
    let cmd = format!("{exe} log '{path}' '{job}'");
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

fn run_plan(terminal: &mut DefaultTerminal) -> Result<(), String> {
    let cfg = match config() {
        Ok(c) => c,
        Err(e) => return run_error(terminal, &e),
    };
    let exe = env::current_exe().map_err(|e| e.to_string())?.display().to_string();
    let (mut plan, mut title, mut jobs) = (active_plan(&cfg), String::from("loading…"), Vec::<JobRow>::new());
    let mut state = ListState::default().with_selected(Some(0));
    let mut list_area = Rect::default();
    let mut head_area = Rect::default();
    let mut tab_ranges: Vec<(u16, u16, String)> = Vec::new();
    let mut note = String::new();
    let mut last_active_check = Instant::now();
    loop {
        if last_active_check.elapsed() > Duration::from_millis(300) {
            let active = active_plan(&cfg);
            if active != plan {
                note.clear();
            }
            plan = active;
            (title, jobs) = jobs_of(&plan); // a cache read: instant
            last_active_check = Instant::now();
        }
        terminal
            .draw(|f| {
                let [head, body] = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(f.area());
                head_area = head;
                tab_ranges.clear();
                let mut spans: Vec<Span> = vec![format!(" {} ▸ ", cfg.context).dim()];
                let mut x = head.x + (cfg.context.chars().count() as u16) + 4;
                for p in &cfg.plans {
                    let label = format!(" {p} ");
                    let w = label.chars().count() as u16;
                    tab_ranges.push((x, x + w, p.to_string()));
                    spans.push(if *p == plan { label.reversed().bold() } else { label.into() });
                    spans.push(" ".into());
                    x += w + 1;
                }
                let age = cache_age(&plan).map(|a| format!("  updated {a}s ago")).unwrap_or_else(|| "  loading…".into());
                spans.push(format!("  {title}").into());
                spans.push(age.dim());
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
                    if let Some((_, _, p)) = tab_ranges.iter().find(|(a, b, _)| m.column >= *a && m.column < *b) {
                        let p = p.clone();
                        if p != plan {
                            switch_to(&p); // the same switch as the tree: swaps the chat, closes a Worker view
                        }
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
                // feedback first, then act (all reads are cache reads now)
                note = format!("opening {}…", job.key);
                terminal.draw(|f| {
                    let [head, _] = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(f.area());
                    f.render_widget(Paragraph::new(Line::from(vec![format!(" {plan} ").reversed().bold(), format!("  {note}").yellow()])), head);
                }).map_err(|e| e.to_string())?;
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

// ---------- widgets (right bar) ----------

fn run_widgets(terminal: &mut DefaultTerminal) -> Result<(), String> {
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

// ---------- log (read-only chat) ----------

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

fn run_log(terminal: &mut DefaultTerminal, path: String) -> Result<(), String> {
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
