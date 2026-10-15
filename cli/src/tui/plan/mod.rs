//! The `plan` view (commands.md §13.2, §17): the centre-top pane of the cockpit, ported from the cockpit of branch
//! `cpd/cpd-ui` (`tui/src/flow.rs`, `tabs.rs`, `pr.rs`, `workers.rs`) onto the read model of [`crate::tui::read`].
//!
//! It shows the Plan of the Lead in the chat slot as a left-to-right flow chart of its Jobs ([`flow`]), under the tabs
//! of the Context's Leads ([`tabs`]), with the Plan's PR ([`pr`]) and the Workers ([`workers`]) at the right of the
//! tabs, and, at the foot, the open Escalations and the Plan review.
//!
//! Data: one snapshot of the store ([`snapshot`]), then the Events, followed in this process from the snapshot's
//! cursor (one `events` query every `events.poll_ms`, no subprocess); a snapshot is retaken at most every
//! [`COALESCE`] after an Event that concerns the view. The PR is what `pr poll` stored: nothing here runs `gh`.
//! The User's keys and clicks run commands ([`actions`]); nothing else starts a process.

mod actions;
mod flow;
mod pr;
mod tabs;
mod workers;

use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event as Input, KeyCode, KeyEventKind, MouseButton, MouseEventKind,
};
use ratatui::layout::{Constraint, Layout, Margin, Position, Rect};
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use self::actions::Actions;
use self::flow::{Chart, Dir, Selection};
use crate::tui::palette;
use self::workers::Workers;
use super::read::{self, ChatSlot, Flow, ReviewStatus};
use super::{not_a_terminal, on_a_terminal};
use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::events::{self, Event, highest_id};
use crate::home::CpdHome;
use crate::ids::{AttemptId, SessionId};
use crate::output::Output;
use crate::plan::Phase;
use crate::store::Store;

/// A snapshot is retaken at most this often, whatever the Event rate (§17).
const COALESCE: Duration = Duration::from_millis(500);

/// What the pane shows when a Lead is in the chat slot.
struct Centre {
    slot: ChatSlot,
    flow: Flow,
    context: String,
    tabs: Vec<(SessionId, String)>,
    workers: Workers,
    chart: Chart,
}

/// One read of the store: the Plan of the chat slot, if any, and the Event id to follow from.
struct Snapshot {
    cursor: i64,
    centre: Option<Centre>,
}

/// The tab label of a Lead: its Plan, and for a side fork also its name.
fn tab_label(plan: &str, name: &str, plan_lead: bool) -> String {
    if plan_lead { plan.to_owned() } else { format!("{plan}/{name}") }
}

fn snapshot(store: &mut Store, concurrency: u32) -> Result<Snapshot, CpdError> {
    // Read first: whatever the later reads see is at least this new, so following from here loses nothing.
    let cursor = store.read(highest_id)?;
    let Some(slot) = read::chat_slot(store)? else { return Ok(Snapshot { cursor, centre: None }) };
    let flow = read::flow(store, &slot.plan)?;
    let tree = read::tree(store)?;
    let context = tree.contexts.into_iter().find(|c| c.id == flow.head.context);
    let tabs = context
        .iter()
        .flat_map(|c| &c.plans)
        .flat_map(|p| p.sessions.iter().map(move |s| (s.id.clone(), tab_label(p.head.id.as_str(), &s.name, s.plan_lead))))
        .collect();
    let context = context.map_or_else(|| flow.head.context.to_string(), |c| c.title);
    let workers = Workers::of(&read::widgets(store, concurrency, 0)?.slots);
    let chart = flow::layout(&flow::flow_jobs(&flow));
    Ok(Snapshot { cursor, centre: Some(Centre { slot, flow, context, tabs, workers, chart }) })
}

/// Whether an Event changes what the pane shows: the Plan's or the Context's, a move of a Lead (the chat slot or the
/// tabs may change), or an Attempt of any Plan (the Workers hold slots machine-wide).
fn concerns(event: &Event, centre: Option<&Centre>) -> bool {
    let mine = |column: &Option<String>, id: &str| column.as_deref() == Some(id);
    event.kind == "session-moved"
        || event.kind.starts_with("attempt-")
        || centre.is_some_and(|c| mine(&event.plan, c.flow.head.id.as_str()) || mine(&event.context, c.flow.head.context.as_str()))
}

