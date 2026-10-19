//! The `plan` view (commands.md §13.2, §17): the centre-top pane of the cockpit, ported from the cockpit of branch
//! `cpd/cpd-ui` (`tui/src/flow.rs`, `pr.rs`, `workers.rs`) onto the read model of [`crate::read`].
//!
//! It shows the Plan of the Lead in the chat slot as one box per PR ([`boxes`], R34 A15), top to bottom by the PRs'
//! needs, each holding the left-to-right flow chart of its Jobs ([`flow`]). The boxes start at the top of the pane:
//! there is no top row and there are no tabs (switching Leads is the tree's). Only while the Plan's latest Plan
//! review is pending is there one row above the boxes ([`ReviewRow`]).
//!
//! Data: one snapshot of the store ([`snapshot`]), then the Events, followed in this process from the snapshot's
//! cursor (one `events` query every `events.poll_ms`, no subprocess); a snapshot is retaken at most every
//! [`COALESCE`] after an Event that concerns the view.
//! The User's keys and clicks run commands ([`actions`]); nothing else starts a process.
//!
//! Keys: ←→↑↓/hjkl select (←→ within a row of boxes, ↑↓ across rows too), Enter opens or closes the Worker, Esc clears; the wheel scrolls; clicks select a pill. There is no `q`: a view pane that
//! dies is respawned by the `pane-died` hook (commands.md §10.9).

mod actions;
mod boxes;
mod flow;
pub(crate) mod pulse;

use std::collections::HashMap;
use std::fs;
use std::time::{Duration, Instant, SystemTime};

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event as Input, KeyCode, KeyEventKind, MouseButton, MouseEventKind,
};
use ratatui::layout::{Constraint, Layout, Margin, Position, Rect};
use ratatui::style::Stylize;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use self::actions::Actions;
use self::boxes::Boxes;
use self::flow::{Dir, Selection};
use crate::link::{CUE, link};
use crate::palette::{self, Meaning};
use super::read::{self, ChatSlot, PlanBoxes, Pulse, ReviewStatus};
use super::{RunCommand, not_a_terminal, on_a_terminal};
use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::{self, Event, highest_id};
use cpd_core::home::CpdHome;
use cpd_core::host::addr::review_host;
use cpd_core::ids::{AttemptId, PlanId, SessionId};
use cpd_core::output::Output;
use cpd_core::store::Store;

/// A snapshot is retaken at most this often, whatever the Event rate (§17).
const COALESCE: Duration = Duration::from_millis(500);

/// The snapshot is retaken at least this often, Event or not: a conflict clears with no Event (`mergeable` changes
/// silently), as the `widgets` view's `REFRESH` (§17). Tests shorten it with `CPD_TEST_VIEW_REFRESH` ([`super::reread`]).
const REFRESH: Duration = Duration::from_secs(2);

/// Whether the snapshot taken `since` ago is retaken: after `coalesce` ([`COALESCE`]) once an Event concerns it
/// (`stale`), else after `refresh` ([`REFRESH`]; both from [`super::reread`]).
fn due(stale: bool, since: Duration, (coalesce, refresh): (Duration, Duration)) -> bool {
    since >= if stale { coalesce } else { refresh }
}

/// While Pulse shows the snapshot is retaken at least this often, Event or not: the window moves and the ages tick,
/// and a running Attempt is not an Event.
const PULSE_REFRESH: Duration = Duration::from_secs(60);

/// Whether the Pulse snapshot taken `since` ago is retaken: after `coalesce` once an Event concerns it (or the pane
/// was resized), else after [`PULSE_REFRESH`].
fn pulse_due(stale: bool, since: Duration, coalesce: Duration) -> bool {
    since >= if stale { coalesce } else { PULSE_REFRESH }
}

/// The view redraws at least this often, for the spinner (one frame: palette.rs); a redraw reads nothing from the store.
const FRAME: Duration = Duration::from_millis(100);

