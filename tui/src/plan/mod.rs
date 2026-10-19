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
//! The Plans' scroll offsets survive a restart of the view, in `run/tui/plan-scroll.json` ([`Offsets`], hs-d254): the
//! one file a view writes, its own state.
//!
//! Keys: ←→↑↓/hjkl select (←→ within a row of boxes, ↑↓ across rows too), Enter opens or closes the Worker, Esc clears; the wheel scrolls; clicks select a pill. There is no `q`: a layout pane that
//! dies stays dead (contract §6.1).

mod actions;
mod boxes;
mod flow;

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
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
use crate::link::link;
use crate::palette::{self, Meaning};
use super::read::{self, ChatSlot, PlanBoxes, ReviewStatus};
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

/// The snapshot is retaken at least this often, Event or not: a conflict clears with no Event (`mergeable` changes
/// silently), as the `widgets` view's `REFRESH` (§17).
const REFRESH: Duration = Duration::from_secs(2);

/// Whether the snapshot taken `since` ago is retaken: after [`COALESCE`] once an Event concerns it (`stale`), else after
/// [`REFRESH`].
fn due(stale: bool, since: Duration) -> bool {
    since >= if stale { COALESCE } else { REFRESH }
}

/// The view redraws at least this often, for the spinner (one frame: palette.rs); a redraw reads nothing from the store.
const FRAME: Duration = Duration::from_millis(100);

/// The blank space around the chart's content, inside the canvas the view scrolls over: at scroll (0, 0) nothing of the
/// chart touches the pane's edges, and scrolled the chart reaches them.
const MARGIN: Margin = Margin { horizontal: 2, vertical: 1 };

/// The row above the boxes while the Plan's latest Plan review is pending (commands.md §13.2): ` Plan review r<n> pending ↗`.
#[derive(Debug, PartialEq, Eq)]
struct ReviewRow {
    number: u32,
    /// `http://localhost:<port>`, the port of `run/<plan>/plannotator-port`; `None` when that file is missing or holds
    /// no port: the row shows without the `↗`.
    url: Option<String>,
}

impl ReviewRow {
    /// The row of `plan`, if its latest review is pending; reads the port file.
    fn of(plan: &PlanBoxes, home: &CpdHome) -> Option<ReviewRow> {
        let review = plan.review.as_ref().filter(|r| r.status == ReviewStatus::Pending)?;
        let port: Option<u16> =
            fs::read_to_string(home.plannotator_port(&plan.plan)).ok().and_then(|text| text.trim().parse().ok());
        Some(ReviewRow { number: review.number, url: port.map(|port| format!("http://localhost:{port}")) })
    }

