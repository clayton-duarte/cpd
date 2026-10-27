//! The `history` view (commands.md §13.2, §17): an archived Session's pi file,
//! read-only, in the chat slot. `cpd session history <id>` puts it there.
//!
//! `cpd tui view history <id>` reads the Session's `file` from the store and
//! draws it once with the session renderer
//! ([`crate::history::render`]); nothing is followed or re-read. An unknown
//! Session is `session_not_found`, a Session with no readable file
//! `file_unreadable`, both before anything is drawn. The top line is
//! `<name> (archived, read-only)`.
//!
//! Keys: Up/`k`, Down/`j`, PageUp, PageDown and the mouse wheel scroll; End or
//! `G` follows the end again (the view starts there); `q` or Esc leaves the
//! history for the chat slot's placeholder text.

mod render;
mod view;

#[cfg(test)]
mod local_time_tests;
#[cfg(test)]
mod render_tests;

use std::io::Write;
use std::time::Duration;

use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::crossterm::event::Event;
use rusqlite::OptionalExtension;

use super::view_ports::{Ctx, Input, Tty, run_view_with};
use super::{Capture, chat, run_view_on};
use crate::history::render::render;
use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::home::CpdHome;
use cpd_core::ids::SessionId;
use cpd_core::output::Output;
use cpd_core::ports::Ports;
use cpd_core::store::Store;
use view::View;

/// `cpd tui view history <id>`.
#[derive(clap::Args, Debug)]
pub struct Args {
    /// The Session whose pi file to show
    #[arg(value_name = "id")]
    id: SessionId,
}

/// What the view shows: the Session's name and whether it is archived, and its file's text.
struct History {
    name: String,
    archived: bool,
    text: String,
}

pub(super) fn run(ports: &Ports, args: Args) -> Result<Output, CpdError> {
    let history = open(args)?;
    run_view_on(ports, "history", Capture::Mouse, || Ok(history), |ctx, history| Ok(event_loop_with(ctx, history)?))?;
    // `q` or Esc: the pane shows the placeholder text, as the chat slot's placeholder does.
    chat::run()
}

/// [`run`] on a terminal, its facts and its input that the caller gives (a test's `TestBackend`, scripted input and a
/// [`Tty`] that says whether there is a terminal): the same refusals before anything is drawn. It ends with the view: the
/// chat slot's placeholder, which `run` shows after it, reads the terminal and is no part of the history.
pub fn run_on<B: Backend, W: Write>(
    ports: &Ports,
    args: Args,
    tty: &mut dyn Tty,
    out: &mut W,
    terminal: Terminal<B>,
    input: &mut dyn Input,
) -> Result<Output, CpdError> {
    let history = open(args)?;
    run_view_with(ports, tty, out, terminal, input, "history", Capture::Mouse, || Ok(history), |ctx, history| {
        Ok(event_loop_with(ctx, history)?)
    })
}

/// What the view shows, read before anything is drawn.
fn open(args: Args) -> Result<History, CpdError> {
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    // `session_not_found` and `file_unreadable` refuse before anything is drawn.
    load(&mut store, &args.id)
}

/// The view's first frame for the Session `name` over the text of its file, drawn on `f` as `run` draws it (the view starts at
/// the end). Tests draw it on a `TestBackend` instead of running the view in a terminal.
pub fn draw_history(name: &str, archived: bool, text: &str, zone: &cpd_core::clock::TimeZone, f: &mut ratatui::Frame) {
    let mut view = View::new(name, archived);
    view.set_lines(render(text, zone));
    view.draw(f);
}

fn load(store: &mut Store, id: &SessionId) -> Result<History, CpdError> {
    let (name, file, archived): (String, Option<String>, bool) = store
        .read(|conn| {
            Ok(conn
                .query_row("SELECT name, file, archived_at IS NOT NULL FROM sessions WHERE id = ?1", [id.as_str()], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })
                .optional()?)
        })?
        .ok_or_else(|| CpdError::new(ErrorCode::SessionNotFound, format!("No Session {id}.")).with("session", id.as_str()))?;
    let unreadable = |why: String| CpdError::new(ErrorCode::FileUnreadable, why).with("session", id.as_str());
    let file = file.ok_or_else(|| unreadable(format!("Session {id} has no session file.")))?;
    let text = std::fs::read_to_string(&file).map_err(|e| unreadable(format!("Cannot read {file} as UTF-8 text: {e}.")))?;
    Ok(History { name, archived, text })
}

fn event_loop_with<B: Backend>(ctx: &mut Ctx<B>, history: History) -> std::io::Result<()> {
    show_with(ctx, &history.name, history.archived, &history.text)
}

/// What the loop waits for the input, when nothing else is due: the view changes only by input, so any time will do.
const IDLE: Duration = Duration::from_secs(60);

/// Draws the history of the Session `name` over the text of its file and takes input until `q` or Esc: what `run` runs in
/// its terminal. Input and frames are the [`Ctx`]'s, so a test runs it on a `TestBackend` and scripted input.
pub fn show_with<B: Backend>(ctx: &mut Ctx<B>, name: &str, archived: bool, text: &str) -> std::io::Result<()> {
    let mut view = View::new(name, archived);
    view.set_lines(render(text, &ctx.ports.clock.zone()));
    loop {
        ctx.draw(|f| view.draw(f))?;
        while !ctx.input.poll(IDLE)? {}
        match ctx.input.read()? {
            Event::Key(key) => {
                if view.key(key) {
                    return Ok(());
                }
            }
            Event::Mouse(m) => view.mouse(m),
            _ => {}
        }
    }
}