/// The blank space around the chart's content, inside the canvas the view scrolls over: at scroll (0, 0) nothing of the
/// chart touches the pane's edges, and scrolled the chart reaches them.
const MARGIN: Margin = Margin { horizontal: 2, vertical: 1 };

/// The row above the boxes while the Plan's latest Plan review is pending (commands.md §13.2): ` Plan review r<n> pending 󰏌`.
#[derive(Debug, PartialEq, Eq)]
struct ReviewRow {
    number: u32,
    /// `http://<host>:<port>`, the port of `run/<plan>/plannotator-port`; `None` when that file is missing or holds
    /// no port: the row shows without the `󰏌`.
    url: Option<String>,
}

impl ReviewRow {
    /// The row of `plan`, if its latest review is pending; reads the port file. `host` is the review host (contract
    /// §6.5), asked for only when there is a port to link.
    fn of(plan: &PlanBoxes, home: &CpdHome, host: impl FnOnce() -> String) -> Option<ReviewRow> {
        let review = plan.review.as_ref().filter(|r| r.status == ReviewStatus::Pending)?;
        let port: Option<u16> =
            fs::read_to_string(home.plannotator_port(&plan.plan)).ok().and_then(|text| text.trim().parse().ok());
        Some(ReviewRow { number: review.number, url: port.map(|port| format!("http://{}:{port}", host())) })
    }

    fn line(&self) -> Line<'static> {
        let mut spans = vec![
            Span::raw(format!(" Plan review r{} ", self.number)),
            Span::styled("pending", palette::fg(Meaning::Review)),
        ];
        if self.url.is_some() {
            spans.push(format!(" {CUE}").into());
        }
        Line::from(spans)
    }

    /// Draws the row in `area` (one row), indented by the chart's margin.
    fn draw(&self, buf: &mut Buffer, area: Rect) {
        let area = area.inner(Margin { horizontal: MARGIN.horizontal, vertical: 0 });
        let line = self.line();
        let width = line.width() as u16;
        line.render(area, buf);
        if let Some(url) = &self.url {
            // from `Plan` (after the leading blank) through `󰏌`
            link(buf, Rect::new(area.x + 1, area.y, width - 1, 1).intersection(area), url);
        }
    }
}