    fn line(&self) -> Line<'static> {
        let mut spans = vec![
            Span::raw(format!(" Plan review r{} ", self.number)),
            Span::styled("pending", palette::fg(Meaning::Review)),
        ];
        if self.url.is_some() {
            spans.push(" ↗".into());
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
            // from `Plan` (after the leading blank) through `↗`
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

/// One read of the store: the Plan of the chat slot, if any, and the Event id to follow from.
struct Snapshot {
    cursor: i64,
    centre: Option<Centre>,
}

fn snapshot(store: &mut Store, home: &CpdHome) -> Result<Snapshot, CpdError> {
    // Read first: whatever the later reads see is at least this new, so following from here loses nothing.
    let cursor = store.read(highest_id)?;
    let Some(slot) = read::chat_slot(store)? else { return Ok(Snapshot { cursor, centre: None }) };
    let plan = read::plan_boxes(store, &slot.plan)?;
    let frame = palette::frame(SystemTime::now());
    let boxes = boxes::layout(&plan.prs, &plan.jobs, frame).with_margin(MARGIN);
    let review = ReviewRow::of(&plan, home);
    Ok(Snapshot { cursor, centre: Some(Centre { slot, plan, review, boxes, frame }) })
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
    /// The scroll offset each Plan was left at (hs-d197), read from [`Offsets`] at the view's start. Not clamped: the
    /// chart may have changed size by the time it comes back, and a draw clamps.
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

    /// Records the offset of `plan`, the Plan on show, once it differs from the one remembered (a Plan never scrolled
    /// is at its corner). Called after a draw, so the offset is clamped.
    fn remember(&mut self, plan: &PlanId) {
        if self.remembered.get(plan).copied().unwrap_or((0, 0)) != self.scroll {
            self.remembered.insert(plan.clone(), self.scroll);
        }
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

/// The Plans' scroll offsets in `$CPD_HOME/run/tui/plan-scroll.json` (commands.md §13.2, hs-d254): a JSON object
/// `{"<plan id>": [x, y]}`. The view's own state, not Ledger state: the one file a view writes.
struct Offsets {
    path: PathBuf,
    /// What the file holds, as far as this view knows; a write is due when the offsets differ from it.
    saved: HashMap<PlanId, (u16, u16)>,
    /// When the file was last written, or tried.
    written: Option<Instant>,
}

/// A write is due at most this often (an offset that changes more often waits).
const SAVE_EVERY: Duration = Duration::from_secs(1);

impl Offsets {
    fn at(home: &CpdHome) -> Offsets {
        // Not in `CpdHome` (contract §1.4): the layout's one source of truth lists this file there.
        let path = home.run_dir().join("tui").join("plan-scroll.json");
        Offsets { path, saved: HashMap::new(), written: None }
    }

    /// The offsets in the file: none when it is missing; `Err` with the reason when it cannot be read or is not
    /// `{"<plan id>": [x, y]}`.
    fn load(&mut self) -> Result<HashMap<PlanId, (u16, u16)>, String> {
        let text = match fs::read(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(HashMap::new()),
            Err(e) => return Err(format!("{}: {e}", self.path.display())),
        };
        let offsets: HashMap<PlanId, (u16, u16)> =
            serde_json::from_slice(&text).map_err(|e| format!("{}: {e}", self.path.display()))?;
        self.saved = offsets.clone();
        Ok(offsets)
    }

    /// Writes `offsets` when they differ from the file and, unless `now` (the Plan changed), a second has passed
    /// since the last write. A failure is returned and leaves the offsets unsaved, so the next call retries.
    fn save(&mut self, offsets: &HashMap<PlanId, (u16, u16)>, now: bool) -> Result<(), String> {
        if *offsets == self.saved || (!now && self.written.is_some_and(|at| at.elapsed() < SAVE_EVERY)) {
            return Ok(());
        }
        self.written = Some(Instant::now());
        self.write(offsets).map_err(|e| format!("scroll positions not saved: {}: {e}", self.path.display()))?;
        self.saved.clone_from(offsets);
        Ok(())
    }

    /// Replaces the file: a temporary file beside it, then a rename.
    fn write(&self, offsets: &HashMap<PlanId, (u16, u16)>) -> io::Result<()> {
        let sorted: BTreeMap<&str, (u16, u16)> = offsets.iter().map(|(plan, at)| (plan.as_str(), *at)).collect();
        let text = serde_json::to_vec(&sorted).map_err(io::Error::other)?;
        let dir = self.path.parent().unwrap_or(Path::new("."));
        fs::create_dir_all(dir)?;
        let tmp = self.path.with_extension(format!("json.tmp{}", std::process::id()));
        fs::write(&tmp, text).and_then(|()| fs::rename(&tmp, &self.path)).inspect_err(|_| {
            let _ = fs::remove_file(&tmp);
        })
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
    let mut snap = snapshot(&mut store, &home)?;
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
    let mut offsets = Offsets::at(home);
    let loaded = offsets.load();
    // an unreadable or invalid file starts empty; it is replaced at the next write
    ui.remembered = loaded.clone().unwrap_or_default();
    ui.switch(None, snap.centre.as_ref().map(|c| &c.plan.plan));
    if let Err(reason) = loaded {
        ui.note = format!("scroll positions reset: {reason}");
    }
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
        if let Some(c) = snap.centre.as_ref() {
            ui.remember(&c.plan.plan);
            if let Err(e) = offsets.save(&ui.remembered, false) {
                ui.error = Some(e);
            }
        }
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
            if due(stale, taken.elapsed()) {
                let before = snap.centre.as_ref().map(|c| (c.slot.session.clone(), c.plan.plan.clone()));
                match snapshot(store, home) {
                    Ok(next) => {
                        *snap = next;
                        ui.error = None;
                        let after = snap.centre.as_ref().map(|c| (c.slot.session.clone(), c.plan.plan.clone()));
                        if before.as_ref().map(|b| &b.0) != after.as_ref().map(|a| &a.0) {
                            ui.switch(before.as_ref().map(|b| &b.1), after.as_ref().map(|a| &a.1));
                            if let Err(e) = offsets.save(&ui.remembered, true) {
                                ui.error = Some(e);
                            }
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
    use crate::read::{JobNode, JobStatus, PrNode, PrStatus, Pull, Review};
    use cpd_core::config::Tier;
    use cpd_core::ids::{ContextId, JobKey, PrKey, SessionId};


    #[test]
    fn the_snapshot_is_retaken_after_the_coalescing_when_stale_else_every_two_seconds() {
        let ms = Duration::from_millis;
        assert!(!due(true, ms(499)) && due(true, ms(500)), "an Event that concerns it: coalesced at 500 ms");
        assert!(!due(false, ms(1999)) && due(false, ms(2000)), "no Event: every 2 s");
        assert!(due(false, Duration::from_secs(60)));
    }

    const URL: &str = "http://localhost:19433";
    const OPEN: &str = "\x1b]8;;http://localhost:19433\x07";

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
        assert_eq!(row.line().to_string(), " Plan review r2 pending ↗");
        assert_eq!(row.line().spans[1].style.fg, palette::fg(Meaning::Review).fg);

        let without = drawn(&centre(None));
        let (buf, rows, ui) = drawn(&centre(Some(row)));
        // the row is indented by the margin, on the pane's first row; the boxes are one row lower than they were
        assert_eq!(rows[0], "   Plan review r2 pending ↗");
        assert_eq!(rows[1], "", "the boxes' own margin row");
        assert_eq!(rows[2], without.1[1], "the first border, one row lower");
        assert_eq!(rows[2], "  ┌ api · api → main · #12 open ↗ ┐");
        assert_eq!(without.1[0], "");
        assert_eq!((ui.body.y, ui.body.height), (1, 6), "clicks and scrolling use the area between the row and the message row");
        assert_eq!(without.2.body.y, 0);
        // `pending` is yellow, `↗` links to the plannotator page; the title's `↗` still links to the pull request
        assert_eq!(buf[(18, 0)].fg, palette::fg(Meaning::Review).fg.unwrap());
        assert_eq!(linked(&buf, 0, OPEN), (3..=26).collect::<Vec<u16>>(), "`Plan` through `↗`, not the blank before it");
        let pull = "\x1b]8;;https://github.com/acme/api/pull/12\x07";
        let title: Vec<char> = rows[2].chars().collect();
        let (first, arrow) = (title.iter().position(|&c| c == '#').unwrap() as u16, title.iter().position(|&c| c == '↗').unwrap() as u16);
        assert_eq!(linked(&buf, 2, pull), (first..=arrow).collect::<Vec<u16>>(), "the title's `#12 open ↗` links to the pull request");
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
        assert_eq!(ReviewRow::of(&plan, &home), None, "no review");
        for status in [ReviewStatus::Approved, ReviewStatus::Rejected] {
            plan.review = Some(review(status));
            assert_eq!(ReviewRow::of(&plan, &home), None, "{status:?}");
        }
        plan.review = Some(review(ReviewStatus::Pending));
        assert_eq!(ReviewRow::of(&plan, &home), Some(ReviewRow { number: 2, url: None }), "no port file");
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
            assert_eq!(ReviewRow::of(&plan, &home).unwrap().url.as_deref(), url, "{text:?}");
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
    fn a_fresh_ui_starts_at_the_top_with_nothing_remembered() {
        assert_eq!(Ui::default().scroll, (0, 0));
        assert!(Ui::default().remembered.is_empty());
    }

    #[test]
    fn a_plan_never_scrolled_is_not_remembered_and_a_scrolled_one_is() {
        let a = plan("plan-a");
        let mut ui = Ui::default();
        ui.remember(&a);
        assert!(ui.remembered.is_empty(), "at its corner: nothing to keep");
        ui.scroll = (0, 4);
        ui.remember(&a);
        ui.scroll = (0, 0);
        ui.remember(&a);
        assert_eq!(ui.remembered.get(&a), Some(&(0, 0)), "back at the corner is a change too");
    }

    fn offsets_in(dir: &tempfile::TempDir) -> Offsets {
        Offsets::at(&CpdHome::resolve(Some(dir.path().into()), None).unwrap())
    }

    fn map(entries: &[(&str, (u16, u16))]) -> HashMap<PlanId, (u16, u16)> {
        entries.iter().map(|(id, at)| (plan(id), *at)).collect()
    }

    #[test]
    fn the_offsets_round_trip_through_the_file_which_is_a_json_object_of_plan_ids() {
        let dir = tempfile::tempdir().unwrap();
        let offsets = map(&[("plan-b", (4, 9)), ("plan-a", (0, 7))]);
        let mut writer = offsets_in(&dir);
        writer.save(&offsets, true).unwrap();
        let path = dir.path().join("run/tui/plan-scroll.json");
        assert_eq!(fs::read_to_string(&path).unwrap(), r#"{"plan-a":[0,7],"plan-b":[4,9]}"#);
        assert_eq!(offsets_in(&dir).load().unwrap(), offsets);
    }

    #[test]
    fn a_missing_file_is_an_empty_map() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(offsets_in(&dir).load().unwrap(), HashMap::new());
    }

    #[test]
    fn an_invalid_file_is_refused_with_its_reason_and_replaced_by_the_next_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run/tui/plan-scroll.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        for bad in ["", "{not json", "[]", r#"{"Bad Id":[0,1]}"#, r#"{"plan-a":[0]}"#, r#"{"plan-a":[0,-1]}"#, r#"{"plan-a":"top"}"#] {
            fs::write(&path, bad).unwrap();
            let reason = offsets_in(&dir).load().unwrap_err();
            assert!(reason.contains("plan-scroll.json"), "{bad:?}: {reason}");
        }
        let mut offsets = offsets_in(&dir);
        offsets.load().unwrap_err();
        offsets.save(&map(&[("plan-a", (0, 3))]), false).unwrap();
        assert_eq!(offsets_in(&dir).load().unwrap(), map(&[("plan-a", (0, 3))]));
    }

    #[test]
    fn an_unreadable_file_is_refused_with_its_reason() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("run/tui/plan-scroll.json")).unwrap(); // reading a directory fails
        let reason = offsets_in(&dir).load().unwrap_err();
        assert!(reason.contains("plan-scroll.json"), "{reason}");
    }

    #[test]
    fn a_write_leaves_no_temporary_file_and_creates_the_parents() {
        let dir = tempfile::tempdir().unwrap();
        let mut offsets = offsets_in(&dir);
        offsets.save(&map(&[("plan-a", (1, 2))]), true).unwrap();
        offsets.save(&map(&[("plan-a", (3, 4))]), true).unwrap();
        let names: Vec<_> = fs::read_dir(dir.path().join("run/tui")).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names, ["plan-scroll.json"]);
    }

    #[test]
    fn a_failed_write_is_reported_leaves_no_temporary_file_and_is_retried() {
        let dir = tempfile::tempdir().unwrap();
        let tui = dir.path().join("run/tui");
        fs::create_dir_all(tui.join("plan-scroll.json")).unwrap(); // a directory cannot be replaced by a file
        let mut offsets = offsets_in(&dir);
        let want = map(&[("plan-a", (0, 5))]);
        let error = offsets.save(&want, true).unwrap_err();
        assert!(error.starts_with("scroll positions not saved: "), "{error}");
        assert_eq!(fs::read_dir(&tui).unwrap().count(), 1, "only the directory is there");
        fs::remove_dir(tui.join("plan-scroll.json")).unwrap();
        offsets.save(&want, true).unwrap();
        assert_eq!(offsets_in(&dir).load().unwrap(), want);
    }

    #[test]
    fn a_write_waits_a_second_after_the_last_unless_the_plan_changed_and_unchanged_offsets_write_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run/tui/plan-scroll.json");
        let mut offsets = offsets_in(&dir);
        offsets.save(&HashMap::new(), false).unwrap();
        assert!(!path.exists(), "nothing changed: nothing written");
        offsets.save(&map(&[("plan-a", (0, 1))]), false).unwrap();
        offsets.save(&map(&[("plan-a", (0, 2))]), false).unwrap();
        assert_eq!(offsets_in(&dir).load().unwrap(), map(&[("plan-a", (0, 1))]), "the second change waits");
        offsets.save(&map(&[("plan-a", (0, 2))]), true).unwrap();
        assert_eq!(offsets_in(&dir).load().unwrap(), map(&[("plan-a", (0, 2))]), "a Plan change writes at once");
        offsets.written = Some(Instant::now() - SAVE_EVERY);
        offsets.save(&map(&[("plan-a", (0, 3))]), false).unwrap();
        assert_eq!(offsets_in(&dir).load().unwrap(), map(&[("plan-a", (0, 3))]), "a second later it writes");
    }
}
