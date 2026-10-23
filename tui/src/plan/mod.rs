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
//! The Plans' scroll offsets survive a restart of the view, in `run/tui/plan-scroll.json` ([`Offsets`], hs-d254): the
//! one file a view writes, its own state.
//!
//! Keys: ←→↑↓/hjkl select (←→ within a row of boxes, ↑↓ across rows too), Enter opens or closes the Worker, Esc clears; the wheel scrolls; clicks select a pill. There is no `q`: a layout pane that
//! dies is started again by the `pane-died` hook (contract §6.1), so the view never exits on a key.

mod actions;
pub(crate) mod boxes;
pub(crate) mod flow;
pub(crate) mod pulse;

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use ratatui::Frame;
use ratatui::crossterm::event::{self, Event as Input, KeyCode, KeyEventKind, MouseButton, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Margin, Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use self::actions::Actions;
use self::boxes::Boxes;
use self::flow::{Dir, Selection};
use crate::palette;
use super::background::Background;
use super::viewlog::ViewLog;
use crate::trace::Trace;
use super::read::{self, ChatSlot, PlanBoxes, Pulse};
use super::pace::{self, Reads};
use super::runner::{ViewError, kept};
use super::{Capture, RunCommand, run_view};
use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::{Event, highest_id};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId, SessionId};
use cpd_core::output::Output;
use cpd_core::store::Store;

/// A snapshot is retaken at most this often, whatever the Event rate (§17).
const COALESCE: Duration = Duration::from_millis(500);

/// Whether the snapshot taken `since` ago is retaken: after [`COALESCE`] once an Event concerns it or a pull request row
/// changed (`stale`: a conflict clears with no Event, `mergeable` changes silently, §17), never else.
fn due(stale: bool, since: Duration) -> bool {
    stale && since >= COALESCE
}

/// While Pulse shows the snapshot is retaken at least this often, Event or not: the window moves and the ages tick,
/// and a running Attempt is not an Event.
const PULSE_REFRESH: Duration = Duration::from_secs(60);

/// Whether the Pulse snapshot taken `since` ago is retaken: after [`COALESCE`] once an Event concerns it (or the pane
/// was resized), else after [`PULSE_REFRESH`].
fn pulse_due(stale: bool, since: Duration) -> bool {
    since >= if stale { COALESCE } else { PULSE_REFRESH }
}

/// The blank space around the chart's content, inside the canvas the view scrolls over: at scroll (0, 0) nothing of the
/// chart touches the pane's edges, and scrolled the chart reaches them.
const MARGIN: Margin = Margin { horizontal: 2, vertical: 1 };
/// The chart canvas keeps one blank column each side; Pulse and the message row keep `MARGIN`.
pub(crate) const CHART_MARGIN: Margin = Margin { horizontal: 1, vertical: 1 };

/// What the pane shows when a Lead is in the chat slot.
pub(crate) struct Centre {
    pub(crate) slot: ChatSlot,
    pub(crate) plan: PlanBoxes,
    pub(crate) boxes: Boxes,
    /// The spinner frame `boxes` is drawn at.
    pub(crate) frame: usize,
}

impl Centre {
    /// Whether a spinner is on screen (R1): the ONE function that names every spinner the plan view draws, one line each.
    pub(crate) fn spins(&self) -> bool {
        let pulls = || self.plan.prs.iter().filter_map(|p| p.pull.as_ref());
        false
            || self.plan.jobs.iter().any(|j| flow::Live::of(j, false) == flow::Live::Running) // a Running Job's pill icon
            || pulls().any(|p| p.lead_working) // a PR box title's Lead dot
    }

    /// Turn the spinners to `frame`, from the layout already done (R2: no layout at a frame change), when a spinner is on
    /// screen and the frame is not the one drawn. Whether it did.
    #[cfg(test)]
    pub(crate) fn turn(&mut self, frame: usize) -> bool {
        self.turn_traced(frame, &mut Trace::new(None))
    }

    /// [`Centre::turn`]; it lays nothing out, so the trace's `layout_us` stays 0. Only the glyph cells change: every frame is one cell wide
    /// and the plan view draws a `palette::SPINNER` glyph nowhere but at a spinner, so the cells holding the drawn
    /// frame's glyph (alone, or inside the link's escapes) get the new one's.
    fn turn_traced(&mut self, frame: usize, _trace: &mut Trace) -> bool {
        let turned = frame != self.frame && self.spins();
        if turned {
            let (from, to) = (palette::SPINNER[self.frame % palette::SPINNER.len()], palette::SPINNER[frame % palette::SPINNER.len()]);
            for cell in &mut self.boxes.chart.buf.content {
                // a cell under the pull request's link holds the glyph inside its hyperlink escapes
                if cell.symbol().contains(from) {
                    let turned = cell.symbol().replace(from, to);
                    cell.set_symbol(&turned);
                }
            }
            self.frame = frame;
        }
        turned
    }
}

