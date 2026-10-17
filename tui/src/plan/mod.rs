//! The `plan` view (commands.md §13.2, §17): the centre-top pane of the cockpit, ported from the cockpit of branch
//! `cpd/cpd-ui` (`tui/src/flow.rs`, `pr.rs`, `workers.rs`) onto the read model of [`crate::read`].
//!
//! It shows the Plan of the Lead in the chat slot as one box per PR ([`boxes`], R34 A15), left to right by the PRs'
//! needs, each holding the left-to-right flow chart of its Jobs ([`flow`]). The boxes start at the top of the pane:
//! there is no top row and there are no tabs (switching Leads is the tree's).
//!
//! Data: one snapshot of the store ([`snapshot`]), then the Events, followed in this process from the snapshot's
//! cursor (one `events` query every `events.poll_ms`, no subprocess); a snapshot is retaken at most every
//! [`COALESCE`] after an Event that concerns the view.
//! The User's keys and clicks run commands ([`actions`]); nothing else starts a process.
//!
//! Keys: ←→↑↓/hjkl select, Enter opens or closes the Worker, Esc clears; the wheel scrolls; clicks select a pill. There is no `q`: a layout pane that
//! dies stays dead (contract §6.1).

mod actions;
mod boxes;
mod flow;

use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event as Input, KeyCode, KeyEventKind, MouseButton, MouseEventKind,
};
use ratatui::layout::{Constraint, Layout, Margin, Position, Rect};
use ratatui::style::Stylize;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use self::actions::Actions;
use self::flow::{Chart, Dir, Selection};
use crate::palette;
use super::read::{self, ChatSlot, PlanBoxes};
use super::{RunCommand, not_a_terminal, on_a_terminal};
use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::{self, Event, highest_id};
use cpd_core::home::CpdHome;
use cpd_core::ids::AttemptId;
use cpd_core::output::Output;
use cpd_core::store::Store;

/// A snapshot is retaken at most this often, whatever the Event rate (§17).
const COALESCE: Duration = Duration::from_millis(500);

/// What the pane shows when a Lead is in the chat slot.
struct Centre {
    slot: ChatSlot,
    plan: PlanBoxes,
    chart: Chart,
}

/// One read of the store: the Plan of the chat slot, if any, and the Event id to follow from.
struct Snapshot {
    cursor: i64,
    centre: Option<Centre>,
}

fn snapshot(store: &mut Store) -> Result<Snapshot, CpdError> {
    // Read first: whatever the later reads see is at least this new, so following from here loses nothing.
    let cursor = store.read(highest_id)?;
    let Some(slot) = read::chat_slot(store)? else { return Ok(Snapshot { cursor, centre: None }) };
    let plan = read::plan_boxes(store, &slot.plan)?;
    let chart = boxes::layout(&plan.prs, &plan.jobs);
    Ok(Snapshot { cursor, centre: Some(Centre { slot, plan, chart }) })
}

/// Whether an Event changes what the pane shows: the Plan's or the Context's, or a move of a Lead (the chat slot may
/// change).
fn concerns(event: &Event, centre: Option<&Centre>) -> bool {
    let mine = |column: &Option<String>, id: &str| column.as_deref() == Some(id);
    event.kind == "session-moved"
        || centre.is_some_and(|c| mine(&event.plan, c.plan.plan.as_str()) || mine(&event.context, c.plan.context.as_str()))
}

/// The line at the foot of the chart: an error, ` no Lead session in the centre` (dim), `no PR yet: cpd pr add` (dim)
/// or `no Plan yet`, then the note of the last action.
fn message(error: Option<&str>, note: &str, centre: Option<&Centre>) -> Option<Line<'static>> {
    let mut parts: Vec<Span<'static>> = error.into_iter().map(|e| Span::styled(e.to_owned(), palette::note())).collect();
    match centre {
        None => parts.push(" no Lead session in the centre".dim()),
        Some(c) if c.plan.prs.is_empty() => parts.push("no PR yet: cpd pr add".dim()),
        Some(c) if c.plan.jobs.is_empty() => parts.push(Span::styled("no Plan yet", palette::note())),
        Some(_) => {}
    }
    if !note.is_empty() {
        parts.push(Span::styled(note.to_owned(), palette::note()));
    }
    (!parts.is_empty()).then(|| Line::from(spaced(parts)))
}

/// `parts` with two spaces between each pair.
fn spaced(parts: Vec<Span<'static>>) -> Vec<Span<'static>> {
    let mut out = Vec::with_capacity(parts.len() * 2);
    for (i, part) in parts.into_iter().enumerate() {
        if i > 0 {
            out.push("  ".into());
        }
        out.push(part);
    }
    out
}

/// The User's state in the view, and where the last draw put things (for clicks).
#[derive(Default)]
struct Ui {
    selected: Selection,
    /// The window's top-left in the chart.
    scroll: (u16, u16),
    note: String,
    error: Option<String>,
    body: Rect,
}

impl Ui {
    /// A different Lead is in the slot: nothing is selected, the chart starts at its corner.
    fn reset(&mut self) {
        self.selected.reset();
        self.scroll = (0, 0);
        self.note.clear();
    }

    fn draw(&mut self, f: &mut Frame, centre: Option<&Centre>) {
        let body = f.area();
        // two columns and one row of padding: nothing of the chart touches the pane's edges
        let mut window = body.inner(Margin { horizontal: 2, vertical: 1 });
        if let Some(text) = message(self.error.as_deref(), &self.note, centre) {
            let [chart_area, message_area] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(window);
            f.render_widget(Paragraph::new(text), message_area);
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
    let job = centre.plan.jobs.iter().find(|j| j.key.as_str() == key)?;
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
                Some(attempt) => match actions.toggle_worker(&centre.plan.plan, &key, &attempt) {
                    Ok(_) => String::new(),
                    Err(e) => e.message().to_owned(),
                },
            };
        }
    }
}