/// The line at the foot of the chart: an error, the note of the last action, then what needs the User.
fn message(error: Option<&str>, note: &str, centre: Option<&Centre>) -> Option<String> {
    let mut parts: Vec<String> = error.map(str::to_owned).into_iter().collect();
    if !note.is_empty() {
        parts.push(note.to_owned());
    }
    if let Some(c) = centre {
        let open = c.flow.escalations.len();
        if open > 0 {
            parts.push(format!("{open} open Escalation{}", if open == 1 { "" } else { "s" }));
        }
        if let Some(r) = c.flow.review.as_ref().filter(|r| r.status == ReviewStatus::Pending || c.flow.head.phase == Phase::Plan) {
            let status = match (r.status, r.current) {
                (ReviewStatus::Pending, _) => "pending",
                (ReviewStatus::Approved, true) => "approved",
                (ReviewStatus::Approved, false) => "approved, but the Jobs changed since",
                (ReviewStatus::Rejected, _) => "rejected",
            };
            parts.push(format!("Plan review r{} {status}", r.number));
        }
        if c.flow.jobs.is_empty() {
            parts.push("no Jobs yet".to_owned());
        }
    }
    (!parts.is_empty()).then(|| parts.join("  "))
}

/// The User's state in the view, and where the last draw put things (for clicks).
#[derive(Default)]
struct Ui {
    selected: Selection,
    /// The window's top-left in the chart.
    scroll: (u16, u16),
    note: String,
    error: Option<String>,
    head: Rect,
    body: Rect,
    tabs: tabs::Hits,
}

impl Ui {
    /// A different Lead is in the slot: nothing is selected, the chart starts at its corner.
    fn reset(&mut self) {
        self.selected.reset();
        self.scroll = (0, 0);
        self.note.clear();
    }

    fn draw(&mut self, f: &mut Frame, centre: Option<&Centre>) {
        let [head, body] = Layout::vertical([Constraint::Length(tabs::HEIGHT), Constraint::Min(1)]).areas(f.area());
        let tab_list: Vec<tabs::Tab> = centre
            .iter()
            .flat_map(|c| &c.tabs)
            .map(|(id, label)| tabs::Tab {
                id: id.clone(),
                label: label.clone(),
                active: centre.is_some_and(|c| &c.slot.session == id),
            })
            .collect();
        let right = centre.map(right_parts).unwrap_or_default();
        (self.head, self.tabs) = tabs::draw(f, head, centre.map(|c| c.context.as_str()), &tab_list, &right);
        // two columns and one row of padding: nothing of the chart touches the pane's edges
        let mut window = body.inner(Margin { horizontal: 2, vertical: 1 });
        if let Some(text) = message(self.error.as_deref(), &self.note, centre) {
            let [chart_area, message_area] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(window);
            f.render_widget(Paragraph::new(text).style(palette::note()), message_area);
            window = chart_area;
        }
        self.body = window;
        let Some(c) = centre else { return };
        let sel = self.selected.resolve(&c.chart); // keeps the User's selection, never creates one
        if let (true, Some(i)) = (self.selected.follow, sel) {
            self.scroll = c.chart.scroll_to(i, self.scroll, (window.width, window.height));
            self.selected.follow = false;
        }
        self.scroll = c.chart.blit(f.buffer_mut(), window, self.scroll, sel);
    }
}

/// What may go at the right of the tabs, most wanted first: the PR with the Workers (long, then short), the Workers,
/// the PR. The tabs row draws the first that fits.
fn right_parts(c: &Centre) -> Vec<Line<'static>> {
    let pr = pr::line(c.flow.head.pr.as_ref());
    let workers = c.workers.forms();
    let beside = |w: &Line<'static>| {
        let mut spans = pr.spans.clone();
        spans.push("  ".into());
        spans.extend(w.spans.clone());
        Line::from(spans)
    };
    let mut parts: Vec<Line<'static>> = workers.iter().map(beside).collect();
    parts.extend(workers);
    parts.push(pr);
    parts
}

/// The Attempt a pill opens: the Job's current one.
fn attempt_of(centre: &Centre, key: &str) -> Option<AttemptId> {
    let job = centre.flow.jobs.iter().find(|j| j.key.as_str() == key)?;
    job.current_attempt().map(|a| a.id.clone())
}

pub(super) fn run() -> Result<Output, CpdError> {
    if !on_a_terminal() {
        return Err(not_a_terminal("tui view plan"));
    }
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    let mut snap = snapshot(&mut store, config.concurrency)?;
    let actions = Actions::new(home, &config);
    let poll = Duration::from_millis(u64::from(config.events.poll_ms));

    // From here on this process owns the terminal (commands.md §1.2).
    let mut terminal = ratatui::init();
    let mouse = ratatui::crossterm::execute!(std::io::stdout(), EnableMouseCapture);
    let result = mouse.map_err(|e| e.to_string()).and_then(|()| {
        show(&mut terminal, &mut store, &mut snap, config.concurrency, poll, &actions).map_err(|e| e.to_string())
    });
    let _ = ratatui::crossterm::execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    match result {
        Ok(()) => Ok(Output::Owned),
        Err(error) => Err(CpdError::new(ErrorCode::NotATerminal, format!("The terminal of cpd2 tui view plan failed: {error}."))
            .with("command", "tui view plan")),
    }
}