/// One read of the store: the Pulse of the Manager in the chat slot, or the Plan of the Lead there, if any, and the
/// Event id to follow from. At most one of `pulse` and `centre` is `Some`.
struct Snapshot {
    cursor: i64,
    centre: Option<Centre>,
    pulse: Option<Pulse>,
    /// The time `boxes::layout` took, when the snapshot was asked to time it (the frame trace), else zero.
    layout: Duration,
}

/// One read of the store for a pane `width` columns wide at `now_ms` (Unix ms).
fn snapshot(store: &mut Store, width: u16, now_ms: i64) -> Result<Snapshot, CpdError> {
    snapshot_timed(store, width, now_ms, false)
}

/// [`snapshot`], timing its layout when `timed`.
fn snapshot_timed(store: &mut Store, width: u16, now_ms: i64, timed: bool) -> Result<Snapshot, CpdError> {
    // Read first: whatever the later reads see is at least this new, so following from here loses nothing.
    let cursor = store.read(highest_id)?;
    if let Some(manager) = read::manager_slot(store)? {
        // Read for the widest the ids allow; the drawing keeps the last buckets that fit.
        let buckets = pulse::buckets(usize::from(width).saturating_sub(2 * usize::from(MARGIN.horizontal)), 0);
        let pulse = read::pulse(store, &manager.context, now_ms, buckets)?;
        return Ok(Snapshot { cursor, centre: None, pulse: Some(pulse), layout: Duration::ZERO });
    }
    let Some(slot) = read::chat_slot(store)? else { return Ok(Snapshot { cursor, centre: None, pulse: None, layout: Duration::ZERO }) };
    let plan = read::plan_boxes(store, &slot.plan)?;
    let frame = palette::frame(SystemTime::now());
    let began = timed.then(Instant::now);
    let boxes = boxes::layout(&plan.prs, &plan.jobs, &plan.needs_user, frame).with_margin(CHART_MARGIN);
    let layout = began.map_or(Duration::ZERO, |b| b.elapsed());
    Ok(Snapshot { cursor, centre: Some(Centre { slot, plan, boxes, frame }), pulse: None, layout })
}

/// What the reader thread brings back: the Events since the last poll, or a snapshot.
enum Read {
    Poll(Result<read::Changes, CpdError>),
    Snapshot(Result<Snapshot, CpdError>),
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
/// or `no Plan yet`: lasting states of the view (commands.md §17 "No notes", rule (b)).
fn message(error: Option<&str>, centre: Option<&Centre>) -> Option<Line<'static>> {
    let mut parts: Vec<Span<'static>> = error.into_iter().map(|e| Span::styled(e.to_owned(), palette::error())).collect();
    match centre {
        None => parts.push(Span::styled(" no Lead session in the centre", palette::muted_text())),
        Some(c) if c.plan.prs.is_empty() => parts.push(Span::styled("no PR yet: cpd pr add", palette::muted_text())),
        Some(c) if c.plan.jobs.is_empty() => parts.push(Span::styled("no Plan yet", palette::muted_text())),
        Some(_) => {}
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
    /// The first Pulse row shown (reset whenever the Context of the Pulse changes).
    pulse_offset: usize,
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
    }

    /// Records the offset of `plan`, the Plan on show, once it differs from the one remembered (a Plan never scrolled
    /// is at its corner). Called after a draw, so the offset is clamped.
    fn remember(&mut self, plan: &PlanId) {
        if self.remembered.get(plan).copied().unwrap_or((0, 0)) != self.scroll {
            self.remembered.insert(plan.clone(), self.scroll);
        }
    }

