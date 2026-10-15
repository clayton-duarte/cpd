//! The `plan` view (commands.md §13.2, §17): the centre-top pane of the cockpit, ported from the cockpit of branch
//! `cpd/cpd-ui` (`tui/src/flow.rs`, `tabs.rs`, `pr.rs`, `workers.rs`) onto the read model of [`crate::read`].
//!
//! It shows the Plan of the Lead in the chat slot as a left-to-right flow chart of its Jobs ([`flow`]), under the tabs
//! of the Context's Leads ([`tabs`]): the top row is only the Context's id and the tabs, with nothing at the right.
//!
//! Data: one snapshot of the store ([`snapshot`]), then the Events, followed in this process from the snapshot's
//! cursor (one `events` query every `events.poll_ms`, no subprocess); a snapshot is retaken at most every
//! [`COALESCE`] after an Event that concerns the view.
//! The User's keys and clicks run commands ([`actions`]); nothing else starts a process.
//!
//! Keys: ←→↑↓/hjkl select, Enter opens or closes the Worker, Esc clears; the wheel scrolls; clicks select a pill or a
//! tab. There is no `q`: a layout pane that dies stays dead (contract §6.1).

mod actions;
mod flow;
mod tabs;

use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event as Input, KeyCode, KeyEventKind, MouseButton, MouseEventKind,
};
use ratatui::layout::{Constraint, Layout, Margin, Position, Rect};
use ratatui::widgets::Paragraph;

use self::actions::Actions;
use self::flow::{Chart, Dir, Selection};
use crate::palette;
use super::read::{self, ChatSlot, Flow, Place, SessionNode};
use super::{RunCommand, not_a_terminal, on_a_terminal};
use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::{self, Event, highest_id};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, SessionId};
use cpd_core::output::Output;
use cpd_core::store::Store;

/// A snapshot is retaken at most this often, whatever the Event rate (§17).
const COALESCE: Duration = Duration::from_millis(500);

/// What the pane shows when a Lead is in the chat slot.
struct Centre {
    slot: ChatSlot,
    flow: Flow,
    /// The Context's id.
    context: String,
    tabs: Vec<(SessionId, String)>,
    chart: Chart,
}

/// One read of the store: the Plan of the chat slot, if any, and the Event id to follow from.
struct Snapshot {
    cursor: i64,
    centre: Option<Centre>,
}

/// The tab label of a Lead Session: its name, else its Plan's id, else the first 8 characters of its id.
fn tab_label(s: &SessionNode) -> String {
    let name = s.name.trim();
    if !name.is_empty() {
        name.to_owned()
    } else if let Some(plan) = &s.plan {
        plan.to_string()
    } else {
        s.id.as_str().chars().take(8).collect()
    }
}

fn snapshot(store: &mut Store) -> Result<Snapshot, CpdError> {
    // Read first: whatever the later reads see is at least this new, so following from here loses nothing.
    let cursor = store.read(highest_id)?;
    let Some(slot) = read::chat_slot(store)? else { return Ok(Snapshot { cursor, centre: None }) };
    let flow = read::flow(store, &slot.plan)?;
    let tree = read::tree(store)?;
    let context = tree.contexts.into_iter().find(|c| c.id == flow.head.context);
    let tabs = context
        .iter()
        .flat_map(|c| &c.plans)
        .flat_map(|p| &p.sessions)
        .filter(|s| s.place != Place::None) // a tab is a Lead with a pane in the cockpit
        .map(|s| (s.id.clone(), tab_label(s)))
        .collect();
    let context = flow.head.context.to_string();
    let chart = flow::layout(&flow::flow_jobs(&flow));
    Ok(Snapshot { cursor, centre: Some(Centre { slot, flow, context, tabs, chart }) })
}

/// Whether an Event changes what the pane shows: the Plan's or the Context's, or a move of a Lead (the chat slot or the
/// tabs may change).
fn concerns(event: &Event, centre: Option<&Centre>) -> bool {
    let mine = |column: &Option<String>, id: &str| column.as_deref() == Some(id);
    event.kind == "session-moved"
        || centre.is_some_and(|c| mine(&event.plan, c.flow.head.id.as_str()) || mine(&event.context, c.flow.head.context.as_str()))
}

/// The line at the foot of the chart: an error, `no Plan yet`, then the note of the last action.
fn message(error: Option<&str>, note: &str, centre: Option<&Centre>) -> Option<String> {
    let mut parts: Vec<&str> = error.into_iter().collect();
    if centre.is_some_and(|c| c.flow.jobs.is_empty()) {
        parts.push("no Plan yet");
    }
    if !note.is_empty() {
        parts.push(note);
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
        (self.head, self.tabs) = tabs::draw(f, head, centre.map(|c| c.context.as_str()), &tab_list);
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

/// The Attempt a pill opens: the Job's current one.
fn attempt_of(centre: &Centre, key: &str) -> Option<AttemptId> {
    let job = centre.flow.jobs.iter().find(|j| j.key.as_str() == key)?;
    job.current_attempt().map(|a| a.id.clone())
}

pub(super) fn run(run_command: RunCommand) -> Result<Output, CpdError> {
    if !on_a_terminal() {
        return Err(not_a_terminal("tui view plan"));
    }
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    let mut snap = snapshot(&mut store)?;
    let actions = Actions::new(home, &config, run_command);
    let poll = Duration::from_millis(u64::from(config.events.poll_ms));

    // From here on this process owns the terminal (commands.md §1.2).
    let mut terminal = ratatui::init();
    let mouse = ratatui::crossterm::execute!(std::io::stdout(), EnableMouseCapture);
    let result = mouse.map_err(|e| e.to_string()).and_then(|()| {
        show(&mut terminal, &mut store, &mut snap, poll, &actions).map_err(|e| e.to_string())
    });
    let _ = ratatui::crossterm::execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    match result {
        Ok(()) => Ok(Output::Owned),
        Err(error) => Err(CpdError::new(ErrorCode::NotATerminal, format!("The terminal of cpd tui view plan failed: {error}."))
            .with("command", "tui view plan")),
    }
}

/// Draws, follows Events and takes input until the terminal fails.
fn show(
    terminal: &mut ratatui::DefaultTerminal,
    store: &mut Store,
    snap: &mut Snapshot,
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
                match snapshot(store) {
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
        if let Some(key) = unselect {
            if let Err(e) = actions.close_worker(&key) {
                ui.note = e.message().to_owned();
            }
        }
        if let Some(key) = open {
            ui.note = match attempt_of(centre, &key) {
                None => format!("{key} has not started yet"),
                Some(attempt) => match actions.toggle_worker(&centre.flow.head.id, &key, &attempt) {
                    Ok(_) => String::new(),
                    Err(e) => e.message().to_owned(),
                },
            };
        }
    }
}
