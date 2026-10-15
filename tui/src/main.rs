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
        event::{self, DisableFocusChange, DisableMouseCapture, EnableFocusChange, EnableMouseCapture, Event, KeyCode, KeyEventKind, MouseButton, MouseEventKind},
        execute,
    },
    layout::{Constraint, Layout, Margin, Position, Rect},
    style::{Modifier, Style, Stylize},
    text::Line,
    widgets::{Paragraph, Wrap},
    DefaultTerminal,
};
use serde_json::Value;
use tui_tree_widget::TreeState;

mod actions;
mod context;
mod cursor;
mod flow;
mod legend;
mod palette;
mod pr;
mod prompt;
mod separator;
mod sessions;
mod statusbar;
mod tabs;
mod workers;

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
    let _ = execute!(std::io::stdout(), EnableMouseCapture, EnableFocusChange);
    let result = match view {
        "context" => run_context(&mut terminal),
        "plan" => run_plan(&mut terminal),
        "dispatch" => run_dispatch(&mut terminal),
        "worker" => run_worker(&mut terminal, args.get(2).cloned().unwrap_or_default()),
        "legend" => run_legend(&mut terminal),
        "statusbar" => run_statusbar(&mut terminal),
        _ => Err(format!("usage: {} context|plan|dispatch|worker <session.jsonl> <job>|legend|statusbar", args[0])),
    };
    let _ = execute!(std::io::stdout(), DisableFocusChange, DisableMouseCapture);
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

/// The session in the centre slot: the one chat pane in window `main`. One cheap tmux call (not logged: the views ask
/// on every 250 ms loop); both views take the active session from here, so the tree's `⏵` and the tab highlight agree.
fn centre_id() -> Option<String> {
    let panes = tmux_checked(&["list-panes", "-t", &format!("{}:main", session()), "-F", "#{@session_id}"]).ok()?;
    panes.lines().find(|l| !l.is_empty()).map(String::from)
}

/// Bring the chat pane of session `id` into the centre slot (the chat pane in window `main`), and focus it so typing
/// goes to the chat. swap-pane keeps both processes running and the slot keeps its size (verified).
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
    if let Some(t) = target {
        tmux(&["select-pane", "-t", &t.pane_id]); // after the swap this pane is the centre chat
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
    /// The session in the centre, as of the last refresh.
    active: Option<String>,
}

impl ContextData {
    /// Stamp the centre chat as seen, re-read the sessions, and keep the selection on its row.
    fn refresh(&mut self) {
        mark_seen(); // before reading: the session in the centre never shows as unseen
        let before = context::selected_index(&self.state, &self.rows);
        self.groups = snapshot();
        self.active = self.groups.iter().flat_map(|(_, l)| l).find(|e| e.session.is_active()).map(|e| e.session.id.clone());
        self.rows = context_rows(&self.groups);
        context::open_new(&mut self.state, &mut self.known, &self.rows);
        context::reselect(&mut self.state, &self.rows, before);
    }

    /// The cursor's session; `None` when there is none.
    fn selected_session(&self) -> Option<&Entry> {
        let id = self.state.selected().get(1)?;
        self.groups.iter().flat_map(|(_, l)| l).find(|e| &e.session.id == id)
    }
}

/// The legend button of the StatusBar (the key `?` in Context opens the same legend).
const LEGEND_BUTTON: &str = " ? legend ";

