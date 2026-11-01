//! The `plan` view (commands.md §13.2, §17): the centre-top pane of the cockpit, ported from the cockpit of branch
//! `cpd/cpd-ui` (`tui/src/flow.rs`, `pr.rs`, `workers.rs`) onto the read model of [`crate::read`].
//!
//! It shows the Plan of the Lead in the chat slot as one box per PR ([`boxes`], R34 A15), top to bottom by the PRs'
//! needs, each holding the left-to-right flow chart of its Jobs ([`flow`]). The boxes start at the top of the pane:
//! there is no top row and there are no tabs (switching Leads is the tree's).
//!
//! Data: one snapshot of the store ([`snapshot`]), then the Events, followed in this process from the snapshot's
//! cursor (one `events` query per read, no subprocess); a snapshot is retaken at most every frame
//! one read per frame after a commit signal (`crate::pump`) and an Event that concerns the view.
//! The User's keys and clicks run commands ([`actions`]); nothing else starts a process.
//!
//! The Plans' scroll offsets survive a restart of the view, in `run/tui/plan-scroll.json` ([`Offsets`], hs-d254): the
//! one file a view writes, its own state.
//!
//! Input: the wheel scrolls; clicks open a Lead tab, a Pulse row or a PR link; a Job pill is neither clickable nor selectable. There is no `q`: a layout pane that
//! dies is started again by the `pane-died` hook (contract §6.1), so the view never exits on a key.

