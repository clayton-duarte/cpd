//! The `logs` view (commands.md §13.2, §17; R36): the Event log, in a tmux popup opened by the StatusBar's
//! `▤` button. It takes the latest [`RECENT`] Events, then looks at the store's highest Event id every
//! `events.poll_ms` and appends what is new. ↑↓, j/k, PgUp/PgDn and the mouse wheel scroll (the dialog kit's
//! scrolling, mouse capture on); its title is a section header; `q` or Esc closes it. It makes no write of its own. With `--event <id>` it opens at that Event (a click on an `engine-error` or `engine-down` Inbox
//! item): the Event's line is selected and scrolled into view, and the view does not follow until the User scrolls to the
//! bottom; an Event outside the latest [`RECENT`] is read on its own and shown first.
//!
//! - [`view`]: the state and the drawing;
//! - [`open`], [`open_at`] and [`popup_args`]: the popup, which the button and the Inbox click run.

mod view;

use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind};

pub use self::view::{Log, draw};
use super::read;
use super::widgets::{legend, newest_client};
use super::runner::{ViewError, kept};
use super::{Capture, run_view};
use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::Selection;
use cpd_core::home::CpdHome;
use cpd_core::host::Tmux;
use cpd_core::output::Output;
use cpd_core::store::Store;

/// How many Events the view starts with (the `events list` default).
pub const RECENT: u32 = 500;
/// `cpd tui view logs`.
#[derive(clap::Args, Debug)]
pub struct Args {
    /// Open at this Event (an Event id), its line selected and in view; without it, at the bottom
    #[arg(long, value_name = "id")]
    pub event: Option<i64>,
}

/// The popup's share of the client, in percent, at most.
const SHARE: u16 = 80;

/// The tmux arguments that open the log in a popup: sized like the legend's, at most [`SHARE`] % of the client's
/// size `(columns, rows)`; the view opens at the Event `event` when there is one.
pub fn popup_args(client: (u16, u16), event: Option<i64>) -> Vec<String> {
    let cap = |n: u16| (u32::from(n) * u32::from(SHARE) / 100) as u16;
    let mut args = legend::popup_args((cap(client.0), cap(client.1)));
    args[3] = " logs ".to_owned();
    *args.last_mut().expect("popup_args ends with the command") = match event {
        Some(id) => format!("cpd tui view logs --event {id}"),
        None => "cpd tui view logs".to_owned(),
    };
    args
}

/// The StatusBar's `▤` click: the log in a tmux popup over the cockpit, at the bottom.
pub(crate) fn open(tmux: &Tmux) -> Result<(), CpdError> {
    open_at(tmux, None)
}

/// The log in a tmux popup over the cockpit, at the Event `event` (the bottom with none): the click on an Inbox item.
pub(crate) fn open_at(tmux: &Tmux, event: Option<i64>) -> Result<(), CpdError> {
    let clients = tmux.run(&["list-clients", "-F", "#{client_activity} #{client_width} #{client_height}"].map(str::to_owned))?;
    let client = newest_client(&clients).ok_or_else(|| {
        CpdError::new(ErrorCode::TmuxFailed, "No tmux client is attached to the cockpit.").with("socket", tmux.socket())
    })?;
    tmux.run(&popup_args(client, event)).map(drop)
}

pub(super) fn run(args: Args) -> Result<Output, CpdError> {
    run_view(
        "logs",
        Capture::Mouse,
        || {
            let home = CpdHome::from_env()?;
            let config = Config::load(&home)?;
            let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
            let first = read::logs(&mut store, RECENT)?;
            let log = match args.event {
                Some(id) => Log::at(with_event(&mut store, first.events, id)?, id),
                None => Log::new(first.events),
            };
            let poll = Duration::from_millis(u64::from(config.events.poll_ms));
            let follow = read::Follow::new(&mut store, &[], first.events_cursor)?;
            Ok((store, poll, follow, log))
        },
        |terminal, (mut store, poll, follow, log)| show(terminal, &mut store, poll, follow, log),
    )
}

/// The keys that close the view.
fn closes(code: KeyCode) -> bool {
    matches!(code, KeyCode::Char('q') | KeyCode::Esc)
}

/// Draws and follows until `q` or Esc; the keys and the wheel scroll. A failed read keeps the Events on screen and shows its error in the last row
/// until a read succeeds ([`kept`]).
fn show(
    terminal: &mut ratatui::DefaultTerminal,
    store: &mut Store,
    poll: Duration,
    mut follow: read::Follow,
    mut log: Log,
) -> Result<(), ViewError> {
    let mut redraw = true; // the pane is written to only when something changed: Events, an error, input, a resize
    loop {
        if redraw {
            terminal.draw(|f| {
                log.fit(f.area().height);
                draw(f, f.area(), &log);
            })?;
            redraw = false;
        }
        if event::poll(poll)? {
            redraw = true;
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press && closes(key.code) => return Ok(()),
                Event::Key(key) => drop(log.key(key)),
                Event::Mouse(mouse) => drop(log.mouse(mouse)),
                _ => {}
            }
        }
        // the follow's cursor stays where it was on a failure, so the next poll reads what was missed
        let shown = log.error.clone();
        if let Some(events) = kept(follow.poll(store), &mut log.error) {
            redraw |= !events.is_empty();
            log.push(events);
        }
        redraw |= log.error != shown;
    }
}

/// `events` (the latest, oldest first), with the Event `id` before them when it is older than all of them. An `id`
/// that no Event has leaves them as they are.
fn with_event(store: &mut Store, mut events: Vec<cpd_core::events::Event>, id: i64) -> Result<Vec<cpd_core::events::Event>, CpdError> {
    if events.first().is_some_and(|e| e.id <= id) {
        return Ok(events);
    }
    let own = store.read(|conn| {
        Selection::resolve(conn, None, None, None, Vec::new())?.fetch(conn, id - 1, Some(id), 1)
    })?;
    events.splice(0..0, own);
    Ok(events)
}

/// Documentation drift (store-G7, tui-G5, pi-ci-G3): the docs and comments say what the code does.
/// The phrases are split so that this file does not contain them.
#[cfg(test)]
mod docs_agree {
    const CONTRACT: &str = include_str!("../../../docs/contract.md");
    const COMMANDS: &str = include_str!("../../../docs/commands.md");
    const LOGS: &str = include_str!("mod.rs");
    const PLAN: &str = include_str!("../plan/mod.rs");

    #[test]
    fn no_stale_statement_is_left() {
        let stale = [
            (CONTRACT, concat!("the store", " Job")),
            (CONTRACT, concat!("≡", " logs")),
            (COMMANDS, concat!("≡", " logs")),
            (COMMANDS, concat!("follow-up", " Job 17")),
            (COMMANDS, concat!("the adapter Job", " tests it")),
            (COMMANDS, concat!("applies an Event's `data`", " directly")),
            (LOGS, concat!("≡", " logs")),
            (PLAN, concat!("stays", " dead")),
        ];
        let left: Vec<&str> = stale.iter().filter(|(text, phrase)| text.contains(phrase)).map(|(_, phrase)| *phrase).collect();
        assert!(left.is_empty(), "stale statements still present: {left:?}");
    }
}