fn run_context(terminal: &mut DefaultTerminal) -> Result<(), String> {
    thread::spawn(fetch_loop);
    let mut data = ContextData { groups: Vec::new(), rows: Vec::new(), state: TreeState::default(), known: HashSet::new(), active: None };
    data.refresh();
    let mut last_refresh = Instant::now();
    // Keyboard focus of this pane, from the terminal's focus events (`focus-events on`); the cursor shows only while focused.
    let mut focused = env::var("TMUX_PANE").ok().and_then(|p| tmux_checked(&["display", "-p", "-t", &p, "#{pane_active}"]).ok()).is_some_and(|a| a.trim() == "1");
    let mut last_input = cursor::Input::Click; // the cursor stays hidden until the first arrow key
    let mut prompt: Option<Prompt> = None;
    let mut note = String::new(); // the one-line result or refusal of the last action, until the next key
    loop {
        // the centre is re-checked on every loop, so a switch from the tabs or from tmux shows here within a loop
        if last_refresh.elapsed() > Duration::from_secs(1) || centre_id() != data.active {
            data.refresh();
            last_refresh = Instant::now();
        }
        let cursor_shown = cursor::shown(focused, last_input, !data.state.selected().is_empty());
        terminal
            .draw(|f| {
                let area = f.area();
                let footer = prompt.as_ref().map(Prompt::text).unwrap_or_else(|| note.clone());
                let width = area.width.max(1) as usize;
                let wrapped = footer.chars().count().div_ceil(width);
                let footer_height = if footer.is_empty() { 0 } else { (wrapped + usize::from(wrapped > 1)).max(1).min(area.height as usize) as u16 };
                let [list_area, footer_area] = Layout::vertical([Constraint::Min(0), Constraint::Length(footer_height)]).areas(area);
                if footer_height > 0 {
                    let style = if prompt.is_some() { Style::new().fg(palette::ui(Ui::Note)).bold() } else { Style::new().fg(palette::ui(Ui::Note)) };
                    f.render_widget(Paragraph::new(footer).style(style).wrap(Wrap { trim: false }), footer_area);
                }
                context::draw(f, list_area, &data.rows, &mut data.state, cursor_shown);
            })
            .map_err(|e| e.to_string())?;
        if !event::poll(Duration::from_millis(250)).map_err(|e| e.to_string())? {
            continue;
        }
        let mut activate: Option<String> = None; // a session to switch to
        let mut outcome = Outcome::Open;
        match event::read().map_err(|e| e.to_string())? {
            Event::FocusGained => focused = true,
            Event::FocusLost => {
                focused = false;
                last_input = cursor::Input::Click;
            }
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
                    if let Some(e) = data.selected_session().filter(|_| cursor_shown) {
                        prompt = Some(Prompt::fork(target_of(e)));
                    }
                }
                KeyCode::Char('?') => {
                    if let Err(why) = open_legend() {
                        note = why;
                    }
                }
                KeyCode::Char('x') => {
                    note.clear();
                    if let Some(e) = data.selected_session().filter(|_| cursor_shown) {
                        match actions::close_refusal(&discover(), &e.session.id) {
                            Some(why) => note = why.into(),
                            None => prompt = Some(Prompt::close(target_of(e))),
                        }
                    }
                }
                // the first arrow after a click only shows the cursor again where it was (the first session when there is none)
                KeyCode::Down | KeyCode::Char('j') | KeyCode::Up | KeyCode::Char('k') => {
                    let delta = if matches!(k.code, KeyCode::Down | KeyCode::Char('j')) { 1 } else { -1 };
                    if cursor_shown {
                        context::move_cursor(&mut data.state, &data.rows, delta);
                    } else if data.state.selected().is_empty() {
                        context::move_cursor(&mut data.state, &data.rows, 1);
                    }
                    last_input = cursor::Input::Arrow;
                }
                KeyCode::Enter => activate = data.selected_session().filter(|_| cursor_shown).map(|e| e.session.id.clone()),
                _ => {}
            },
            Event::Mouse(m) if prompt.is_none() && m.kind == MouseEventKind::Down(MouseButton::Left) => {
                note.clear();
                last_input = cursor::Input::Click;
                if let Some(row) = data.state.rendered_at(Position::new(m.column, m.row)).map(<[String]>::to_vec) {
                    // a Context row only collapses or expands; a session row switches to it
                    if !context::click_context(&mut data.state, &row) {
                        if let [_, id] = row.as_slice() {
                            activate = Some(id.clone());
                        }
                    }
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
        if let Some(id) = activate {
            switch_to(&id);
            data.refresh();
            last_refresh = Instant::now();
        }
    }
}

/// The StatusBar view (one row, the bottom of the cockpit): the legend button on the left, the workers indicator on the
/// right (re-read every 2 s). A click on the button opens the legend; the bar never takes keyboard focus (`tmux.conf`).
fn run_statusbar(terminal: &mut DefaultTerminal) -> Result<(), String> {
    let home = cpd_home();
    let mut workers = workers::read(&home);
    let mut last_workers = Instant::now();
    let mut clickable = Vec::new();
    let mut note = String::new(); // why the legend did not open, shown in the middle until the next click
    keep_one_row(); // a respawned pane keeps its old size
    let mut last_check = Instant::now();
    loop {
        if last_workers.elapsed() > Duration::from_secs(2) {
            workers = workers::read(&home);
            last_workers = Instant::now();
        }
        terminal
            .draw(|f| {
                let (left, right) = statusbar_items(&workers, &note, f.area().width);
                clickable = statusbar::draw(f, f.area(), &left, &right);
            })
            .map_err(|e| e.to_string())?;
        let ready = event::poll(Duration::from_millis(250)).map_err(|e| e.to_string())?;
        let ev = if ready { Some(event::read().map_err(|e| e.to_string())?) } else { None };
        if matches!(ev, Some(Event::Resize(..))) || last_check.elapsed() > Duration::from_millis(500) {
            keep_one_row();
            last_check = Instant::now();
        }
        let Some(ev) = ev else { continue };
        if let Event::Mouse(m) = ev {
            if m.kind == MouseEventKind::Down(MouseButton::Left) {
                note.clear();
                let at = Position::new(m.column, m.row);
                if let Some((_, statusbar::Action::Legend)) = clickable.iter().find(|(r, _)| r.contains(at)) {
                    if let Err(why) = open_legend() {
                        note = why;
                    }
                }
            }
        }
    }
}

/// Resize this pane back to one row when its height is not 1 (tmux has no fixed-height panes), unless the window is
/// zoomed. The height is tmux's own (`#{pane_height}`), not the terminal's: on unzoom tmux restores the pane's size
/// without telling the program, so a resize that happened while zoomed is only seen by asking tmux. A failure is
/// ignored: the next check tries again.
fn keep_one_row() {
    let Ok(pane) = env::var("TMUX_PANE") else { return };
    let Ok(out) = tmux_checked(&["display-message", "-p", "-t", &pane, "#{window_zoomed_flag} #{pane_height}"]) else { return };
    let mut parts = out.split_whitespace();
    let (Some(zoomed), Some(Ok(height))) = (parts.next(), parts.next().map(str::parse::<u16>)) else { return };
    if statusbar::needs_resize(height, zoomed == "1") {
        let _ = tmux_checked(&["resize-pane", "-t", &pane, "-y", "1"]);
    }
}

/// The StatusBar's items: the legend button (left), then the workers indicator (right). A failure note, when there is
/// one, goes left after the button.
fn statusbar_items(workers: &workers::Workers, note: &str, width: u16) -> (Vec<statusbar::Item>, Vec<statusbar::Item>) {
    let mut left = vec![statusbar::Item { line: Line::from(LEGEND_BUTTON).style(Style::new().add_modifier(Modifier::REVERSED)), action: Some(statusbar::Action::Legend) }];
    if !note.is_empty() {
        left.push(statusbar::Item { line: Line::from(note.to_string()), action: None });
    }
    let right = workers.line(width).map(|line| statusbar::Item { line, action: None }).into_iter().collect();
    (left, right)
}

// ---------- plan view (top centre) ----------

/// A Job as the plan view keeps it: what the flowchart draws, and the latest Attempt (what Enter/click opens).
struct JobRow {
    flow: flow::FlowJob,
    attempt: Option<String>,
}

/// All the Jobs of a Workflow (the flowchart hides the cancelled ones); `None` until its first fetch is cached.
fn jobs_of(plan: &str) -> Option<Vec<JobRow>> {
    let v = read_json(&cache_file(plan))?;
    let rows = v["jobs"]
        .as_array()?
        .iter()
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
        .collect();
    Some(rows)
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

/// Close the Worker pane when it shows `job`; any other Worker view stays.
fn close_worker_view(job: &str) {
    let panes = tmux(&["list-panes", "-t", &format!("{}:main", session()), "-F", "#{pane_id} #{@view} #{@job}"]);
    for line in panes.lines() {
        let f: Vec<&str> = line.split(' ').collect();
        if f.len() >= 3 && f[1] == "worker" && f[2] == job {
            tmux(&["kill-pane", "-t", f[0]]);
        }
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

/// What the chart area says instead of a chart: no Workflow yet, loading, or the note of the last action.
fn plan_message(centre: Option<&Centre>, loaded: bool, note: &str) -> Option<String> {
    let status = match centre.map(|c| c.active.workflow.is_some()) {
        None => None,
        Some(false) => Some("no Workflow yet".to_string()),
        Some(true) => (!loaded).then(|| "loading… (first fetch)".to_string()),
    };
    match (status, note.is_empty()) {
        (Some(s), true) => Some(s),
        (Some(s), false) => Some(format!("{s}  {note}")),
        (None, false) => Some(note.to_string()),
        (None, true) => None,
    }
}

fn run_plan(terminal: &mut DefaultTerminal) -> Result<(), String> {
    let exe = env::current_exe().map_err(|e| e.to_string())?.display().to_string();
    let (mut centre_now, mut loaded, mut jobs) = (None::<Centre>, false, Vec::<JobRow>::new());
    // The selection is held by Job key, so a refresh keeps it; the scroll offset is the window's top-left in the chart.
    let (mut selected, mut scroll) = (flow::Selection::default(), (0u16, 0u16));
    let mut chart = None::<flow::Chart>;
    let mut body_area = Rect::default();
    let mut head_area = Rect::default();
    let mut tab_ranges: Vec<(u16, u16, String)> = Vec::new();
    let mut note = String::new();
    let mut last_check = Instant::now() - Duration::from_secs(1);
    loop {
        // the centre is re-checked on every loop (one tmux call), so a switch from the tree or from tmux shows at once
        let centre_changed = centre_id() != centre_now.as_ref().map(|c| c.active.session.id.clone());
        if centre_changed || last_check.elapsed() > Duration::from_secs(1) {
            let before = centre_now.as_ref().map(|c| c.active.session.id.clone());
            centre_now = centre();
            if before != centre_now.as_ref().map(|c| c.active.session.id.clone()) {
                note.clear();
                selected.reset();
                scroll = (0, 0);
            }
            let rows = centre_now.as_ref().and_then(|c| c.active.workflow.as_deref()).and_then(jobs_of); // a cache read: instant
            loaded = rows.is_some();
            jobs = rows.unwrap_or_default();
            last_check = Instant::now();
        }
        terminal
            .draw(|f| {
                let [head, body] = Layout::vertical([Constraint::Length(tabs::HEIGHT), Constraint::Min(1)]).areas(f.area());
                let flows: Vec<flow::FlowJob> = jobs.iter().map(|j| j.flow.clone()).collect();
                let fc = flow::layout(&flows);
                let tab_list: Vec<tabs::Tab> = centre_now.iter().flat_map(|c| &c.tabs).map(|e| tabs::Tab { id: e.session.id.clone(), label: e.label.clone(), active: e.session.is_active() }).collect();
                (head_area, tab_ranges) = tabs::draw(f, head, centre_now.as_ref().map(|c| c.context.as_str()), &tab_list);
                // two columns and one row of padding: nothing of the chart touches the pane's edges
                let mut window = body.inner(Margin { horizontal: 2, vertical: 1 });
                if let Some(message) = plan_message(centre_now.as_ref(), loaded, &note) {
                    let [chart_area, message_area] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(window);
                    f.render_widget(Paragraph::new(message).style(Style::new().fg(palette::ui(Ui::Note))), message_area);
                    window = chart_area;
                }
                body_area = window;
                let sel = selected.resolve(&fc); // keeps the User's selection, never creates one
                if let (true, Some(i)) = (selected.follow, sel) {
                    scroll = fc.scroll_to(i, scroll, (window.width, window.height));
                    selected.follow = false;
                }
                scroll = fc.blit(f.buffer_mut(), window, scroll, sel);
                chart = Some(fc);
            })
            .map_err(|e| e.to_string())?;
        if !event::poll(Duration::from_millis(250)).map_err(|e| e.to_string())? {
            continue;
        }
        let mut open = None::<String>;
        let mut unselect = None::<String>; // the Job whose Worker view closes
        match event::read().map_err(|e| e.to_string())? {
            Event::Key(k) if k.kind == KeyEventKind::Press => {
                let dir = match k.code {
                    KeyCode::Down | KeyCode::Char('j') => Some(flow::Dir::Down),
                    KeyCode::Up | KeyCode::Char('k') => Some(flow::Dir::Up),
                    KeyCode::Left | KeyCode::Char('h') => Some(flow::Dir::Left),
                    KeyCode::Right | KeyCode::Char('l') => Some(flow::Dir::Right),
                    _ => None,
                };
                match (k.code, dir, chart.as_ref()) {
                    (KeyCode::Char('q'), ..) => return Ok(()),
                    (_, Some(d), Some(c)) => selected.arrow(c, d),
                    (KeyCode::Enter, ..) => open = selected.enter(),
                    (KeyCode::Esc, ..) => unselect = selected.clear(),
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
                    if body_area.contains(Position::new(m.column, m.row)) {
                        let hit = chart.as_ref().and_then(|c| {
                            c.pill_at(m.column - body_area.x + scroll.0, m.row - body_area.y + scroll.1).map(|i| c.pills[i].key.clone())
                        });
                        match hit {
                            Some(key) if selected.key.as_ref() == Some(&key) => {
                                selected.click(key.clone());
                                unselect = Some(key);
                            }
                            Some(key) => {
                                selected.click(key.clone());
                                open = Some(key);
                            }
                            None => unselect = selected.clear(),
                        }
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
        if let Some(key) = unselect {
            close_worker_view(&key);
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

type PlanRow = (String, Option<u64>, Option<pr::Record>);

/// Rows of the Dispatch sections other than the PRs: homeserver, git and Jira take two each; three separators lie between the four.
const DISPATCH_FIXED_ROWS: u16 = 2 + 2 + 2 + 3 * separator::HEIGHT;

/// The Dispatch pane: homeserver, PRs, git and Jira, each section titled, with the shared separator between each pair.
fn draw_dispatch(f: &mut ratatui::Frame, area: Rect, load: &[&str], rows: &[PlanRow], now: u64) {
    let [home, sep1, prs, sep2, git, sep3, jira] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(separator::HEIGHT),
        Constraint::Min(1),
        Constraint::Length(separator::HEIGHT),
        Constraint::Length(2),
        Constraint::Length(separator::HEIGHT),
        Constraint::Length(2),
    ])
    .areas(area);
    for sep in [sep1, sep2, sep3] {
        separator::draw(f, sep);
    }
    f.render_widget(Paragraph::new(vec![Line::from(" homeserver".bold()), Line::from(format!(" load {}", load.join(" ")))]), home);
    let mut text = vec![Line::from(" PRs".bold())];
    // plans with a PR first; those without only while the pane has room for the rest
    let room = (area.height as usize).saturating_sub(DISPATCH_FIXED_ROWS as usize + text.len());
    let mut used = 0;
    for (label, number, rec) in rows.iter().filter(|r| r.1.is_some()).chain(rows.iter().filter(|r| r.1.is_none())) {
        let lines = pr::render(label, *number, rec.as_ref(), now, area.width as usize);
        if number.is_none() && used + lines.len() > room {
            continue;
        }
        used += lines.len();
        text.extend(lines);
    }
    if rows.is_empty() {
        text.push(Line::from(" no Plans in the cockpit".dim()));
    }
    f.render_widget(Paragraph::new(text), prs);
    f.render_widget(Paragraph::new(vec![Line::from(" git".bold()), Line::from(" (future)".dim())]), git);
    f.render_widget(Paragraph::new(vec![Line::from(" Jira".bold()), Line::from(" (future)".dim())]), jira);
}

fn run_dispatch(terminal: &mut DefaultTerminal) -> Result<(), String> {
    thread::spawn(pr_loop);
    loop {
        let load = fs::read_to_string("/proc/loadavg").unwrap_or_default();
        let load: Vec<&str> = load.split_whitespace().take(3).collect();
        let now = now_secs();
        let rows: Vec<PlanRow> = plans()
            .into_iter()
            .map(|(label, plan)| {
                let number = pr_of(&plan).and_then(|p| p.0);
                (label, number, read_json(&cache_file(&pr_cache_name(&plan))).and_then(|v| pr::Record::from_json(&v)))
            })
            .collect();
        terminal
            .draw(|f| {
                draw_dispatch(f, f.area(), &load, &rows, now);
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

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

    fn text(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect::<String>().trim_end().to_string()
    }

    #[test]
    fn the_statusbar_has_the_legend_button_left_and_the_workers_right() {
        let mut terminal = Terminal::new(TestBackend::new(40, 1)).unwrap();
        let mut hits = Vec::new();
        terminal
            .draw(|f| {
                let (left, right) = statusbar_items(&workers::Workers { running: 1, limit: Some(4) }, "", f.area().width);
                hits = statusbar::draw(f, f.area(), &left, &right);
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        assert_eq!(text(buf, 0), format!("{LEGEND_BUTTON}{}workers 1/4", " ".repeat(40 - LEGEND_BUTTON.len() - "workers 1/4".len())));
        // the reported Rect is where the reversed button is drawn
        assert_eq!(hits, [(Rect::new(0, 0, LEGEND_BUTTON.len() as u16, 1), statusbar::Action::Legend)]);
        assert!((0..LEGEND_BUTTON.len() as u16).all(|x| buf[(x, 0)].modifier.contains(Modifier::REVERSED)));
        assert!(!buf[(LEGEND_BUTTON.len() as u16, 0)].modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn the_context_view_draws_neither_the_legend_button_nor_the_workers() {
        let mut terminal = Terminal::new(TestBackend::new(30, 6)).unwrap();
        let mut state = TreeState::default();
        terminal.draw(|f| context::draw(f, f.area(), &[], &mut state, false)).unwrap();
        let buf = terminal.backend().buffer();
        for y in 0..6 {
            assert!(!text(buf, y).contains("legend") && !text(buf, y).contains("workers"));
        }
    }

    #[test]
    fn the_dispatch_sections_are_titled_with_a_separator_between_each_pair() {
        let mut terminal = Terminal::new(TestBackend::new(30, 14)).unwrap();
        terminal.draw(|f| draw_dispatch(f, f.area(), &["0.1", "0.2", "0.3"], &[], 0)).unwrap();
        let buf = terminal.backend().buffer();
        let rule = "─".repeat(30);
        let lines: Vec<String> = (0..14).map(|y| text(buf, y)).collect();
        assert_eq!(lines[..3], [" homeserver", " load 0.1 0.2 0.3", rule.as_str()]);
        assert_eq!(lines[3], " PRs");
        assert_eq!(lines[4], " no Plans in the cockpit");
        assert_eq!(lines[5..], ["", "", "", rule.as_str(), " git", " (future)", rule.as_str(), " Jira", " (future)"]);
    }
}