/// Draws, follows Events and takes input until `q` or the terminal fails.
fn show(
    terminal: &mut ratatui::DefaultTerminal,
    store: &mut Store,
    snap: &mut Snapshot,
    concurrency: u32,
    poll: Duration,
    actions: &Actions,
) -> std::io::Result<()> {
    let mut ui = Ui::default();
    let follow = store.read(|conn| events::Selection::resolve(conn, None, None, None, Vec::new()));
    let follow = match follow {
        Ok(f) => f,
        Err(e) => return Err(std::io::Error::other(e.to_string())),
    };
    let (mut stale, mut taken) = (false, Instant::now());
    loop {
        terminal.draw(|f| ui.draw(f, snap.centre.as_ref()))?;
        if !event::poll(poll)? {
            // idle: look for Events
            match store.read(|conn| follow.fetch(conn, snap.cursor, None, 500)) {
                Ok(events) => {
                    ui.error = None;
                    if let Some(last) = events.last() {
                        snap.cursor = last.id;
                    }
                    stale |= events.iter().any(|e| concerns(e, snap.centre.as_ref()));
                }
                Err(e) => ui.error = Some(e.to_string()),
            }
            if stale && taken.elapsed() >= COALESCE {
                let before = snap.centre.as_ref().map(|c| c.slot.session.clone());
                match snapshot(store, concurrency) {
                    Ok(next) => {
                        *snap = next;
                        ui.error = None;
                        if before != snap.centre.as_ref().map(|c| c.slot.session.clone()) {
                            ui.reset();
                        }
                    }
                    Err(e) => ui.error = Some(e.to_string()),
                }
                (stale, taken) = (false, Instant::now());
            }
            continue;
        }
        let mut open = None::<String>;
        let mut unselect = None::<String>; // the Job whose Worker pane closes
        match event::read()? {
            Input::Key(k) if k.kind == KeyEventKind::Press => {
                let dir = match k.code {
                    KeyCode::Down | KeyCode::Char('j') => Some(Dir::Down),
                    KeyCode::Up | KeyCode::Char('k') => Some(Dir::Up),
                    KeyCode::Left | KeyCode::Char('h') => Some(Dir::Left),
                    KeyCode::Right | KeyCode::Char('l') => Some(Dir::Right),
                    _ => None,
                };
                let chart = snap.centre.as_ref().map(|c| &c.chart);
                match (k.code, dir, chart) {
                    (KeyCode::Char('q'), ..) => return Ok(()),
                    (_, Some(d), Some(c)) => ui.selected.arrow(c, d),
                    (KeyCode::Enter, ..) => open = ui.selected.enter(),
                    (KeyCode::Esc, ..) => unselect = ui.selected.clear(),
                    _ => {}
                }
            }
            Input::Mouse(m) => match m.kind {
                MouseEventKind::Down(MouseButton::Left) if m.row == ui.head.y => {
                    if let Some((_, _, id)) = ui.tabs.iter().find(|(a, b, _)| m.column >= *a && m.column < *b) {
                        if let Err(e) = actions.open_lead(id) {
                            ui.note = e.message().to_owned();
                        }
                        stale = true; // the move is an Event, but take the snapshot at once
                        taken = Instant::now() - COALESCE;
                    }
                }
                MouseEventKind::Down(MouseButton::Left) if ui.body.contains(Position::new(m.column, m.row)) => {
                    let hit = snap.centre.as_ref().and_then(|c| {
                        let (x, y) = (m.column - ui.body.x + ui.scroll.0, m.row - ui.body.y + ui.scroll.1);
                        c.chart.pill_at(x, y).map(|i| c.chart.pills[i].key.clone())
                    });
                    match hit {
                        Some(key) if ui.selected.key.as_ref() == Some(&key) => {
                            ui.selected.click(key.clone());
                            unselect = Some(key);
                        }
                        Some(key) => {
                            ui.selected.click(key.clone());
                            open = Some(key);
                        }
                        None => unselect = ui.selected.clear(),
                    }
                }
                MouseEventKind::ScrollUp => ui.scroll.1 = ui.scroll.1.saturating_sub(3),
                MouseEventKind::ScrollDown => ui.scroll.1 = ui.scroll.1.saturating_add(3), // clamped when drawn
                MouseEventKind::ScrollLeft => ui.scroll.0 = ui.scroll.0.saturating_sub(4),
                MouseEventKind::ScrollRight => ui.scroll.0 = ui.scroll.0.saturating_add(4),
                _ => {}
            },
            _ => {}
        }
        let Some(centre) = snap.centre.as_ref() else { continue };
        if let Some(attempt) = unselect.and_then(|key| attempt_of(centre, &key)) {
            if let Err(e) = actions.close_worker(&attempt) {
                ui.note = e.message().to_owned();
            }
        }
        if let Some(key) = open {
            ui.note = match attempt_of(centre, &key) {
                None => format!("{key} has not started yet"),
                Some(attempt) => match actions.toggle_worker(&centre.flow.head.id, &attempt) {
                    Ok(_) => String::new(),
                    Err(e) => e.message().to_owned(),
                },
            };
        }
    }
}