/// What the pane shows when a Lead is in the chat slot.
struct Centre {
    slot: ChatSlot,
    plan: PlanBoxes,
    review: Option<ReviewRow>,
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

/// One read of the store: the Pulse of the Manager in the chat slot, or the Plan of the Lead there, if any, and the
/// Event id to follow from. At most one of `pulse` and `centre` is `Some`.
struct Snapshot {
    cursor: i64,
    centre: Option<Centre>,
    pulse: Option<Pulse>,
}

/// One read of the store for a pane `width` columns wide at `now_ms` (Unix ms).
fn snapshot(store: &mut Store, home: &CpdHome, width: u16, now_ms: i64) -> Result<Snapshot, CpdError> {
    // Read first: whatever the later reads see is at least this new, so following from here loses nothing.
    let cursor = store.read(highest_id)?;
    if let Some(manager) = read::manager_slot(store)? {
        // Read for the widest the ids allow; the drawing keeps the last buckets that fit.
        let buckets = pulse::buckets(usize::from(width).saturating_sub(2 * usize::from(MARGIN.horizontal)), 0);
        let pulse = read::pulse(store, &manager.context, now_ms, buckets)?;
        return Ok(Snapshot { cursor, centre: None, pulse: Some(pulse) });
    }
    let Some(slot) = read::chat_slot(store)? else { return Ok(Snapshot { cursor, centre: None, pulse: None }) };
    let plan = read::plan_boxes(store, &slot.plan)?;
    let frame = palette::frame(SystemTime::now());
    let boxes = boxes::layout(&plan.prs, &plan.jobs, frame).with_margin(MARGIN);
    let review = ReviewRow::of(&plan, home, || review_host(&mut std::io::sink()));
    Ok(Snapshot { cursor, centre: Some(Centre { slot, plan, review, boxes, frame }), pulse: None })
}

/// Whether an Event changes what the pane shows: the Plan's or the Context's, or a move of a Lead (the chat slot may
/// change); while Pulse shows, any Event of its Context.
fn concerns(event: &Event, centre: Option<&Centre>, pulse: Option<&Pulse>) -> bool {
    let mine = |column: &Option<String>, id: &str| column.as_deref() == Some(id);
    event.kind == "session-moved"
        || centre.is_some_and(|c| mine(&event.plan, c.plan.plan.as_str()) || mine(&event.context, c.plan.context.as_str()))
        || pulse.is_some_and(|p| mine(&event.context, p.context.as_str()))
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
    /// The first Pulse row shown (reset whenever the Context of the Pulse changes).
    pulse_offset: usize,
    note: String,
    error: Option<String>,
    /// The area clicks and the wheel use: the chart's window, or the Pulse rows inside the margin.
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

    /// Draws Pulse: the rows inside the margin, then the message row (an error, ` no Plans yet`, the note).
    fn draw_pulse(&mut self, f: &mut Frame, pulse: &Pulse, frame: usize) {
        let mut window = f.area();
        let mut parts: Vec<Span<'static>> =
            self.error.iter().map(|e| Span::styled(e.clone(), palette::note())).collect();
        if pulse.rows.is_empty() {
            parts.push(" no Plans yet".dim());
        }
        if !self.note.is_empty() {
            parts.push(Span::styled(self.note.clone(), palette::note()));
        }
        if !parts.is_empty() {
            let [rows, message_area] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(window);
            f.render_widget(
                Paragraph::new(Line::from(spaced(parts))),
                message_area.inner(Margin { horizontal: MARGIN.horizontal, vertical: 0 }),
            );
            window = rows;
        }
        self.body = window.inner(MARGIN);
        self.pulse_offset = pulse::draw(f.buffer_mut(), self.body, pulse, self.pulse_offset, frame);
    }

    fn draw(&mut self, f: &mut Frame, centre: Option<&Centre>) {
        let body = f.area();
        let mut window = body; // the chart's own margin keeps it off the pane's edges at scroll (0, 0)
        if let Some(row) = centre.and_then(|c| c.review.as_ref()) {
            let [row_area, below] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(window);
            row.draw(f.buffer_mut(), row_area);
            window = below;
        }
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
    let width = ratatui::crossterm::terminal::size()
        .map_err(|e| {
            CpdError::new(ErrorCode::NotATerminal, format!("The size of the terminal of cpd tui view plan is unknown: {e}."))
                .with("command", "tui view plan")
        })?
        .0;
    let mut snap = snapshot(&mut store, &home, width, now_ms())?;
    let actions = Actions::new(home.clone(), &config, run_command);
    let poll = Duration::from_millis(u64::from(config.events.poll_ms));

    // From here on this process owns the terminal (commands.md §1.2).
    let mut terminal = ratatui::init();
    let mouse = ratatui::crossterm::execute!(std::io::stdout(), EnableMouseCapture);
    let result = mouse.map_err(|e| e.to_string()).and_then(|()| {
        show(&mut terminal, &mut store, &home, &mut snap, poll, &actions).map_err(|e| e.to_string())
    });
    let _ = ratatui::crossterm::execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    match result {
        Ok(()) => Ok(Output::Owned),
        Err(error) => Err(CpdError::new(ErrorCode::NotATerminal, format!("The terminal of cpd tui view plan failed: {error}."))
            .with("command", "tui view plan")),
    }
}

/// Now, in Unix ms.
fn now_ms() -> i64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// Draws, follows Events and takes input until the terminal fails.
fn show(
    terminal: &mut ratatui::DefaultTerminal,
    store: &mut Store,
    home: &CpdHome,
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
    let periods = super::reread(COALESCE, REFRESH);
    let (mut stale, mut taken, mut fetched) = (false, Instant::now(), Instant::now());
    loop {
        if let Some(c) = snap.centre.as_mut() {
            c.turn(palette::frame(SystemTime::now()));
        }
        terminal.draw(|f| match snap.pulse.as_ref() {
            Some(pulse) => ui.draw_pulse(f, pulse, palette::frame(SystemTime::now())),
            None => ui.draw(f, snap.centre.as_ref()),
        })?;
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
                    stale |= events.iter().any(|e| concerns(e, snap.centre.as_ref(), snap.pulse.as_ref()));
                }
                Err(e) => ui.error = Some(e.to_string()),
            }
            let due = if snap.pulse.is_some() { pulse_due(stale, taken.elapsed(), periods.0) } else { due(stale, taken.elapsed(), periods) };
            if due {
                let before = snap.centre.as_ref().map(|c| (c.slot.session.clone(), c.plan.plan.clone()));
                let before_pulse = snap.pulse.as_ref().map(|p| p.context.clone());
                let width = terminal.size().map_or(0, |s| s.width);
                match snapshot(store, home, width, now_ms()) {
                    Ok(next) => {
                        *snap = next;
                        ui.error = None;
                        let after = snap.centre.as_ref().map(|c| (c.slot.session.clone(), c.plan.plan.clone()));
                        if before_pulse != snap.pulse.as_ref().map(|p| p.context.clone()) {
                            ui.pulse_offset = 0;
                        }
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
        let mut lead = None::<SessionId>; // the Lead a click on a Pulse row opens
        match event::read()? {
            Input::Resize(..) => stale = true, // the bucket count changes
            Input::Mouse(m) if snap.pulse.is_some() => {
                let Some(pulse) = snap.pulse.as_ref() else { continue };
                let height = usize::from(ui.body.height);
                match m.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        lead = pulse::row_at(ui.body, ui.pulse_offset, pulse.rows.len(), m.column, m.row)
                            .map(|i| pulse.rows[i].lead.clone());
                    }
                    MouseEventKind::ScrollUp => ui.pulse_offset = pulse::scroll(ui.pulse_offset, false, pulse.rows.len(), height),
                    MouseEventKind::ScrollDown => ui.pulse_offset = pulse::scroll(ui.pulse_offset, true, pulse.rows.len(), height),
                    _ => {}
                }
            }
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
        if let Some(lead) = lead {
            ui.note = actions.open_lead(&lead).err().map(|e| e.message().to_owned()).unwrap_or_default();
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
    use crate::read::{JobNode, JobStatus, PrNode, PrStatus, Pull, Review};
    use cpd_core::config::Tier;
    use cpd_core::ids::{ContextId, JobKey, PrKey, SessionId};


    #[test]
    fn the_snapshot_is_retaken_after_the_coalescing_when_stale_else_every_two_seconds() {
        let (ms, release) = (Duration::from_millis, (COALESCE, REFRESH));
        assert!(!due(true, ms(499), release) && due(true, ms(500), release), "an Event that concerns it: coalesced at 500 ms");
        assert!(!due(false, ms(1999), release) && due(false, ms(2000), release), "no Event: every 2 s");
        assert!(due(false, Duration::from_secs(60), release));
    }

    const URL: &str = "http://review.test:19433";
    const OPEN: &str = "\x1b]8;;http://review.test:19433\x07";

    fn test_host() -> String {
        "review.test".to_owned()
    }

    fn review(status: ReviewStatus) -> Review {
        Review {
            id: "r2".into(),
            number: 2,
            status,
            fingerprint: "0".repeat(64),
            current: true,
            feedback: None,
            requested_at: "2026-01-02T00:00:00.000Z".into(),
            decided_at: None,
        }
    }

    fn centre(review: Option<ReviewRow>) -> Centre {
        let prs = vec![PrNode {
            key: PrKey::parse("api").unwrap(),
            repo_name: "api".into(),
            base: "main".into(),
            status: PrStatus::Open,
            needs: Vec::new(),
            pull: Some(Pull {
                number: 12,
                state: read::PrState::Open,
                url: "https://github.com/acme/api/pull/12".into(),
                conflicting: false,
            }),
        }];
        let plan = PlanBoxes {
            plan: PlanId::parse("ship").unwrap(),
            context: ContextId::parse("ctx").unwrap(),
            prs,
            jobs: Vec::new(),
            review: None,
            events_cursor: 0,
        };
        let boxes = boxes::layout(&plan.prs, &plan.jobs, 0).with_margin(MARGIN);
        let slot = ChatSlot {
            session: SessionId::parse("lead-ship").unwrap(),
            name: "lead".into(),
            plan: plan.plan.clone(),
            moved_event: 1,
        };
        Centre { slot, plan, review, boxes, frame: 0 }
    }

    /// A cell's symbol without the OSC 8 link around it.
    fn plain(symbol: &str) -> String {
        symbol.replace(OPEN, "").replace("\x1b]8;;https://github.com/acme/api/pull/12\x07", "").replace("\x1b]8;;\x07", "")
    }

    use ratatui::buffer::CellWidth;

    /// The columns of row `y` covered by a link opened with `open`, a piece's second cell included.
    fn linked(buf: &Buffer, y: u16, open: &str) -> Vec<u16> {
        let mut xs = Vec::new();
        for x in 0..buf.area.width {
            if buf[(x, y)].symbol().contains(open) {
                xs.extend(x..x + buf[(x, y)].cell_width().max(1));
            }
        }
        xs
    }

    /// The pane `centre` draws on a 60 × 8 terminal, and its screen text, one string per row.
    fn drawn(centre: &Centre) -> (Buffer, Vec<String>, Ui) {
        let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
        let mut ui = Ui::default();
        terminal.draw(|f| ui.draw(f, Some(centre))).unwrap();
        let buf = terminal.backend().buffer().clone();
        let rows = (0..8)
            .map(|y| {
                let mut covered = 0;
                (0..60)
                    .map(|x| {
                        // the second cell of a link piece is drawn by the piece
                        if covered > 0 {
                            covered -= 1;
                            return String::new();
                        }
                        covered = buf[(x, y)].cell_width().saturating_sub(1);
                        plain(buf[(x, y)].symbol())
                    })
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect();
        (buf, rows, ui)
    }

    #[test]
    fn a_pending_review_is_a_row_above_the_boxes_which_start_one_row_lower() {
        let row = ReviewRow { number: 2, url: Some(URL.into()) };
        assert_eq!(row.line().to_string(), " Plan review r2 pending 󰏌");
        assert_eq!(row.line().spans[1].style.fg, palette::fg(Meaning::Review).fg);

        let without = drawn(&centre(None));
        let (buf, rows, ui) = drawn(&centre(Some(row)));
        // the row is indented by the margin, on the pane's first row; the boxes are one row lower than they were
        assert_eq!(rows[0], "   Plan review r2 pending 󰏌");
        assert_eq!(rows[1], "", "the boxes' own margin row");
        assert_eq!(rows[2], without.1[1], "the first border, one row lower");
        assert_eq!(rows[2], "  ┌ api · api 󱦰 main · #12 open 󰏌 ┐");
        assert_eq!(without.1[0], "");
        assert_eq!((ui.body.y, ui.body.height), (1, 6), "clicks and scrolling use the area between the row and the message row");
        assert_eq!(without.2.body.y, 0);
        // `pending` is yellow, `󰏌` links to the plannotator page; the title's `󰏌` still links to the pull request
        assert_eq!(buf[(18, 0)].fg, palette::fg(Meaning::Review).fg.unwrap());
        assert_eq!(linked(&buf, 0, OPEN), (3..=26).collect::<Vec<u16>>(), "`Plan` through `󰏌`, not the blank before it");
        let pull = "\x1b]8;;https://github.com/acme/api/pull/12\x07";
        let title: Vec<char> = rows[2].chars().collect();
        let (first, arrow) = (title.iter().position(|&c| c == '#').unwrap() as u16, title.iter().position(|&c| c.to_string() == CUE).unwrap() as u16);
        assert_eq!(linked(&buf, 2, pull), (first..=arrow).collect::<Vec<u16>>(), "the title's `#12 open 󰏌` links to the pull request");
    }

    #[test]
    fn a_review_row_without_a_port_has_no_arrow_and_no_link() {
        let row = ReviewRow { number: 2, url: None };
        let (buf, rows, _) = drawn(&centre(Some(row)));
        assert_eq!(rows[0], "   Plan review r2 pending");
        assert!((0..60).all(|x| !buf[(x, 0)].symbol().contains("]8;;")));
        assert!(linked(&buf, 0, "]8;;").is_empty());
    }

    #[test]
    fn no_review_row_while_the_review_is_not_pending() {
        let home = CpdHome::resolve(Some(std::env::temp_dir().join("cpd-plan-view-no-home").into()), None).unwrap();
        let mut plan = centre(None).plan;
        assert_eq!(ReviewRow::of(&plan, &home, test_host), None, "no review");
        for status in [ReviewStatus::Approved, ReviewStatus::Rejected] {
            plan.review = Some(review(status));
            assert_eq!(ReviewRow::of(&plan, &home, test_host), None, "{status:?}");
        }
        plan.review = Some(review(ReviewStatus::Pending));
        assert_eq!(ReviewRow::of(&plan, &home, test_host), Some(ReviewRow { number: 2, url: None }), "no port file");
    }

    #[test]
    fn the_port_is_the_trimmed_line_of_the_port_file_and_must_be_a_port() {
        let dir = std::env::temp_dir().join(format!("cpd-plan-view-port-{}", std::process::id()));
        let home = CpdHome::resolve(Some(dir.clone().into()), None).unwrap();
        let mut plan = centre(None).plan;
        plan.review = Some(review(ReviewStatus::Pending));
        let file = home.plannotator_port(&plan.plan);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        for (text, url) in [
            ("19433\n", Some(URL)),
            ("  19433  ", Some(URL)),
            ("", None),
            ("nineteen\n", None),
            ("70000\n", None),
            ("19433\n19434\n", None),
        ] {
            fs::write(&file, text).unwrap();
            assert_eq!(ReviewRow::of(&plan, &home, test_host).unwrap().url.as_deref(), url, "{text:?}");
        }
        fs::remove_dir_all(dir).unwrap();
    }


    const PANE: (u16, u16) = (40, 10);

    fn plan(id: &str) -> PlanId {
        PlanId::parse(id).unwrap()
    }

    /// A Plan of one PR with `jobs` independent Jobs: a chart `jobs` rows tall (and some more for the box).
    fn scroll_centre(plan_id: &str, jobs: usize) -> Centre {
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
            tier: Some(Tier::parse("S1").unwrap()),
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
            review: None,
            events_cursor: 0,
        };
        let boxes = boxes::layout(&plan.prs, &plan.jobs, 0).with_margin(MARGIN);
        let slot = ChatSlot {
            session: SessionId::parse("00000000-0000-4000-8000-000000000000").unwrap(),
            name: "lead".into(),
            plan: plan.plan.clone(),
            moved_event: 0,
        };
        Centre { slot, plan, review: None, boxes, frame: 0 }
    }

    fn draw(ui: &mut Ui, centre: &Centre) {
        let mut terminal = Terminal::new(TestBackend::new(PANE.0, PANE.1)).unwrap();
        terminal.draw(|f| ui.draw(f, Some(centre))).unwrap();
    }

    #[test]
    fn switching_plans_and_back_restores_the_offset_with_no_selection() {
        let (a, b) = (plan("plan-a"), plan("plan-b"));
        let mut ui = Ui::default();
        draw(&mut ui, &scroll_centre("plan-a", 30));
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

        let small = scroll_centre("plan-a", 12);
        draw(&mut ui, &small);
        let height = small.boxes.chart.pills.iter().map(|p| p.rect.bottom()).max().unwrap();
        assert!(ui.scroll.1 < 20 && ui.scroll.1 + PANE.1 >= height, "{:?}, chart {height} rows", ui.scroll);
    }

    #[test]
    fn a_view_that_restarts_starts_at_the_top() {
        assert_eq!(Ui::default().scroll, (0, 0));
        assert!(Ui::default().remembered.is_empty());
    }

    fn pulse_of(rows: &[&str]) -> Pulse {
        let bucket = |count| read::Bucket { count, job_merged: false, pr_merged: false, worker_ran: false };
        let rows = rows
            .iter()
            .map(|id| read::PulseRow {
                plan: PlanId::parse(id).unwrap(),
                lead: SessionId::parse(&format!("lead-{id}")).unwrap(),
                dot: read::Dot::Idle,
                buckets: vec![bucket(1); 12],
                merges: 0,
                age_minutes: Some(3),
                quiet: false,
            })
            .collect();
        Pulse { context: ContextId::parse("ctx").unwrap(), rows, max: 1, events_cursor: 0 }
    }

    fn pulse_screen(ui: &mut Ui, pulse: &Pulse, height: u16) -> (Buffer, Vec<String>) {
        let mut terminal = Terminal::new(TestBackend::new(60, height)).unwrap();
        terminal.draw(|f| ui.draw_pulse(f, pulse, 0)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let rows = (0..height)
            .map(|y| (0..60).map(|x| buf[(x, y)].symbol().to_owned()).collect::<String>().trim_end().to_owned())
            .collect();
        (buf, rows)
    }

    #[test]
    fn pulse_rows_sit_inside_the_margin_and_a_click_selects_the_rows_lead_with_the_offset() {
        let pulse = pulse_of(&["aa", "bb", "cc", "dd", "ee", "ff"]);
        let mut ui = Ui::default();
        let (_, rows) = pulse_screen(&mut ui, &pulse, 5); // margin row, then 3 rows, then the margin row
        assert_eq!(rows[0], "");
        let idle = palette::dot_span(read::Dot::Idle, 0, false).content;
        assert!(rows[1].starts_with(&format!("  {idle} aa  ")), "{:?}", rows[1]);
        assert!(rows[1].ends_with("         3m"), "{:?}", rows[1]);
        assert_eq!(ui.body, Rect::new(2, 1, 56, 3));
        let lead = |ui: &Ui, y| {
            pulse::row_at(ui.body, ui.pulse_offset, pulse.rows.len(), 5, y).map(|i| pulse.rows[i].lead.to_string())
        };
        assert_eq!(lead(&ui, 2).as_deref(), Some("lead-bb"));
        ui.pulse_offset = pulse::scroll(ui.pulse_offset, true, 6, 3);
        ui.pulse_offset = pulse::scroll(ui.pulse_offset, true, 6, 3);
        assert_eq!(ui.pulse_offset, 3, "clamped at the last row");
        let (_, rows) = pulse_screen(&mut ui, &pulse, 5);
        assert!(rows[1].starts_with(&format!("  {idle} dd  ")), "{:?}", rows[1]);
        assert_eq!(lead(&ui, 1).as_deref(), Some("lead-dd"));
        assert_eq!(lead(&ui, 3).as_deref(), Some("lead-ff"));
    }

    #[test]
    fn a_context_without_plans_says_so_dim_in_the_message_row() {
        let mut ui = Ui::default();
        let (buf, rows) = pulse_screen(&mut ui, &pulse_of(&[]), 5);
        assert_eq!(rows[4], "   no Plans yet");
        assert!(buf[(3, 4)].modifier.contains(ratatui::style::Modifier::DIM));
    }

    #[test]
    fn the_pulse_re_reads_on_its_contexts_events_every_minute_and_on_resize() {
        let event = |kind: &str, context: &str| Event {
            id: 1,
            at: "2026-01-01T00:00:00.000Z".into(),
            kind: kind.into(),
            context: Some(context.into()),
            plan: None,
            job: None,
            attempt: None,
            session: None,
            text: String::new(),
            data: serde_json::Value::Null,
        };
        let pulse = pulse_of(&["aa"]);
        assert!(concerns(&event("job-merged", "ctx"), None, Some(&pulse)));
        assert!(!concerns(&event("job-merged", "other"), None, Some(&pulse)));
        assert!(concerns(&event("session-moved", "other"), None, Some(&pulse)));
        let ms = Duration::from_millis;
        assert!(pulse_due(true, ms(500), COALESCE) && !pulse_due(true, ms(499), COALESCE));
        assert!(!pulse_due(false, Duration::from_secs(59), COALESCE) && pulse_due(false, Duration::from_secs(60), COALESCE));
    }

    fn fixture(store: &mut Store, sql: &str) {
        store
            .write("fixture", |tx| {
                tx.execute_batch(sql)?;
                Ok::<(), CpdError>(())
            })
            .unwrap();
    }

    fn session_row(id: &str, role: &str, plan: Option<&str>) -> String {
        let plan = plan.map_or("NULL".to_owned(), |p| format!("'{p}'"));
        format!(
            "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at)
             VALUES ('{id}', '{role}', 'ctx', {plan}, NULL, 'n', '/s/{id}.jsonl', '/repo', 'O3', NULL, 'idle', NULL,
                     '2026-01-03T00:00:00.000Z', '2026-01-02T00:00:00.000Z');"
        )
    }

    fn moved(session: &str) -> String {
        format!(
            "INSERT INTO events (txn, at, kind, context, plan, session, text, data)
             VALUES (cpd_txn(), '2026-10-18T15:00:00.000Z', 'session-moved', 'ctx', NULL, '{session}', 'f',
                     '{{\"pane\":\"%1\",\"window\":\"@1\",\"place\":\"chat\"}}');"
        )
    }

    #[test]
    fn the_snapshot_holds_pulse_while_the_manager_is_newest_in_the_slot_and_the_plan_once_a_lead_moved_there() {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().to_path_buf().into()), None).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        fixture(
            &mut store,
            &format!(
                "INSERT INTO contexts (id, title, jira, created_at, created_on) VALUES ('ctx', 'T', NULL, '2026-01-01T00:00:00.000Z', 'm1');
                 {}{}
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                 VALUES ('p1', 'ctx', 'P', 'dispatch', 'lead', '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
                 INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
                 VALUES ('p1', 'repo', '/repo', 'main', 'dispatch', 'cpd/p1', '/wt', cpd_txn(), '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z');
                 UPDATE sessions SET plan = 'p1' WHERE id = 'lead';
                 {}",
                session_row("mgr", "manager", None),
                session_row("lead", "lead", None),
                moved("mgr")
            ),
        );
        let now = 1_792_336_050_000;
        let snap = snapshot(&mut store, &home, 125, now).unwrap();
        let pulse = snap.pulse.expect("the Manager is in the slot");
        assert!(snap.centre.is_none());
        assert_eq!(pulse.context.as_str(), "ctx");
        assert_eq!(pulse.rows.iter().map(|r| r.plan.as_str()).collect::<Vec<_>>(), ["p1"]);
        assert_eq!(pulse.rows[0].buckets.len(), 125 - 4 - 21, "as wide as the pane allows for ids of any width");

        fixture(&mut store, &moved("lead"));
        let snap = snapshot(&mut store, &home, 125, now).unwrap();
        assert!(snap.pulse.is_none());
        assert_eq!(snap.centre.expect("the Lead is in the slot").plan.plan.as_str(), "p1");
    }
}
