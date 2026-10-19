//! The `logs` view (commands.md §13.2, §17; R36): the Event log, in a tmux popup opened by the StatusBar's
//! ` ≡ logs ` button. It takes the latest [`RECENT`] Events, then looks at the store's highest Event id every
//! `events.poll_ms` and appends what is new. ↑↓, j/k and PgUp/PgDn scroll; `q` or Esc closes it. It makes no
//! write of its own.
//!
//! - [`view`]: the state and the drawing;
//! - [`open`] and [`popup_args`]: the popup, which the button runs.

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
use cpd_core::home::CpdHome;
use cpd_core::host::Tmux;
use cpd_core::output::Output;
use cpd_core::store::Store;

/// How many Events the view starts with (the `events list` default).
pub const RECENT: u32 = 500;
/// The popup's share of the client, in percent, at most.
const SHARE: u16 = 80;

/// The tmux arguments that open the log in a popup: sized like the legend's, at most [`SHARE`] % of the client's
/// size `(columns, rows)`.
pub fn popup_args(client: (u16, u16)) -> Vec<String> {
    let cap = |n: u16| (u32::from(n) * u32::from(SHARE) / 100) as u16;
    let mut args = legend::popup_args((cap(client.0), cap(client.1)));
    args[3] = " logs ".to_owned();
    *args.last_mut().expect("popup_args ends with the command") = "cpd tui view logs".to_owned();
    args
}

/// The StatusBar's ` ≡ logs ` click: the log in a tmux popup over the cockpit.
pub(crate) fn open(tmux: &Tmux) -> Result<(), CpdError> {
    let clients = tmux.run(&["list-clients", "-F", "#{client_activity} #{client_width} #{client_height}"].map(str::to_owned))?;
    let client = newest_client(&clients).ok_or_else(|| {
        CpdError::new(ErrorCode::TmuxFailed, "No tmux client is attached to the cockpit.").with("socket", tmux.socket())
    })?;
    tmux.run(&popup_args(client)).map(drop)
}

pub(super) fn run() -> Result<Output, CpdError> {
    run_view(
        "logs",
        Capture::Keys,
        || {
            let home = CpdHome::from_env()?;
            let config = Config::load(&home)?;
            let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
            let first = read::logs(&mut store, RECENT)?;
            let poll = Duration::from_millis(u64::from(config.events.poll_ms));
            let follow = read::Follow::new(&mut store, &[], first.events_cursor)?;
            Ok((store, poll, follow, first))
        },
        |terminal, (mut store, poll, follow, first)| show(terminal, &mut store, poll, follow, first),
    )
}

/// Draws and follows until `q` or Esc. A failed read keeps the Events on screen and shows its error in the last row
/// until a read succeeds ([`kept`]).
fn show(
    terminal: &mut ratatui::DefaultTerminal,
    store: &mut Store,
    poll: Duration,
    mut follow: read::Follow,
    first: read::Logs,
) -> Result<(), ViewError> {
    let mut log = Log::new(first.events);
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
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                        code => log.key(code),
                    }
                }
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
