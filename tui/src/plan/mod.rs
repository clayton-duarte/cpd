//! The `plan` view (commands.md §13.2, §17): the centre-top pane of the cockpit, ported from the cockpit of branch
//! `cpd/cpd-ui` (`tui/src/flow.rs`, `pr.rs`, `workers.rs`) onto the read model of [`crate::read`].
//!
//! It shows the Plan of the Lead in the chat slot as one box per PR ([`boxes`], R34 A15), top to bottom by the PRs'
//! needs, each holding the left-to-right flow chart of its Jobs ([`flow`]). The boxes start at the top of the pane:
//! there is no top row and there are no tabs (switching Leads is the tree's).
//!
//! Data: one snapshot of the store ([`snapshot`]), then the Events, followed in this process from the snapshot's
//! cursor (one `events` query every `events.poll_ms`, no subprocess); a snapshot is retaken at most every
//! [`COALESCE`] after an Event that concerns the view.
//! The User's keys and clicks run commands ([`actions`]); nothing else starts a process.
//!
//! Keys: ←→↑↓/hjkl select (←→ within a row of boxes, ↑↓ across rows too), Enter opens or closes the Worker, Esc clears; the wheel scrolls; clicks select a pill. There is no `q`: a layout pane that
//! dies stays dead (contract §6.1).

mod actions;
mod boxes;
mod flow;

use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};

use ratatui::Frame;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event as Input, KeyCode, KeyEventKind, MouseButton, MouseEventKind,
};
use ratatui::layout::{Constraint, Layout, Margin, Position, Rect};
use ratatui::style::Stylize;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use self::actions::Actions;
use self::boxes::Boxes;
use self::flow::{Dir, Selection};
use crate::palette;
use super::read::{self, ChatSlot, PlanBoxes};
use super::{RunCommand, not_a_terminal, on_a_terminal};
use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::{self, Event, highest_id};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::output::Output;
use cpd_core::store::Store;

/// A snapshot is retaken at most this often, whatever the Event rate (§17).
const COALESCE: Duration = Duration::from_millis(500);

/// The view redraws at least this often, for the spinner (one frame: palette.rs); a redraw reads nothing from the store.
const FRAME: Duration = Duration::from_millis(250);

/// The blank space around the chart's content, inside the canvas the view scrolls over: at scroll (0, 0) nothing of the
/// chart touches the pane's edges, and scrolled the chart reaches them.
const MARGIN: Margin = Margin { horizontal: 2, vertical: 1 };

/// What the pane shows when a Lead is in the chat slot.
struct Centre {
    slot: ChatSlot,
    plan: PlanBoxes,
    boxes: Boxes,
    /// The spinner frame `boxes` is drawn at.
    frame: usize,
}

impl Centre {
    /// Draw the boxes again at spinner frame `frame`, from the Plan already read.
    fn turn(&mut self, frame: usize) {
        if frame != self.frame {
            self.boxes = boxes::layout(&self.plan.prs, &self.plan.jobs, frame).with_margin(MARGIN);
            self.frame = frame;
        }
    }
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
    let frame = palette::frame(SystemTime::now());
    let boxes = boxes::layout(&plan.prs, &plan.jobs, frame).with_margin(MARGIN);
    Ok(Snapshot { cursor, centre: Some(Centre { slot, plan, boxes, frame }) })
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
    /// The scroll offset each Plan was left at, kept while the view runs only (hs-d197). Not clamped: the chart may
    /// have changed size by the time it comes back, and a draw clamps.
    remembered: HashMap<PlanId, (u16, u16)>,
    note: String,
    error: Option<String>,
    body: Rect,
}

impl Ui {
    /// A different Lead is in the slot, `from` its Plan before and `to` its Plan now: nothing is selected, the chart
    /// is at the offset `to` was left at, or at its corner. Leads of one Plan share the Plan's offset.
    fn switch(&mut self, from: Option<&PlanId>, to: Option<&PlanId>) {
        if let Some(from) = from {
            self.remembered.insert(from.clone(), self.scroll);
        }
        self.selected.reset();
        self.scroll = to.and_then(|plan| self.remembered.get(plan).copied()).unwrap_or((0, 0));
        self.note.clear();
    }

