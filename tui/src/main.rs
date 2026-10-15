//! cpd-tui: the cockpit's views.
//!
//! One binary, four views, each meant to run in its own tmux pane of the cockpit (see `scripts/cockpit.sh`):
//! - `context`:   left bar. Context → the Lead sessions running in the cockpit (discovered from tmux, named by their pi
//!              names); Enter/click switches the centre (tmux swap-pane of the session's chat pane); `n` forks the
//!              session into a new Plan, `x` closes it (one inline prompt each, see `prompt.rs`).
//! - `plan`:    top centre. The Jobs of the Workflow of the session in the centre; tabs are the Context's sessions.
//!              Enter/click on a Job opens the Worker's chat read-only beside the chat.
//! - `dispatch`: right bar. The PR status of each Plan (read from `gh`, interim), the homeserver load, placeholders.
//! - `worker <session.jsonl> <job>`: read-only chat view of a pi session file (follows it live).
//! - `legend`:  what the colours and icons mean (`legend.rs`); the Context pane's button and `?` open it in a popup.
//!
//! Data comes only from the cpd CLI (`cpd … --wf`), fetched by the context view into a cache the other views read.

use std::{
    collections::HashSet,
    env, fs,
    path::PathBuf,
    process::Command,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use ratatui::{
    crossterm::{
        event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind},
        execute,
    },
    layout::{Constraint, Layout, Margin, Position, Rect},
    style::{Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    DefaultTerminal,
};
use serde_json::Value;
use tui_tree_widget::TreeState;

mod actions;
mod context;
mod flow;
mod legend;
mod palette;
mod pr;
mod prompt;
mod sessions;

use palette::Ui;
use prompt::{Outcome, Prompt};
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
        "legend" => run_legend(&mut terminal),
        _ => Err(format!("usage: {} context|plan|dispatch|worker <session.jsonl> <job>|legend", args[0])),
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

/// Run a tmux command on the cockpit's server; its stdout, or the error when tmux fails.
fn tmux_checked(args: &[&str]) -> Result<String, String> {
    let out = Command::new("tmux").arg("-L").arg(socket()).args(args).output().map_err(|e| format!("tmux: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!("tmux {}: {}", args.first().unwrap_or(&""), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// `n`: fork `source` into a new Plan called `name`, in a new hidden window `park-<name>` that runs `fork-lead.sh`
/// (it waits for the source's turn to finish, then forks; the new session shows in the tree at once). The centre stays.
fn fork_session(source: &Session, name: &str) -> Result<String, String> {
    if source.file.is_empty() {
        return Err("the session has no session file yet".into());
    }
    let exe = env::current_exe().map_err(|e| e.to_string())?;
    let scripts = actions::scripts_dir(&exe).ok_or("cannot find the scripts directory from the binary")?;
    if !scripts.join("fork-lead.sh").is_file() {
        return Err(format!("{} not found", scripts.join("fork-lead.sh").display()));
    }
    let id = uuid::Uuid::new_v4().to_string();
    let window = actions::window_name(name);
    let command = actions::fork_command(&scripts, &socket(), &source.file, &id, name);
    tmux_checked(&["new-window", "-d", "-t", &format!("{}:", session()), "-n", &window, "-c", &source.cwd, &command])?;
    Ok(format!("forking {name} in window {window}"))
}

/// `x` confirmed: kill the chat pane of session `id` (pi stops, the session file stays; a window that held only that
/// pane closes with it). Judged on a fresh listing, so a session swapped into the centre meanwhile is still refused.
fn close_session(id: &str) -> Result<String, String> {
    let all = discover();
    if let Some(why) = actions::close_refusal(&all, id) {
        return Err(why.into());
    }
    let pane = all.iter().find(|s| s.id == id).map(|s| s.pane_id.clone()).unwrap_or_default();
    tmux_checked(&["kill-pane", "-t", &pane])?;
    Ok("closed".into())
}

/// `?` or a click on the button: open the legend in a tmux popup over the whole cockpit (the Context pane is too
/// narrow for it), sized to the legend and at most the client's size. `display-popup` returns when the popup closes.
fn open_legend() -> Result<(), String> {
    let exe = env::current_exe().map_err(|e| e.to_string())?;
    let clients = tmux_checked(&["list-clients", "-F", "#{client_activity} #{client_width} #{client_height}"])?;
    let client = clients
        .lines()
        .filter_map(|l| {
            let n: Vec<u64> = l.split(' ').filter_map(|x| x.parse().ok()).collect();
            Some((*n.first()?, u16::try_from(*n.get(1)?).ok()?, u16::try_from(*n.get(2)?).ok()?))
        })
        .max()
        .ok_or("no tmux client attached")?;
    tmux_checked(&legend::popup_args(&exe, (client.1, client.2)).iter().map(String::as_str).collect::<Vec<_>>()).map(|_| ())
}

/// The legend view: draws until any key or click.
fn run_legend(terminal: &mut DefaultTerminal) -> Result<(), String> {
    loop {
        terminal.draw(|f| legend::draw(f, f.area())).map_err(|e| e.to_string())?;
        match event::read().map_err(|e| e.to_string())? {
            Event::Key(k) if k.kind == KeyEventKind::Press => return Ok(()),
            Event::Mouse(m) if matches!(m.kind, MouseEventKind::Down(_)) => return Ok(()),
            _ => {}
        }
    }
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
    mark_seen();
}

/// Stamp `@seen_at` (epoch seconds) on the chat pane in the centre slot: the User sees that session now, so a turn it
/// settled before this moment is no longer unseen (`context::needs_you`).
fn mark_seen() {
    let now = now_secs().to_string();
    for line in tmux(&["list-panes", "-t", &format!("{}:main", session()), "-F", "#{pane_id} #{@session_id}"]).lines() {
        if let Some((pane, id)) = line.split_once(' ') {
            if !id.is_empty() {
                tmux(&["set", "-p", "-t", pane, "@seen_at", &now]);
            }
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

/// A session's Workflow as the Context column shows it, from the cached `cpd workflow show`; `None` until it is fetched.
fn workflow_summary(plan: &str) -> Option<context::Workflow> {
    let v = read_json(&cache_file(plan))?;
    let phase = v["phase"].as_str()?.to_string();
    let counts = context::count_jobs(&phase, v["jobs"].as_array()?.iter().filter_map(|j| j["status"].as_str()));
    Some(context::Workflow { phase, counts })
}

/// The Context column's rows from the sessions grouped by Context.
fn context_rows(groups: &[(String, Vec<Entry>)]) -> Vec<context::ContextRow> {
    groups
        .iter()
        .map(|(name, list)| context::ContextRow {
            name: name.clone(),
            sessions: list
                .iter()
                .map(|e| context::SessionRow {
                    id: e.session.id.clone(),
                    label: e.label.clone(),
                    active: e.session.is_active(),
                    needs_you: context::needs_you(e.session.asking, &e.session.state, e.session.settled_at, e.session.seen_at),
                    workflow: e.workflow.as_deref().and_then(workflow_summary),
                })
                .collect(),
        })
        .collect()
}

fn target_of(e: &Entry) -> prompt::Target {
    prompt::Target { id: e.session.id.clone(), label: e.label.clone(), working: e.session.state == "working" }
}

/// The Context view's data: the sessions, the tree's rows, and the tree's state.
struct ContextData {
    groups: Vec<(String, Vec<Entry>)>,
    rows: Vec<context::ContextRow>,
    state: TreeState<String>,
    known: HashSet<String>,
}

impl ContextData {
    /// Stamp the centre chat as seen, re-read the sessions, and keep the selection on its row.
    fn refresh(&mut self) {
        mark_seen(); // before reading: the session in the centre never shows as unseen
        let before = context::selected_index(&self.state, &self.rows);
        self.groups = snapshot();
        self.rows = context_rows(&self.groups);
        context::open_new(&mut self.state, &mut self.known, &self.rows);
        context::reselect(&mut self.state, &self.rows, before);
    }

    /// The selected row's session; `None` on a Context row.
    fn selected_session(&self) -> Option<&Entry> {
        let id = self.state.selected().get(1)?;
        self.groups.iter().flat_map(|(_, l)| l).find(|e| &e.session.id == id)
    }
}

/// The button at the bottom row of the Context pane (also the key `?`).
const LEGEND_BUTTON: &str = " ? legend ";

fn run_context(terminal: &mut DefaultTerminal) -> Result<(), String> {
    thread::spawn(fetch_loop);
    let mut data = ContextData { groups: Vec::new(), rows: Vec::new(), state: TreeState::default(), known: HashSet::new() };
    data.refresh();
    let mut last_refresh = Instant::now();
    let mut prompt: Option<Prompt> = None;
    let mut note = String::new(); // the one-line result or refusal of the last action, until the next key
    let mut button = Rect::default(); // the legend button, set by each draw
    loop {
        if last_refresh.elapsed() > Duration::from_secs(1) {
            data.refresh();
            last_refresh = Instant::now();
        }
        terminal
            .draw(|f| {
                let area = f.area();
                let footer = prompt.as_ref().map(Prompt::text).unwrap_or_else(|| note.clone());
                let width = area.width.max(1) as usize;
                let wrapped = footer.chars().count().div_ceil(width);
                let footer_height = if footer.is_empty() { 0 } else { (wrapped + usize::from(wrapped > 1)).max(1).min(area.height as usize) as u16 };
                let [list_area, footer_area, button_area] = Layout::vertical([Constraint::Min(0), Constraint::Length(footer_height), Constraint::Length(1)]).areas(area);
                let label = Line::from(LEGEND_BUTTON);
                button = Rect { width: (label.width() as u16).min(button_area.width), ..button_area };
                f.render_widget(Paragraph::new(label).style(Style::new().add_modifier(Modifier::REVERSED)), button);
                if footer_height > 0 {
                    let style = if prompt.is_some() { Style::new().fg(palette::ui(Ui::Note)).bold() } else { Style::new().fg(palette::ui(Ui::Note)) };
                    f.render_widget(Paragraph::new(footer).style(style).wrap(Wrap { trim: false }), footer_area);
                }
                context::draw(f, list_area, &data.rows, &mut data.state);
            })
            .map_err(|e| e.to_string())?;
        if !event::poll(Duration::from_millis(250)).map_err(|e| e.to_string())? {
            continue;
        }
        let mut activate: Option<Vec<String>> = None; // a row to act on: toggle a Context, switch to a session
        let mut outcome = Outcome::Open;
        match event::read().map_err(|e| e.to_string())? {
            // an open prompt takes every key; mouse clicks are ignored meanwhile, so the target cannot be swapped into the centre
            Event::Key(k) if k.kind == KeyEventKind::Press && prompt.is_some() => {
                if let Some(p) = prompt.as_mut() {
                    outcome = p.key(k);
                }
            }
            Event::Key(k) if k.kind == KeyEventKind::Press => match k.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Char('n') => {
                    note.clear();
                    match (data.selected_session(), data.state.selected().len()) {
                        (Some(e), _) => prompt = Some(Prompt::fork(target_of(e))),
                        (None, 1) => note = "pick the session to fork".into(),
                        (None, _) => {}
                    }
                }
                KeyCode::Char('?') => {
                    if let Err(why) = open_legend() {
                        note = why;
                    }
                }
                KeyCode::Char('x') => {
                    note.clear();
                    if let Some(e) = data.selected_session() {
                        match actions::close_refusal(&discover(), &e.session.id) {
                            Some(why) => note = why.into(),
                            None => prompt = Some(Prompt::close(target_of(e))),
                        }
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    data.state.key_down();
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    data.state.key_up();
                }
                KeyCode::Enter => activate = Some(data.state.selected().to_vec()),
                KeyCode::Char(' ') if data.state.selected().len() == 1 => activate = Some(data.state.selected().to_vec()),
                _ => {}
            },
            Event::Mouse(m) if prompt.is_none() && m.kind == MouseEventKind::Down(MouseButton::Left) => {
                note.clear();
                if button.contains(Position::new(m.column, m.row)) {
                    if let Err(why) = open_legend() {
                        note = why;
                    }
                } else if let Some(row) = data.state.rendered_at(Position::new(m.column, m.row)).map(<[String]>::to_vec) {
                    data.state.select(row.clone());
                    activate = Some(row);
                }
            }
            _ => {}
        }
        match outcome {
            Outcome::Open => {}
            Outcome::Cancel => prompt = None,
            Outcome::Fork { id, name } => {
                prompt = None;
                let source = discover().into_iter().find(|s| s.id == id);
                note = match source.ok_or_else(|| "session is already gone".to_string()).and_then(|s| fork_session(&s, &name)) {
                    Ok(done) | Err(done) => done,
                };
            }
            Outcome::Close { id } => {
                prompt = None;
                note = match close_session(&id) {
                    Ok(done) | Err(done) => done,
                };
                last_refresh = Instant::now() - Duration::from_secs(2); // the tree drops the session on this loop's refresh
            }
        }
        match activate.as_deref() {
            Some([_]) => {
                data.state.toggle_selected();
            }
            Some([_, id]) => {
                switch_to(id);
                data.refresh();
                last_refresh = Instant::now();
            }
            _ => {}
        }
    }
}

// ---------- plan view (top centre) ----------

/// A Job as the plan view keeps it: what the flowchart draws, and the latest Attempt (what Enter/click opens).
struct JobRow {
    flow: flow::FlowJob,
    attempt: Option<String>,
}

/// The Workflow's title and all its Jobs (the flowchart hides the cancelled ones and counts them).
fn jobs_of(plan: &str) -> (String, Vec<JobRow>) {
    let v = read_json(&cache_file(plan)).unwrap_or(Value::Null);
    let rows = v["jobs"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|j| JobRow {
                    flow: flow::FlowJob {
                        key: j["key"].as_str().unwrap_or("?").into(),
                        status: j["status"].as_str().unwrap_or("?").into(),
                        tier: j["tier"].as_str().unwrap_or("").into(),
                        needs: j["needs"].as_array().map(|n| n.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
                        attempt: j["attempts"].as_array().map_or(0, |a| a.len()),
                    },
                    attempt: j["attempts"].as_array().and_then(|a| a.last()).and_then(|a| a["id"].as_str()).map(String::from),
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
    // The selection is held by Job key, so a refresh keeps it; the scroll offset is the window's top-left in the chart.
    let (mut selected, mut scroll, mut follow) = (None::<String>, (0u16, 0u16), true);
    let mut chart = None::<flow::Chart>;
    let mut body_area = Rect::default();
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
                (selected, scroll, follow) = (None, (0, 0), true);
            }
            (title, jobs) = match centre_now.as_ref().and_then(|c| c.active.workflow.as_deref()) {
                Some(wf) => jobs_of(wf), // a cache read: instant
                None => ("no Workflow yet".into(), Vec::new()),
            };
            last_check = Instant::now();
        }
        terminal
            .draw(|f| {
                let [head, _gap, body] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)]).areas(f.area());
                head_area = head;
                let flows: Vec<flow::FlowJob> = jobs.iter().map(|j| j.flow.clone()).collect();
                let fc = flow::layout(&flows);
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
                    if fc.hidden > 0 {
                        spans.push(format!("  {} cancelled hidden", fc.hidden).dim());
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
                    spans.push(format!("  {note}").fg(palette::ui(Ui::Note)));
                }
                // two columns and one row of padding: nothing of the chart touches the pane's edges
                let window = body.inner(Margin { horizontal: 2, vertical: 1 });
                body_area = window;
                f.render_widget(Paragraph::new(Line::from(spans)), head);
                let sel = selected.as_ref().and_then(|k| fc.pills.iter().position(|p| &p.key == k)).or_else(|| fc.columns.first().and_then(|c| c.first().copied()));
                selected = sel.map(|i| fc.pills[i].key.clone());
                if let (true, Some(i)) = (follow, sel) {
                    scroll = fc.scroll_to(i, scroll, (window.width, window.height));
                    follow = false;
                }
                scroll = fc.blit(f.buffer_mut(), window, scroll, sel);
                chart = Some(fc);
            })
            .map_err(|e| e.to_string())?;
        if !event::poll(Duration::from_millis(250)).map_err(|e| e.to_string())? {
            continue;
        }
        let mut open = None::<String>;
        let here = chart.as_ref().and_then(|c| Some((c, c.pills.iter().position(|p| Some(&p.key) == selected.as_ref())?)));
        match event::read().map_err(|e| e.to_string())? {
            Event::Key(k) if k.kind == KeyEventKind::Press => {
                let dir = match k.code {
                    KeyCode::Down | KeyCode::Char('j') => Some(flow::Dir::Down),
                    KeyCode::Up | KeyCode::Char('k') => Some(flow::Dir::Up),
                    KeyCode::Left | KeyCode::Char('h') => Some(flow::Dir::Left),
                    KeyCode::Right | KeyCode::Char('l') => Some(flow::Dir::Right),
                    _ => None,
                };
                match (k.code, dir, here) {
                    (KeyCode::Char('q'), ..) => return Ok(()),
                    (_, Some(d), Some((c, i))) => {
                        selected = Some(c.pills[c.step(i, d)].key.clone());
                        follow = true;
                    }
                    (KeyCode::Enter, ..) => open = selected.clone(),
                    _ => {}
                }
            }
            Event::Mouse(m) => match m.kind {
                MouseEventKind::Down(MouseButton::Left) if m.row == head_area.y => {
                    if let Some((_, _, id)) = tab_ranges.iter().find(|(a, b, _)| m.column >= *a && m.column < *b) {
                        switch_to(id); // the same switch as the context view: swaps the chat, closes a Worker view
                        last_check = Instant::now() - Duration::from_secs(1);
                    }
                }
                MouseEventKind::Down(MouseButton::Left) => {
                    let hit = chart.as_ref().filter(|_| body_area.contains(Position::new(m.column, m.row))).and_then(|c| {
                        c.pill_at(m.column - body_area.x + scroll.0, m.row - body_area.y + scroll.1).map(|i| c.pills[i].key.clone())
                    });
                    if let Some(key) = hit {
                        selected = Some(key.clone());
                        open = Some(key);
                    }
                }
                MouseEventKind::ScrollUp => scroll.1 = scroll.1.saturating_sub(3),
                MouseEventKind::ScrollDown => scroll.1 = scroll.1.saturating_add(3), // clamped when drawn
                MouseEventKind::ScrollLeft => scroll.0 = scroll.0.saturating_sub(4),
                MouseEventKind::ScrollRight => scroll.0 = scroll.0.saturating_add(4),
                _ => {}
            },
            _ => {}
        }
        if let Some(key) = open {
            if let Some(job) = jobs.iter().find(|j| j.flow.key == key) {
                note = match job.attempt.as_deref().map(|a| (a, session_of(a))) {
                    None => format!("{key} has not started yet"),
                    Some((a, None)) => format!("{a}: session path not cached yet, try again in a few seconds"),
                    Some((_, Some(path))) => {
                        toggle_worker_view(&exe, &key, &path);
                        String::new()
                    }
                };
            }
        }
    }
}

// ---------- dispatch (right bar) ----------

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn pr_cache_name(plan: &str) -> String {
    format!("pr-{plan}")
}

/// The Plans in the cockpit, once each, labelled as in the Context column.
fn plans() -> Vec<(String, String)> {
    let mut seen = HashSet::new();
    snapshot().into_iter().flat_map(|(_, l)| l).filter_map(|e| Some((e.label, e.workflow?))).filter(|(_, wf)| seen.insert(wf.clone())).collect()
}

/// `(pr number, repo)` of a Plan from its cached `cpd workflow show`; the number is `None` while `pr` is null.
fn pr_of(plan: &str) -> Option<(Option<u64>, String)> {
    let w = &read_json(&cache_file(plan))?["workflow"];
    Some((w["pr"]["number"].as_u64(), w["repo"].as_str()?.to_string()))
}

/// Read-only `gh pr view` in the Workflow's repo (never changes a PR); `None` when gh fails, times out or answers badly.
fn gh_pr_view(repo: &str, number: u64) -> Option<Value> {
    let out = Command::new("timeout").args(["30", "gh", "pr", "view", &number.to_string(), "--json", pr::GH_FIELDS]).current_dir(repo).output().ok()?;
    let v: Value = serde_json::from_slice(&out.stdout).ok().filter(|_| out.status.success())?;
    pr::parse_live(&v).ok().map(|_| v)
}

/// Off the draw loop: every Plan's PR is read by gh at most once a minute (`pr::due`) into `cache/pr-<plan>.json`.
fn pr_loop() {
    let _ = fs::create_dir_all(format!("{}/cache", state_dir()));
    loop {
        for (_, plan) in plans() {
            let Some((Some(number), repo)) = pr_of(&plan) else { continue };
            let name = pr_cache_name(&plan);
            let prev = read_json(&cache_file(&name)).and_then(|v| pr::Record::from_json(&v));
            let now = now_secs();
            if pr::due(prev.as_ref(), number, now) {
                let rec = pr::Record::after(prev.as_ref(), number, now, gh_pr_view(&repo, number));
                write_atomic(&cache_file(&name), &rec.to_json().to_string());
            }
        }
        thread::sleep(Duration::from_secs(5));
    }
}

fn run_dispatch(terminal: &mut DefaultTerminal) -> Result<(), String> {
    thread::spawn(pr_loop);
    loop {
        let load = fs::read_to_string("/proc/loadavg").unwrap_or_default();
        let load: Vec<&str> = load.split_whitespace().take(3).collect();
        let now = now_secs();
        let rows: Vec<(String, Option<u64>, Option<pr::Record>)> = plans()
            .into_iter()
            .map(|(label, plan)| {
                let number = pr_of(&plan).and_then(|p| p.0);
                (label, number, read_json(&cache_file(&pr_cache_name(&plan))).and_then(|v| pr::Record::from_json(&v)))
            })
            .collect();
        terminal
            .draw(|f| {
                let width = f.area().width as usize;
                let mut text = vec![Line::from(" homeserver".bold()), Line::from(format!(" load {}", load.join(" "))), Line::from(""), Line::from(" PRs".bold())];
                // plans with a PR first; those without only while the pane has room for the rest
                let room = (f.area().height as usize).saturating_sub(text.len() + 6);
                let mut used = 0;
                for (label, number, rec) in rows.iter().filter(|r| r.1.is_some()).chain(rows.iter().filter(|r| r.1.is_none())) {
                    let lines = pr::render(label, *number, rec.as_ref(), now, width);
                    if number.is_none() && used + lines.len() > room {
                        continue;
                    }
                    used += lines.len();
                    text.extend(lines);
                }
                if rows.is_empty() {
                    text.push(Line::from(" no Plans in the cockpit".dim()));
                }
                text.extend([Line::from(""), Line::from(" git".bold()), Line::from(" (future)".dim()), Line::from(""), Line::from(" Jira".bold()), Line::from(" (future)".dim())]);
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
                    lines.push(Line::from(vec![format!("{ts} ").dim(), "you  ".fg(palette::ui(Ui::You)).bold()]));
                    for l in part["text"].as_str().unwrap_or("").lines().take(12) {
                        lines.push(Line::from(format!("      {l}")).fg(palette::ui(Ui::You)));
                    }
                }
                ("assistant", "text") => {
                    lines.push(Line::from(vec![format!("{ts} ").dim(), "agent".fg(palette::ui(Ui::Agent)).bold()]));
                    for l in part["text"].as_str().unwrap_or("").lines() {
                        lines.push(Line::from(format!("      {l}")));
                    }
                }
                ("assistant", "toolCall") => {
                    let name = part["name"].as_str().unwrap_or("tool");
                    let arg = part["arguments"]["command"].as_str().or(part["arguments"]["path"].as_str()).unwrap_or("");
                    let arg: String = arg.lines().next().unwrap_or("").chars().take(160).collect();
                    lines.push(Line::from(vec![format!("{ts} ").dim(), format!("→ {name} ").fg(palette::ui(Ui::Tool)), arg.into()]));
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