mod actions;
pub(crate) mod boxes;
pub(crate) mod flow;
pub(crate) mod pulse;

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::crossterm::event::{Event as Input, MouseButton, MouseEventKind};
use ratatui::layout::{Constraint, Layout, Margin, Position, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use self::actions::Actions;
use self::boxes::Boxes;
use crate::box_widget::spacing;
use crate::dialog::scroll::Scroll;
use crate::palette;
use super::background::Background;
use super::viewlog::ViewLog;
use crate::trace::Trace;
use super::read::{self, ChatSlot, PlanBoxes, Pulse};
use super::pace::{self, Reads};
use crate::pump::{Feed, Schedule, Signal, SAFETY};
use super::runner::{ViewError, kept};
use super::view_ports::{Ctx, Input as ViewInput, Tty, frame_now, run_view_with, wall_ms};
use super::{Capture, RunCommand, run_view_on};
use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::{Event, highest_id};
use cpd_core::home::CpdHome;
use cpd_core::ids::{PlanId, SessionId};
use cpd_core::output::Output;
use cpd_core::ports::Ports;
use cpd_core::store::Store;

/// While Pulse shows the snapshot is retaken at least this often, Event or not: the window moves and the ages tick,
/// and a running Attempt is not an Event.
const PULSE_REFRESH: Duration = Duration::from_secs(60);

/// The blank space around the chart's content, inside the canvas the view scrolls over: at scroll (0, 0) nothing of the
/// chart touches the pane's edges, and scrolled the chart reaches them.
const MARGIN: Margin = Margin { horizontal: 2, vertical: 1 };
/// The blank space around the chart's canvas of boxes: the container's one column of box margin
/// ([`spacing::MARGIN_X`]) each side and one row; Pulse and the message row keep `MARGIN`.
pub(crate) const CHART_MARGIN: Margin = Margin::new(spacing::MARGIN_X, 1);

/// Whether a spinner is on screen for `jobs` of a Plan: a running Job's pill (hs-d586: also a failed one being worked on,
/// `needs_user.worked_on`).
pub(crate) fn spins(jobs: &[read::JobNode], needs_user: &read::NeedsUser) -> bool {
    jobs.iter().any(|j| matches!(flow::Live::of(j, needs_user.worked_on.contains(&j.key)), flow::Live::Running | flow::Live::FailedWorked))
}

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
        false
            || spins(&self.plan.jobs, &self.plan.needs_user) // a running Job's pill icon, or a failed one's while the Lead works
            || self.plan.prs.iter().any(|p| p.lead_dot.is_some()) // a PR box title's Lead dot
    }

    /// Turn the spinners to `frame`, from the layout already done (R2: no layout at a frame change), when a spinner is on
    /// screen and the frame is not the one drawn. Whether it did.
    #[cfg(test)]
    pub(crate) fn turn(&mut self, frame: usize) -> bool {
        self.turn_traced(frame, &mut Trace::new(None))
    }

    /// [`Centre::turn`]; it lays nothing out, so the trace's `layout_us` stays 0. Only the glyph cells change: every frame is one cell wide
    /// and the plan view draws a `palette::SPINNER` glyph nowhere but at a spinner, so the cells holding the drawn
    /// frame's glyph get the new one's.
    fn turn_traced(&mut self, frame: usize, _trace: &mut Trace) -> bool {
        let turned = frame != self.frame && self.spins();
        if turned {
            let (from, to) = (palette::SPINNER[self.frame % palette::SPINNER.len()], palette::SPINNER[frame % palette::SPINNER.len()]);
            for cell in &mut self.boxes.chart.buf.content {
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

/// The clocks the screen of `snap` needs, `taken` being when the snapshot was taken: `frame` is true when `snap.centre` is `Some`
/// and its `spins()` holds, else false; `second` is false (the plan view shows no per-second clock: Pulse's ages are in minutes,
/// refreshed by `at`); `at` is `Some(taken + PULSE_REFRESH)` when `snap.pulse` is `Some`, else `None`.
fn needs(snap: &Snapshot, taken: Instant) -> crate::pump::Needs {
    crate::pump::Needs {
        frame: snap.centre.as_ref().is_some_and(Centre::spins),
        second: false,
        at: snap.pulse.as_ref().map(|_| taken + PULSE_REFRESH),
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
    snapshot_timed(&Ports::current(), store, width, now_ms, false)
}

/// [`snapshot`]'s chart, with the spinner frame and the time read on the clock of `ports`.
#[cfg(test)]
pub(crate) fn snapshot_with(ports: &Ports, store: &mut Store, width: u16) -> Result<Option<Centre>, CpdError> {
    Ok(snapshot_timed(ports, store, width, now_ms(ports), false)?.centre)
}

/// [`snapshot`], timing its layout when `timed`; the frame and the timing are read on the clock of `ports`.
fn snapshot_timed(ports: &Ports, store: &mut Store, width: u16, now_ms: i64, timed: bool) -> Result<Snapshot, CpdError> {
    // Read first: whatever the later reads see is at least this new, so following from here loses nothing.
    let cursor = store.read(highest_id)?;
    if let Some(manager) = read::manager_slot(store)? {
        // Read for the widest the ids allow; the drawing keeps the last buckets that fit.
        let buckets = pulse::buckets(usize::from(width).saturating_sub(2 * usize::from(MARGIN.horizontal)), 0);
        let pulse = read::pulse(store, &manager.context, now_ms, buckets)?;
        return Ok(Snapshot { cursor, centre: None, pulse: Some(pulse), layout: Duration::ZERO });
    }
    let Some(slot) = read::chat_slot(store)? else { return Ok(Snapshot { cursor, centre: None, pulse: None, layout: Duration::ZERO }) };
    let plan = read::plan_boxes_at(store, &slot.plan, now_ms)?;
    let frame = frame_now(ports);
    let began = timed.then(|| ports.clock.now());
    let boxes = boxes::layout(&plan.prs, &plan.jobs, &plan.needs_user, frame).with_margin(CHART_MARGIN);
    let layout = began.map_or(Duration::ZERO, |b| ports.clock.now().saturating_duration_since(b));
    Ok(Snapshot { cursor, centre: Some(Centre { slot, plan, boxes, frame }), pulse: None, layout })
}

/// What the reader thread brings back from one read: the poll's outcome and, when the poll found a change that concerns the
/// view (or the read was forced, or the last snapshot failed), the snapshot.
struct Read {
    poll: Result<(), CpdError>,
    snapshot: Option<Result<Snapshot, CpdError>>,
}

/// The ids of what the pane shows, copied small for the reader thread ([`concerns`] without the `Snapshot`).
#[derive(Clone, Default)]
struct Watching {
    plan: Option<String>,
    context: Option<String>,
    pulse: Option<String>,
}

impl Watching {
    fn of(centre: Option<&Centre>, pulse: Option<&Pulse>) -> Watching {
        Watching {
            plan: centre.map(|c| c.plan.plan.as_str().to_owned()),
            context: centre.map(|c| c.plan.context.as_str().to_owned()),
            pulse: pulse.map(|p| p.context.as_str().to_owned()),
        }
    }

    fn concerns(&self, event: &Event) -> bool {
        let mine = |column: &Option<String>, id: &Option<String>| id.is_some() && column == id;
        event.kind == "session-moved"
            || mine(&event.plan, &self.plan)
            || mine(&event.context, &self.context)
            || mine(&event.context, &self.pulse)
    }
}

/// Whether an Event changes what the pane shows: the Plan's or the Context's, or a move of a Lead (the chat slot may
/// change); while Pulse shows, any Event of its Context.
#[cfg(test)]
fn concerns(event: &Event, centre: Option<&Centre>, pulse: Option<&Pulse>) -> bool {
    Watching::of(centre, pulse).concerns(event)
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
    /// The window's top-left in the chart.
    scroll: (u16, u16),
    /// The scroll offset each Plan was left at (hs-d197), read from [`Offsets`] at the view's start. Not clamped: the
    /// chart may have changed size by the time it comes back, and a draw clamps.
    remembered: HashMap<PlanId, (u16, u16)>,
    /// Pulse's scroll (reset whenever the Context of the Pulse changes).
    pulse_scroll: Scroll,
    error: Option<String>,
    /// The area clicks and the wheel use: the chart's window, or the Pulse rows inside the margin.
    body: Rect,
    /// The links of the last drawn frame (pro-d60): what a click on a PR box title reads.
    links: crate::link::Links,
    /// Where the canvas the `links` were drawn on has its corner in the chart (`Boxes::links_origin`).
    links_origin: (u16, u16),
}

impl Ui {
    /// A mouse event on the frame last drawn: a left press (any modifiers) on a drawn link opens its url through
    /// `ports.runner` and answers `Ok(true)`, changing nothing else; any other event answers `Ok(false)` and changes
    /// nothing. A failed `open` is the `Err`.
    fn click(&mut self, ports: &Ports, m: ratatui::crossterm::event::MouseEvent) -> Result<bool, CpdError> {
        let cell = Position::new(m.column, m.row);
        if m.kind != MouseEventKind::Down(MouseButton::Left) || !self.body.contains(cell) {
            return Ok(false);
        }
        let at = |screen: u16, body: u16, scroll: u16, origin: u16| (screen - body + scroll).checked_sub(origin);
        let (Some(x), Some(y)) = (
            at(m.column, self.body.x, self.scroll.0, self.links_origin.0),
            at(m.row, self.body.y, self.scroll.1, self.links_origin.1),
        ) else {
            return Ok(false);
        };
        match self.links.at(Position::new(x, y)) {
            Some(url) => crate::link::open_url(&*ports.runner, url).map(|()| true),
            None => Ok(false),
        }
    }

    /// A different Lead is in the slot, `from` its Plan before and `to` its Plan now: the chart
    /// is at the offset `to` was left at, or at its corner. Leads of one Plan share the Plan's offset.
    fn switch(&mut self, from: Option<&PlanId>, to: Option<&PlanId>) {
        if let Some(from) = from {
            self.remembered.insert(from.clone(), self.scroll);
        }
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
        pulse::draw(f.buffer_mut(), self.body, pulse, &mut self.pulse_scroll);
    }

    /// Draws what `snap` holds: Pulse, or the chart of the Plan in the chat slot.
    fn draw_snapshot(&mut self, f: &mut Frame, snap: &Snapshot) {
        match snap.pulse.as_ref() {
            Some(pulse) => self.draw_pulse(f, pulse),
            None => self.draw(f, snap.centre.as_ref()),
        }
    }

    fn draw(&mut self, f: &mut Frame, centre: Option<&Centre>) {
        let body = f.area();
        let mut window = body; // the chart's own margin keeps it off the pane's edges at scroll (0, 0)
        if let Some(text) = message(self.error.as_deref(), centre) {
            let [chart_area, message_area] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(window);
            // the pane's last row, at column `MARGIN.horizontal` (2)
            f.render_widget(Paragraph::new(text), message_area.inner(Margin { horizontal: MARGIN.horizontal, vertical: 0 }));
            window = chart_area;
        }
        self.body = window;
        self.links = crate::link::Links::default();
        let Some(c) = centre else { return };
        self.scroll = c.boxes.chart.blit(f.buffer_mut(), window, self.scroll);
        (self.links, self.links_origin) = (c.boxes.links.clone(), c.boxes.links_origin);
    }
}

/// The Plans' scroll offsets in `$CPD_HOME/run/tui/plan-scroll.json` (commands.md §13.2, hs-d254): a JSON object
/// `{"<plan id>": [x, y]}`. The view's own state, not Ledger state: the one file a view writes.
pub(crate) struct Offsets {
    path: PathBuf,
    /// What the file holds, as far as this view knows; a write is due when the offsets differ from it.
    saved: HashMap<PlanId, (u16, u16)>,
    /// When the file was last written, or tried.
    written: Option<Instant>,
}

/// A write is due at most this often (an offset that changes more often waits).
const SAVE_EVERY: Duration = Duration::from_secs(1);

impl Offsets {
    pub(crate) fn at(home: &CpdHome) -> Offsets {
        // Not in `CpdHome` (contract §1.4): the layout's one source of truth lists this file there.
        let path = home.run_dir().join("tui").join("plan-scroll.json");
        Offsets { path, saved: HashMap::new(), written: None }
    }

    /// The offsets in the file: none when it is missing; `Err` with the reason when it cannot be read or is not
    /// `{"<plan id>": [x, y]}`.
    pub(crate) fn load(&mut self) -> Result<HashMap<PlanId, (u16, u16)>, String> {
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

    /// When a write that is due but held back by [`SAVE_EVERY`] can be made, if one is waiting.
    fn due_at(&self, offsets: &HashMap<PlanId, (u16, u16)>) -> Option<Instant> {
        (*offsets != self.saved).then(|| self.written.map(|at| at + SAVE_EVERY)).flatten()
    }

    /// Writes `offsets` when they differ from the file and, unless `now` (the Plan changed), a second has passed
    /// since the last write. A failure is returned and leaves the offsets unsaved, so the next call retries.
    #[cfg(test)]
    fn save(&mut self, offsets: &HashMap<PlanId, (u16, u16)>, now: bool) -> Result<(), String> {
        self.save_with(&Ports::real(), offsets, now)
    }

    /// [`Offsets::save`] with the second counted on the clock of `ports`.
    pub(crate) fn save_with(&mut self, ports: &Ports, offsets: &HashMap<PlanId, (u16, u16)>, now: bool) -> Result<(), String> {
        let at_now = ports.clock.now();
        if *offsets == self.saved || (!now && self.written.is_some_and(|at| at_now.saturating_duration_since(at) < SAVE_EVERY)) {
            return Ok(());
        }
        self.written = Some(at_now);
        self.write(offsets).map_err(|e| format!("scroll positions not saved: {}: {e}", self.path.display()))?;
        self.saved.clone_from(offsets);
        Ok(())
    }

    /// Replaces the file through [`cpd_core::atomic::write_atomic`].
    fn write(&self, offsets: &HashMap<PlanId, (u16, u16)>) -> io::Result<()> {
        let sorted: BTreeMap<&str, (u16, u16)> = offsets.iter().map(|(plan, at)| (plan.as_str(), *at)).collect();
        let text = serde_json::to_vec(&sorted).map_err(io::Error::other)?;
        cpd_core::atomic::write_atomic(&self.path, text)
    }
}

/// The view's first frame for `store` at `now_ms` (Unix ms), drawn on `f` as `run` draws it: one snapshot, at the corner of its
/// chart. Tests draw it on a `TestBackend` instead of running the view in a terminal.
pub fn draw_first_frame(store: &mut Store, f: &mut Frame, now_ms: i64) -> Result<(), CpdError> {
    let snap = snapshot(store, f.area().width, now_ms)?;
    Ui::default().draw_snapshot(f, &snap);
    Ok(())
}

pub(super) fn run(ports: &Ports, run_command: RunCommand) -> Result<Output, CpdError> {
    run_view_on(
        ports,
        "plan",
        Capture::Mouse,
        || {
            prepare(ports, CpdHome::from_env()?, run_command, || {
                Ok(ratatui::crossterm::terminal::size()
                    .map_err(|e| {
                        CpdError::new(ErrorCode::NotATerminal, format!("The size of the terminal of cpd tui view plan is unknown: {e}."))
                            .with("command", "tui view plan")
                    })?
                    .0)
            })
        },
        |ctx, prepared| {
            let feed = Feed::real(prepared.home.root());
            show_with(ctx, feed, prepared)
        },
    )
}

/// What the view starts from: the home, the first snapshot, the follow's reader and the actions of the keys and clicks.
pub struct Prepared {
    home: CpdHome,
    snap: Snapshot,
    poll: Duration,
    reader: Arc<Mutex<pace::Owned<read::Follow>>>,
    actions: Arc<Actions>,
}

/// The first snapshot of `home` for a pane `width` columns wide (`width` is asked after the store opened, as `run` does), and
/// the rest `show_with` starts from.
pub fn prepare(
    ports: &Ports,
    home: CpdHome,
    run_command: RunCommand,
    width: impl FnOnce() -> Result<u16, CpdError>,
) -> Result<Prepared, CpdError> {
    let config = Config::load(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    let snap = snapshot_timed(ports, &mut store, width()?, now_ms(ports), false)?;
    let actions = Arc::new(Actions::new(home.clone(), &config, run_command));
    let poll = Duration::from_millis(u64::from(config.events.poll_ms));
    let follow = read::Follow::new(&mut store, &[], snap.cursor)?;
    // the reads after the first run on the reader thread, with a store of its own (R4)
    let reader = pace::Owned::new(home.clone(), u64::from(config.timeouts.store_busy_ms), follow);
    Ok(Prepared { home, snap, poll, reader, actions })
}

/// [`run`] on a terminal, its facts and its input that the caller gives (a test's `TestBackend`, scripted input and a
/// [`Tty`] that says whether there is a terminal): the same refusals before anything is drawn. The loop is driven by `input`
/// (nothing is read on a thread, nothing waits in real time) and watches no directory: it polls.
pub fn run_on<B: Backend, W: io::Write>(
    ports: &Ports,
    run_command: RunCommand,
    tty: &mut dyn Tty,
    out: &mut W,
    terminal: Terminal<B>,
    input: &mut dyn ViewInput,
) -> Result<Output, CpdError> {
    let width = terminal.size().map(|size| size.width);
    run_view_with(
        ports,
        tty,
        out,
        terminal,
        input,
        "plan",
        Capture::Mouse,
        || {
            prepare(ports, CpdHome::from_env()?, run_command, || {
                width.map_err(|e| CpdError::new(ErrorCode::NotATerminal, format!("The size of the terminal of cpd tui view plan is unknown: {e}.")))
            })
        },
        |ctx, prepared| show_with(ctx, Feed::driven(), prepared),
    )
}

/// Now, in Unix ms, on the clock of `ports`.
fn now_ms(ports: &Ports) -> i64 {
    wall_ms(ports) as i64
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

/// Draws, follows Events and takes input until the terminal fails. The loop's one wait is `feed`'s pump: a test's is
/// scripted (signals, the `ManualClock` of `ctx.ports`, reads and Worker actions inline, no watcher) and `ctx.input` is not
/// read; the end of a script is `Signal::InputFailed`.
pub fn show_with<B: Backend>(ctx: &mut Ctx<B>, feed: Feed, prepared: Prepared) -> Result<(), ViewError> {
    let Prepared { home, mut snap, poll, reader, actions } = prepared;
    let (ports, home, snap, actions) = (ctx.ports, &home, &mut snap, &actions);
    let Feed { pump, watch } = feed;
    let mut ui = Ui::default();
    let mut offsets = Offsets::at(home);
    let log = ViewLog::of(home, "plan");
    let mut trace = Trace::new(std::env::var("CPD_TUI_TRACE").ok().as_deref());
    let epoch = ports.clock.now();
    // a log that cannot be written is the view's error line: a lasting state
    let write = |ui: &mut Ui, line: &str| {
        if let Err(why) = log.append_with(ports, line) {
            ui.error = Some(why);
        }
    };
    for line in begin(&mut ui, &mut offsets, snap.centre.as_ref().map(|c| &c.plan.plan)) {
        write(&mut ui, &line);
    }
    // a failed poll is made again at the next read, a failed snapshot likewise; both are shown (runner::kept), the
    // snapshot's error first, then the watcher's (lasting)
    let (mut snapshot_error, mut follow_error) = (None, None);
    let (watch, watch_error) = match watch {
        Ok(w) => (w, None),
        Err(e) => (None, Some(format!("no commit watcher: {}; polling every {} ms", e.message(), poll.as_millis()))),
    };
    let _watch = watch; // lives as long as the loop
    let mut schedule = Schedule::new(ports.clock.now(), if watch_error.is_some() || pump.is_driven() { poll } else { SAFETY });
    if watch_error.is_some() {
        ui.error = watch_error.clone();
    }
    let mut taken = ports.clock.now();
    // the reads run on the reader thread (R4), one at a time: `schedule` asks for none while one is out
    let mut reads = Reads::<Read>::on(&pump);
    // the pane is written to only when something changed: a snapshot, an error, a spinner's frame, input
    let mut redraw = true;
    // the Lead's tmux calls run off this thread (tui-W7); one that fails opens the error dialog
    // from there ([`Actions::report`]), so the view only draws again
    let mut background = Background::<()>::on(&pump);
    loop {
        while background.finished().is_some() {
            redraw = true;
        }
        while let Some(done) = trace.read(|| reads.take()) {
            schedule.done();
            let shown = ui.error.clone();
            kept(done.poll, &mut follow_error);
            if let Some(result) = done.snapshot {
                ctx.snapshot_read();
                let before = snap.centre.as_ref().map(|c| (c.slot.session.clone(), c.plan.plan.clone()));
                let before_pulse = snap.pulse.as_ref().map(|p| p.context.clone());
                let next = kept(result.inspect(|s| {
                    trace.add_layout(s.layout);
                }), &mut snapshot_error);
                if let Some(next) = next {
                    redraw = true;
                    *snap = next;
                    let after = snap.centre.as_ref().map(|c| (c.slot.session.clone(), c.plan.plan.clone()));
                    if before_pulse != snap.pulse.as_ref().map(|p| p.context.clone()) {
                        ui.pulse_scroll = Scroll::default();
                    }
                    if before.as_ref().map(|b| &b.0) != after.as_ref().map(|a| &a.0) {
                        ui.switch(before.as_ref().map(|b| &b.1), after.as_ref().map(|a| &a.1));
                        if let Err(e) = offsets.save_with(ports, &ui.remembered, true) {
                            ui.error = Some(e);
                        }
                    }
                }
                taken = ports.clock.now();
            }
            ui.error = snapshot_error.clone().or_else(|| follow_error.clone()).or_else(|| watch_error.clone());
            redraw |= ui.error != shown;
        }
        if let Some(kind) = schedule.take(ports.clock.now()) {
            let width = ctx.terminal.size().map_or(0, |s| s.width);
            let (timed, shared, ports) = (trace.is_on(), Arc::clone(&reader), ports.clone());
            let watching = Watching::of(snap.centre.as_ref(), snap.pulse.as_ref());
            let retry = snapshot_error.is_some();
            reads.request(move || {
                let mut changed = false;
                let polled = pace::Owned::read(&shared, |store, follow| follow.poll_changes(store));
                let poll = polled.map(|changes| {
                    changed = changes.pulls || changes.events.iter().any(|e| watching.concerns(e));
                });
                let snapshot = (changed || kind.force || retry)
                    .then(|| pace::Owned::read(&shared, |store, _| snapshot_timed(&ports, store, width, now_ms(&ports), timed)));
                Read { poll, snapshot }
            });
        }
        let frame = frame_now(ports);
        if let Some(c) = snap.centre.as_mut() {
            redraw |= c.turn_traced(frame, &mut trace);
        }
        // the scroll offsets file follows the chart's offset (a write only when they changed, at most every SAVE_EVERY)
        if let Some(c) = snap.centre.as_ref() {
            ui.remember(&c.plan.plan);
            if let Err(e) = offsets.save_with(ports, &ui.remembered, false) {
                redraw |= ui.error.as_deref() != Some(e.as_str());
                ui.error = Some(e);
            }
        }
        if redraw {
            let (drawn, written) = trace.draw(epoch, &log, frame, || {
                ctx.draw(|f| ui.draw_snapshot(f, snap))
            });
            drawn?;
            if let Err(why) = written {
                ui.error = Some(why);
            }
            redraw = false;
        }
        // block until a signal, or the next clock the screen needs (R3)
        let now = ports.clock.now();
        let wall_ms = wall_ms(ports);
        // a scroll offset not yet saved (held back by SAVE_EVERY) wakes the loop when it may be written
        let save_at = offsets.due_at(&ui.remembered);
        let until = crate::pump::wake(now, wall_ms, &needs(snap, taken), [schedule.deadline(now), save_at].into_iter().flatten().min());
        let signal = if pump.is_driven() {
            pump.wait_driven(ports, &mut *ctx.input, until, schedule.is_out() || background.busy())
        } else {
            pump.wait(until)
        };
        let input = match signal {
            Signal::Input(input) => input,
            Signal::InputFailed(e) => return Err(e.into()),
            Signal::Commit => {
                schedule.commit();
                continue;
            }
            Signal::Read => continue,
            Signal::Due => {
                if snap.pulse.is_some() && ports.clock.now() >= taken + PULSE_REFRESH {
                    schedule.force(); // the window moves and the ages tick: not an Event
                    taken = ports.clock.now();
                }
                continue;
            }
        };
        redraw = true; // a key, a click, a resize
        let mut lead = None::<SessionId>; // the Lead a click on a Pulse row opens
        match input {
            Input::Resize(..) => schedule.force(), // the bucket count changes
            Input::Mouse(m) if snap.pulse.is_some() => {
                let Some(pulse) = snap.pulse.as_ref() else { continue };
                match m.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        lead = pulse::row_at(ui.body, ui.pulse_scroll.offset(), pulse.rows.len(), m.column, m.row)
                            .map(|i| pulse.rows[i].lead.clone());
                    }
                    MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                        ui.pulse_scroll.mouse(m);
                    }
                    _ => {}
                }
            }
            Input::Mouse(m) => match ui.click(ports, m) {
                // a PR box title's link: the pull request is opened and nothing else changes
                Ok(true) => {}
                Err(e) => {
                    let actions = Arc::clone(actions);
                    background.submit(move || actions.report("Open Pull Request", &e));
                }
                Ok(false) => match m.kind {
                    MouseEventKind::ScrollUp => ui.scroll.1 = ui.scroll.1.saturating_sub(3),
                    MouseEventKind::ScrollDown => ui.scroll.1 = ui.scroll.1.saturating_add(3), // clamped when drawn
                    MouseEventKind::ScrollLeft => ui.scroll.0 = ui.scroll.0.saturating_sub(4),
                    MouseEventKind::ScrollRight => ui.scroll.0 = ui.scroll.0.saturating_add(4),
                    _ => {}
                },
            },
            _ => {}
        }
        if let Some(lead) = lead {
            let actions = Arc::clone(actions);
            background.submit(move || {
                if let Err(e) = actions.open_lead(&lead) {
                    actions.report("Open Lead", &e);
                }
            });
        }
    }
}

/// The Plan view's fixtures for the tests of its boxes: a PR `key` (repo `<key>-repo`, base `main`, `dispatch`, no needs).
#[cfg(test)]
fn sample_pr(key: &str) -> read::PrNode {
    read::PrNode {
        key: cpd_core::ids::PrKey::parse(key).unwrap(),
        repo_name: format!("{key}-repo"),
        base: "main".into(),
        status: read::PrStatus::Dispatch,
        needs: Vec::new(),
        pull: None,
        waiting_on: Vec::new(),
        superseded_by: None,
        elapsed_seconds: None,
        cost_usd: None,
        lead_dot: None,
    }
}

/// A queued Job `key` of PR `pr`, of Tier S1, with no needs.
#[cfg(test)]
fn sample_job(pr: &str, key: &str) -> read::JobNode {
    read::JobNode {
        kind: "work".into(),
        key: cpd_core::ids::JobKey::parse(key).unwrap(),
        pr: Some(cpd_core::ids::PrKey::parse(pr).unwrap()),
        seq: 0,
        title: String::new(),
        status: read::JobStatus::Queued,
        tier: Some(cpd_core::config::Tier::parse("S1").unwrap()),
        needs: Vec::new(),
        appended: false,
        retry_tier: None,
        shown_tier: Some(cpd_core::config::Tier::parse("S1").unwrap()),
        failed_cause: None,
        cancel_reason: None,
        merged_sha: None,
        waiting_on: Vec::new(),
        attempts: Vec::new(),
    }
}

/// What the view reads for a Plan of `prs` and `jobs`, laid out as [`snapshot`] does (spinner frame 0).
#[cfg(test)]
fn sample_centre(prs: Vec<read::PrNode>, jobs: Vec<read::JobNode>) -> Centre {
    let needs_user = read::NeedsUser::default();
    let boxes = boxes::layout(&prs, &jobs, &needs_user, 0).with_margin(CHART_MARGIN);
    let plan = PlanBoxes {
        plan: PlanId::parse("p1").unwrap(),
        context: cpd_core::ids::ContextId::parse("ctx").unwrap(),
        prs,
        jobs,
        needs_user,
        events_cursor: 0,
    };
    let slot = ChatSlot { session: SessionId::parse("lead").unwrap(), name: "n".into(), plan: plan.plan.clone(), moved_event: 1 };
    Centre { slot, plan, boxes, frame: 0 }
}

/// The layout test's sample (`layout_tests`): the Plan view drawn by [`Ui::draw`] into a `width` x `height` pane at a
/// scroll offset. Placed is one box per PR, its rect on the canvas (`boxes::layout`'s buffer, with the canvas margin) less the
/// offset the draw clamped to, kept to the chart's window: the canvas does not expose the boxes' rects, so each is read off
/// the canvas by its corners (`╭` to the next `╮` on its row, `╭` to the next `╰` down its column). A box the window
/// clips is `Rule::Clipped`.
#[cfg(test)]
pub(crate) fn layout_sample(width: u16, height: u16, case: crate::layout_tests::PlanCase) -> crate::layout_tests::Sample {
    use crate::layout_tests::{Exempt, Kind, PlanCase, Placed, Rule, Sample, Scroll};
    use ratatui::backend::TestBackend;
    let (scroll, prs, jobs) = match case {
        PlanCase::Empty(scroll) => (scroll, vec![sample_pr("api")], Vec::new()),
        PlanCase::Three(scroll) => (
            scroll,
            vec![sample_pr("api"), sample_pr("web")],
            vec![sample_job("api", "api-1"), sample_job("api", "api-2"), sample_job("web", "web-1")],
        ),
    };
    let expected = prs.len();
    let centre = sample_centre(prs, jobs);
    let canvas = &centre.boxes.chart.buf;
    let mut ui = Ui::default();
    if scroll == Scroll::Middle {
        ui.scroll = (canvas.area.width / 2, canvas.area.height / 2);
    }
    let mut terminal = ratatui::Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| ui.draw(f, Some(&centre))).unwrap();
    let window = ui.body;
    let (ox, oy) = ui.scroll;
    let mut boxes = Vec::new();
    for y in 0..canvas.area.height {
        for x in 0..canvas.area.width {
            if canvas[(x, y)].symbol() != "╭" {
                continue;
            }
            let right = (x + 1..canvas.area.width).find(|&x2| canvas[(x2, y)].symbol() == "╮");
            let bottom = (y + 1..canvas.area.height).find(|&y2| canvas[(x, y2)].symbol() == "╰");
            let (Some(right), Some(bottom)) = (right, bottom) else { continue };
            let whole = Rect::new(x, y, right - x + 1, bottom - y + 1);
            let view = Rect::new(ox, oy, window.width, window.height);
            let seen = view.intersection(whole);
            let rect = Rect::new(window.x + seen.x.saturating_sub(ox), window.y + seen.y.saturating_sub(oy), seen.width, seen.height);
            let exempt = if seen == whole {
                Vec::new()
            } else {
                vec![Exempt { name: "the pane clips the box: the canvas scrolls", row: rect.y, rule: Rule::Clipped }]
            };
            boxes.push(Placed { name: "pr box", kind: Kind::FullText, rect, exempt });
        }
    }
    assert_eq!(boxes.len(), expected, "one box per PR on the canvas\n{}", crate::layout_tests::dump(canvas));
    Sample { buf: terminal.backend().buffer().clone(), container: Rect::new(0, 0, width, height), boxes }
}

#[cfg(test)]
mod box_link_tests;
#[cfg(test)]
mod box_tests;

#[cfg(test)]
mod margin_tests;
#[cfg(test)]
mod pill_label_tests;
#[cfg(test)]
mod pill_tier_tests;
#[cfg(test)]
mod lines_tests;
#[cfg(test)]
mod box_lines_tests;
#[cfg(test)]
mod counter_tests;
#[cfg(test)]
mod footer_cost_tests;
#[cfg(test)]
mod needs_tests;
#[cfg(test)]
mod chain_tests;
#[cfg(test)]
mod explore_view_tests;

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    use super::*;
    use crate::dialog::scroll::WHEEL_ROWS;
    use crate::read::{JobNode, JobStatus, PrNode, PrStatus};
    use cpd_core::config::Tier;
    use cpd_core::ids::{ContextId, JobKey, PrKey, SessionId};


    #[test]
    fn a_read_is_taken_for_a_commit_at_most_once_per_frame_and_for_no_change_only_on_the_safety_time() {
        use crate::pump::{FRAME, SAFETY, Schedule};
        let t0 = Instant::now();
        let mut s = Schedule::new(t0, SAFETY);
        assert!(s.take(t0 + FRAME).is_none(), "no change: no read");
        s.commit();
        assert!(s.take(t0 + FRAME).is_some(), "a commit: one read");
        s.done();
        s.commit();
        assert!(s.take(t0 + FRAME + FRAME / 2).is_none(), "a second commit in the same frame waits");
        assert!(s.take(t0 + FRAME * 2).is_some());
    }

    #[test]
    fn the_watching_ids_say_which_events_concern_the_view() {
        let w = Watching { plan: Some("p".into()), context: Some("c".into()), pulse: None };
        let event = |kind: &str, plan: &str| Event {
            id: 1,
            at: String::new(),
            kind: kind.into(),
            context: None,
            plan: Some(plan.into()),
            job: None,
            attempt: None,
            session: None,
            text: String::new(),
            data: serde_json::Value::Null,
        };
        assert!(w.concerns(&event("job-merged", "p")) && !w.concerns(&event("job-merged", "q")));
        assert!(w.concerns(&event("session-moved", "q")));
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
            elapsed_seconds: None,
            cost_usd: None,
            lead_dot: None,
        };
        let job = |n: usize| JobNode {
            kind: "work".into(),
            key: JobKey::parse(&format!("job-{n}")).unwrap(),
            pr: Some(pr.clone()),
            seq: 0,
            title: String::new(),
            status: JobStatus::Queued,
            tier: Some(Tier::parse("S1").unwrap()),
            needs: Vec::new(),
            appended: false,
            retry_tier: None,
            shown_tier: Some(Tier::parse("S1").unwrap()),
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
    fn switching_plans_and_back_restores_the_offset() {
        let (a, b) = (plan("plan-a"), plan("plan-b"));
        let mut ui = Ui::default();
        draw(&mut ui, &scroll_centre("plan-a", 30));
        ui.scroll = (0, 7);

        ui.switch(Some(&a), Some(&b));
        assert_eq!(ui.scroll, (0, 0), "B starts at its corner");
        ui.scroll = (0, 2);

        ui.switch(Some(&b), Some(&a));
        assert_eq!(ui.scroll, (0, 7));
        ui.switch(Some(&a), Some(&b));
        assert_eq!(ui.scroll, (0, 2), "each Plan has its own");
    }

    #[test]
    fn two_leads_of_one_plan_share_its_offset() {
        let a = plan("plan-a");
        let mut ui = Ui { scroll: (4, 9), ..Ui::default() };
        ui.switch(Some(&a), Some(&a));
        assert_eq!(ui.scroll, (4, 9));
    }

    #[test]
    fn no_lead_and_back_is_the_same_plan() {
        let a = plan("plan-a");
        let mut ui = Ui { scroll: (0, 5), ..Ui::default() };
        ui.switch(Some(&a), None);
        assert_eq!(ui.scroll, (0, 0));
        ui.switch(None, Some(&a));
        assert_eq!(ui.scroll, (0, 5));
    }

    #[test]
    fn a_chart_that_shrank_clamps_the_restored_offset_when_drawn() {
        let (a, b) = (plan("plan-a"), plan("plan-b"));
        let mut ui = Ui { scroll: (0, 20), ..Ui::default() };
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
            pulse::row_at(ui.body, ui.pulse_scroll.offset(), pulse.rows.len(), 5, y).map(|i| pulse.rows[i].lead.to_string())
        };
        assert_eq!(lead(&ui, 2).as_deref(), Some("lead-bb"));
        ui.pulse_scroll.by(WHEEL_ROWS as isize);
        ui.pulse_scroll.by(WHEEL_ROWS as isize);
        assert_eq!(ui.pulse_scroll.offset(), 3, "clamped at the last row");
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
    fn the_pulse_re_reads_on_its_contexts_events() {
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