    fn draw(&mut self, f: &mut Frame, centre: Option<&Centre>) {
        let body = f.area();
        let mut window = body; // the chart's own margin keeps it off the pane's edges at scroll (0, 0)
        if let Some(text) = message(self.error.as_deref(), &self.note, centre) {
            let [chart_area, message_area] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(window);
            // the pane's last row, indented like the chart's content at scroll (0, 0)
            f.render_widget(Paragraph::new(text), message_area.inner(Margin { horizontal: MARGIN.horizontal, vertical: 0 }));
            window = chart_area;
        }
        self.body = window;
        let Some(c) = centre else { return };
        let sel = self.selected.resolve(&c.boxes.chart); // keeps the User's selection, never creates one
        if let (true, Some(i)) = (self.selected.follow, sel) {
            self.scroll = c.boxes.chart.scroll_to(i, self.scroll, (window.width, window.height));
            self.selected.follow = false;
        }
        self.scroll = c.boxes.chart.blit(f.buffer_mut(), window, self.scroll, sel);
    }
}

/// An arrow key: as [`Selection::arrow`], each step taken across the boxes ([`Boxes::step`]).
fn arrow(selected: &mut Selection, boxes: &Boxes, dir: Dir) {
    match selected.resolve(&boxes.chart) {
        Some(i) => {
            selected.key = Some(boxes.chart.pills[boxes.step(i, dir)].key.clone());
            selected.follow = true;
        }
        None => selected.arrow(&boxes.chart, dir),
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
    let (mut stale, mut taken, mut fetched) = (false, Instant::now(), Instant::now());
    loop {
        if let Some(c) = snap.centre.as_mut() {
            c.turn(palette::frame(SystemTime::now()));
        }
        terminal.draw(|f| ui.draw(f, snap.centre.as_ref()))?;
        if !event::poll(poll.min(FRAME))? {
            // idle: look for Events, every `poll`
            if fetched.elapsed() < poll {
                continue;
            }
            fetched = Instant::now();
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
                let before = snap.centre.as_ref().map(|c| (c.slot.session.clone(), c.plan.plan.clone()));
                match snapshot(store) {
                    Ok(next) => {
                        *snap = next;
                        ui.error = None;
                        let after = snap.centre.as_ref().map(|c| (c.slot.session.clone(), c.plan.plan.clone()));
                        if before.as_ref().map(|b| &b.0) != after.as_ref().map(|a| &a.0) {
                            ui.switch(before.as_ref().map(|b| &b.1), after.as_ref().map(|a| &a.1));
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
                let boxes = snap.centre.as_ref().map(|c| &c.boxes);
                match (k.code, dir, boxes) {
                    (_, Some(d), Some(b)) => arrow(&mut ui.selected, b, d),
                    (KeyCode::Enter, ..) => open = ui.selected.enter(),
                    (KeyCode::Esc, ..) => unselect = ui.selected.clear(),
                    _ => {}
                }
            }
            Input::Mouse(m) => match m.kind {
                MouseEventKind::Down(MouseButton::Left) if ui.body.contains(Position::new(m.column, m.row)) => {
                    let hit = snap.centre.as_ref().and_then(|c| {
                        let (x, y) = (m.column - ui.body.x + ui.scroll.0, m.row - ui.body.y + ui.scroll.1);
                        c.boxes.chart.pill_at(x, y).map(|i| c.boxes.chart.pills[i].key.clone())
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

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;
    use crate::read::{JobNode, JobStatus, PrNode, PrStatus};
    use cpd_core::config::Tier;
    use cpd_core::ids::{ContextId, JobKey, PrKey, SessionId};

    const PANE: (u16, u16) = (40, 10);

    fn plan(id: &str) -> PlanId {
        PlanId::parse(id).unwrap()
    }

    /// A Plan of one PR with `jobs` independent Jobs: a chart `jobs` rows tall (and some more for the box).
    fn centre(plan_id: &str, jobs: usize) -> Centre {
        let pr = PrKey::parse("web").unwrap();
        let node = PrNode {
            key: pr.clone(),
            repo_name: "repo".into(),
            base: "main".into(),
            status: PrStatus::Dispatch,
            needs: Vec::new(),
            pull: None,
        };
        let job = |n: usize| JobNode {
            key: JobKey::parse(&format!("job-{n}")).unwrap(),
            pr: pr.clone(),
            seq: 0,
            title: String::new(),
            status: JobStatus::Queued,
            tier: Tier::parse("S1").unwrap(),
            needs: Vec::new(),
            appended: false,
            retry_tier: None,
            failed_cause: None,
            cancel_reason: None,
            merged_sha: None,
            attempts: Vec::new(),
        };
        let plan = PlanBoxes {
            plan: PlanId::parse(plan_id).unwrap(),
            context: ContextId::parse("ctx").unwrap(),
            prs: vec![node],
            jobs: (0..jobs).map(job).collect(),
            events_cursor: 0,
        };
        let boxes = boxes::layout(&plan.prs, &plan.jobs, 0).with_margin(MARGIN);
        let slot = ChatSlot {
            session: SessionId::parse("00000000-0000-4000-8000-000000000000").unwrap(),
            name: "lead".into(),
            plan: plan.plan.clone(),
            moved_event: 0,
        };
        Centre { slot, plan, boxes, frame: 0 }
    }

    fn draw(ui: &mut Ui, centre: &Centre) {
        let mut terminal = Terminal::new(TestBackend::new(PANE.0, PANE.1)).unwrap();
        terminal.draw(|f| ui.draw(f, Some(centre))).unwrap();
    }

    #[test]
    fn switching_plans_and_back_restores_the_offset_with_no_selection() {
        let (a, b) = (plan("plan-a"), plan("plan-b"));
        let mut ui = Ui::default();
        draw(&mut ui, &centre("plan-a", 30));
        ui.scroll = (0, 7);
        ui.selected.key = Some("job-3".into());

        ui.switch(Some(&a), Some(&b));
        assert_eq!((ui.scroll, &ui.selected), ((0, 0), &Selection::default()), "B starts at its corner");
        ui.scroll = (0, 2);

        ui.switch(Some(&b), Some(&a));
        assert_eq!((ui.scroll, &ui.selected), ((0, 7), &Selection::default()));
        ui.switch(Some(&a), Some(&b));
        assert_eq!(ui.scroll, (0, 2), "each Plan has its own");
    }

    #[test]
    fn two_leads_of_one_plan_share_its_offset() {
        let a = plan("plan-a");
        let mut ui = Ui::default();
        ui.scroll = (4, 9);
        ui.switch(Some(&a), Some(&a));
        assert_eq!(ui.scroll, (4, 9));
    }

    #[test]
    fn no_lead_and_back_is_the_same_plan() {
        let a = plan("plan-a");
        let mut ui = Ui::default();
        ui.scroll = (0, 5);
        ui.switch(Some(&a), None);
        assert_eq!(ui.scroll, (0, 0));
        ui.switch(None, Some(&a));
        assert_eq!(ui.scroll, (0, 5));
    }

    #[test]
    fn a_chart_that_shrank_clamps_the_restored_offset_when_drawn() {
        let (a, b) = (plan("plan-a"), plan("plan-b"));
        let mut ui = Ui::default();
        ui.scroll = (0, 20);
        ui.switch(Some(&a), Some(&b));
        ui.switch(Some(&b), Some(&a));
        assert_eq!(ui.scroll, (0, 20), "restored as it was left");

        let small = centre("plan-a", 12);
        draw(&mut ui, &small);
        let height = small.boxes.chart.pills.iter().map(|p| p.rect.bottom()).max().unwrap();
        assert!(ui.scroll.1 < 20 && ui.scroll.1 + PANE.1 >= height, "{:?}, chart {height} rows", ui.scroll);
    }

    #[test]
    fn a_view_that_restarts_starts_at_the_top() {
        assert_eq!(Ui::default().scroll, (0, 0));
        assert!(Ui::default().remembered.is_empty());
    }
}