    /// Draws Pulse: the rows inside the margin, then the message row (an error, ` no Plans yet`).
    fn draw_pulse(&mut self, f: &mut Frame, pulse: &Pulse) {
        let mut window = f.area();
        let mut parts: Vec<Span<'static>> =
            self.error.iter().map(|e| Span::styled(e.clone(), palette::error())).collect();
        if pulse.rows.is_empty() {
            parts.push(Span::styled(" no Plans yet", palette::muted_text()));
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
        self.pulse_offset = pulse::draw(f.buffer_mut(), self.body, pulse, self.pulse_offset);
    }

    fn draw(&mut self, f: &mut Frame, centre: Option<&Centre>) {
        let body = f.area();
        let mut window = body; // the chart's own margin keeps it off the pane's edges at scroll (0, 0)
        if let Some(text) = message(self.error.as_deref(), centre) {
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
    run_view(
        "plan",
        Capture::Mouse,
        || {
            let home = CpdHome::from_env()?;
            let config = Config::load(&home)?;
            let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
            let width = ratatui::crossterm::terminal::size()
                .map_err(|e| {
                    CpdError::new(ErrorCode::NotATerminal, format!("The size of the terminal of cpd tui view plan is unknown: {e}."))
                        .with("command", "tui view plan")
                })?
                .0;
            let snap = snapshot(&mut store, width, now_ms())?;
            let actions = Arc::new(Actions::new(home.clone(), &config, run_command));
            let poll = Duration::from_millis(u64::from(config.events.poll_ms));
            let follow = read::Follow::new(&mut store, &[], snap.cursor)?;
            // the reads after the first run on the reader thread, with a store of its own (R4)
            let reader = pace::Owned::new(home.clone(), u64::from(config.timeouts.store_busy_ms), follow);
            Ok((home, snap, poll, reader, actions))
        },
        |terminal, (home, mut snap, poll, reader, actions)| show(terminal, &home, &mut snap, poll, reader, &actions),
    )
}

/// Now, in Unix ms.
fn now_ms() -> i64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_millis() as i64)
}

/// Starts `ui` from the offsets file: an unreadable or invalid file starts empty and is replaced at the next write. Returns
/// the lines for the view's log (`run/tui/plan.log`).
/// A reset draws nothing (commands.md §17 "No notes").
fn begin(ui: &mut Ui, offsets: &mut Offsets, plan: Option<&PlanId>) -> Vec<String> {
    let loaded = offsets.load();
    ui.remembered = loaded.clone().unwrap_or_default();
    ui.switch(None, plan);
    loaded.err().map(|reason| format!("scroll positions reset: {reason}")).into_iter().collect()
}

/// Draws, follows Events and takes input until the terminal fails.
fn show(
    terminal: &mut ratatui::DefaultTerminal,
    home: &CpdHome,
    snap: &mut Snapshot,
    poll: Duration,
    reader: Arc<Mutex<pace::Owned<read::Follow>>>,
    actions: &Arc<Actions>,
) -> Result<(), ViewError> {
    let mut ui = Ui::default();
    let mut offsets = Offsets::at(home);
    let log = ViewLog::of(home, "plan");
    let mut trace = Trace::new(std::env::var("CPD_TUI_TRACE").ok().as_deref());
    let epoch = Instant::now();
    // a log that cannot be written is the view's error line: a lasting state
    let write = |ui: &mut Ui, line: &str| {
        if let Err(why) = log.append(line) {
            ui.error = Some(why);
        }
    };
    for line in begin(&mut ui, &mut offsets, snap.centre.as_ref().map(|c| &c.plan.plan)) {
        write(&mut ui, &line);
    }
    // a failed poll is made again at the next one, a failed snapshot at the next refresh (`stale`); both are shown
    // (runner::kept), the snapshot's error first
    let (mut snapshot_error, mut follow_error) = (None, None);
    let (mut stale, mut taken, mut fetched) = (false, Instant::now(), Instant::now());
    // the reads run on the reader thread (R4), one at a time: none is asked for while one is out
    let mut reads = Reads::<Read>::new();
    let mut reading = false;
    // the pane is written to only when something changed: a snapshot, an error, a spinner's frame, input
    let mut redraw = true;
    // the Worker pane's and the Lead's tmux calls run off this thread (tui-W7); one that fails opens the error dialog
    // from there ([`Actions::report`]), so the view only draws again
    let mut worker = Background::<()>::new();
    loop {
        while worker.finished().is_some() {
            redraw = true;
        }
        while let Some(done) = trace.read(|| reads.take()) {
            reading = false;
            let shown = ui.error.clone();
            match done {
                Read::Poll(polled) => {
                    if let Some(changes) = kept(polled, &mut follow_error) {
                        stale |= changes.pulls
                            || changes.events.iter().any(|e| concerns(e, snap.centre.as_ref(), snap.pulse.as_ref()));
                    }
                    ui.error = snapshot_error.clone().or_else(|| follow_error.clone());
                    let due = if snap.pulse.is_some() { pulse_due(stale, taken.elapsed()) } else { due(stale, taken.elapsed()) };
                    if due {
                        let width = terminal.size().map_or(0, |s| s.width);
                        let (timed, shared) = (trace.is_on(), Arc::clone(&reader));
                        stale = false; // what comes while the snapshot is read (a resize) is seen by the next
                        reading = true;
                        reads.request(move || {
                            Read::Snapshot(pace::Owned::read(&shared, |store, _| snapshot_timed(store, width, now_ms(), timed)))
                        });
                        taken = Instant::now();
                    }
                }
                Read::Snapshot(taken_now) => {
                    let before = snap.centre.as_ref().map(|c| (c.slot.session.clone(), c.plan.plan.clone()));
                    let before_pulse = snap.pulse.as_ref().map(|p| p.context.clone());
                    let next = kept(taken_now.map(|s| {
                        trace.add_layout(s.layout);
                        s
                    }), &mut snapshot_error);
                    stale |= snapshot_error.is_some();
                    ui.error = snapshot_error.clone().or_else(|| follow_error.clone());
                    if let Some(next) = next {
                        redraw = true;
                        *snap = next;
                        let after = snap.centre.as_ref().map(|c| (c.slot.session.clone(), c.plan.plan.clone()));
                        if before_pulse != snap.pulse.as_ref().map(|p| p.context.clone()) {
                            ui.pulse_offset = 0;
                        }
                        if before.as_ref().map(|b| &b.0) != after.as_ref().map(|a| &a.0) {
                            ui.switch(before.as_ref().map(|b| &b.1), after.as_ref().map(|a| &a.1));
                            if let Err(e) = offsets.save(&ui.remembered, true) {
                                ui.error = Some(e);
                            }
                        }
                    }
                    taken = Instant::now();
                }
            }
            redraw |= ui.error != shown;
        }
        let frame = palette::frame(SystemTime::now());
        if let Some(c) = snap.centre.as_mut() {
            redraw |= c.turn_traced(frame, &mut trace);
        }
        // the scroll offsets file follows the chart's offset (a write only when they changed, at most every SAVE_EVERY)
        if let Some(c) = snap.centre.as_ref() {
            ui.remember(&c.plan.plan);
            if let Err(e) = offsets.save(&ui.remembered, false) {
                redraw |= ui.error.as_deref() != Some(e.as_str());
                ui.error = Some(e);
            }
        }
        if redraw {
            let (drawn, written) = trace.draw(epoch, &log, frame, || {
                terminal
                    .draw(|f| match snap.pulse.as_ref() {
                        Some(pulse) => ui.draw_pulse(f, pulse),
                        None => ui.draw(f, snap.centre.as_ref()),
                    })
                    .map(drop)
            });
            drawn?;
            if let Err(why) = written {
                ui.error = Some(why);
            }
            redraw = false;
        }
        // wait for the next frame boundary of the clock (R3)
        if !event::poll(pace::wait_now())? {
            // idle: ask for Events, every `poll`
            if fetched.elapsed() >= poll && !reading {
                fetched = Instant::now();
                let shared = Arc::clone(&reader);
                reading = true;
                reads.request(move || Read::Poll(pace::Owned::read(&shared, |store, follow| follow.poll_changes(store))));
            }
            continue;
        }
        redraw = true; // a key, a click, a resize
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
            let actions = Arc::clone(actions);
            worker.submit(move || {
                if let Err(e) = actions.open_lead(&lead) {
                    actions.report("Open Lead", &e);
                }
            });
        }
        let Some(centre) = snap.centre.as_ref() else { continue };
        if let Some(key) = unselect {
            let actions = Arc::clone(actions);
            worker.submit(move || {
                if let Err(e) = actions.close_worker(&key) {
                    actions.report("Close Worker", &e);
                }
            });
        }
        if let Some(key) = open {
            match attempt_of(centre, &key) {
                attempt => {
                    let (actions, plan) = (Arc::clone(actions), centre.plan.plan.clone());
                    worker.submit(move || {
                        if let Err(e) = actions.toggle_worker(&plan, &key, attempt.as_ref()) {
                            actions.report("Open Worker", &e);
                        }
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod margin_tests;

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    use super::*;
    use crate::read::{JobNode, JobStatus, PrNode, PrStatus};
    use cpd_core::config::Tier;
    use cpd_core::ids::{ContextId, JobKey, PrKey, SessionId};


    #[test]
    fn the_snapshot_is_retaken_after_the_coalescing_when_stale_and_never_else() {
        let ms = Duration::from_millis;
        assert!(!due(true, ms(499)) && due(true, ms(500)), "an Event that concerns it: coalesced at 500 ms");
        assert!(!due(false, ms(2000)), "nothing changed: no snapshot (tui-F3)");
        assert!(!due(false, Duration::from_secs(60)));
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
            waiting_on: Vec::new(),
            superseded_by: None,
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
            waiting_on: Vec::new(),
            attempts: Vec::new(),
        };
        let plan = PlanBoxes {
            plan: PlanId::parse(plan_id).unwrap(),
            context: ContextId::parse("ctx").unwrap(),
            prs: vec![node],
            jobs: (0..jobs).map(job).collect(),
            needs_user: read::NeedsUser::default(),
            events_cursor: 0,
        };
        let boxes = boxes::layout(&plan.prs, &plan.jobs, &plan.needs_user, 0).with_margin(CHART_MARGIN);
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
    fn the_chart_turns_with_the_clock_only_while_a_job_runs() {
        let mut queued = scroll_centre("plan-a", 3);
        assert!(!queued.spins());
        assert!(!queued.turn(1), "nothing spins: nothing to draw again");
        assert_eq!(queued.frame, 0);

        let mut running = scroll_centre("plan-a", 3);
        running.plan.jobs[1].status = JobStatus::Running;
        assert!(running.spins());
        assert!(running.turn(1), "a new frame of the spinner");
        assert!(!running.turn(1), "the same frame: nothing to draw again");
        assert_eq!(running.frame, 1);
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

    fn pulse_of(rows: &[&str]) -> Pulse {
        let bucket = |count| read::Bucket { count, kind: Some(read::Kind::Other), job_merged: false, pr_merged: false, worker_ran: false };
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
        terminal.draw(|f| ui.draw_pulse(f, pulse)).unwrap();
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
        assert!(rows[1].starts_with("  aa  "), "{:?}", rows[1]);
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
        assert!(rows[1].starts_with("  dd  "), "{:?}", rows[1]);
        assert_eq!(lead(&ui, 1).as_deref(), Some("lead-dd"));
        assert_eq!(lead(&ui, 3).as_deref(), Some("lead-ff"));
    }

    #[test]
    fn a_context_without_plans_says_so_dim_in_the_message_row() {
        let mut ui = Ui::default();
        let (buf, rows) = pulse_screen(&mut ui, &pulse_of(&[]), 5);
        assert_eq!(rows[4], "   no Plans yet");
        assert!(crate::palette::check::is_muted(&buf[(3, 4)]));
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
        assert!(pulse_due(true, ms(500)) && !pulse_due(true, ms(499)));
        assert!(!pulse_due(false, Duration::from_secs(59)) && pulse_due(false, Duration::from_secs(60)));
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
        let snap = snapshot(&mut store, 125, now).unwrap();
        let pulse = snap.pulse.expect("the Manager is in the slot");
        assert!(snap.centre.is_none());
        assert_eq!(pulse.context.as_str(), "ctx");
        assert_eq!(pulse.rows.iter().map(|r| r.plan.as_str()).collect::<Vec<_>>(), ["p1"]);
        assert_eq!(pulse.rows[0].buckets.len(), 125 - 4 - 13, "as wide as the pane allows for ids of any width");

        fixture(&mut store, &moved("lead"));
        let snap = snapshot(&mut store, 125, now).unwrap();
        assert!(snap.pulse.is_none());
        assert_eq!(snap.centre.expect("the Lead is in the slot").plan.plan.as_str(), "p1");
    }

    /// The pane `ui` draws for `centre`, as the buffer.
    fn frame_of(ui: &mut Ui, centre: &Centre) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(PANE.0, PANE.1)).unwrap();
        terminal.draw(|f| ui.draw(f, Some(centre))).unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn a_scroll_file_that_is_reset_draws_nothing_and_logs() {
        // commands.md §17 "No notes": no `scroll positions reset: …` line; the reason goes to run/tui/plan.log
        let centre = scroll_centre("plan-a", 3);
        let plan_id = plan("plan-a");
        let clean = tempfile::tempdir().unwrap();
        let mut plain = Ui::default();
        begin(&mut plain, &mut offsets_in(&clean), Some(&plan_id));
        let plain = frame_of(&mut plain, &centre);

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run/tui/plan-scroll.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{not json").unwrap();
        let mut ui = Ui::default();
        let lines = begin(&mut ui, &mut offsets_in(&dir), Some(&plan_id));
        assert_eq!(frame_of(&mut ui, &centre), plain, "the frame is the one of a start with no file");
        assert!(
            lines.iter().any(|l| l.starts_with("scroll positions reset: ") && l.contains("plan-scroll.json")),
            "logged: {lines:?}"
        );
    }
}
