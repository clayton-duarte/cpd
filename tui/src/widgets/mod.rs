//! The `widgets` view (commands.md §13.2, §17): the Dispatch column, the
//! machine-wide right column of the cockpit. It takes one snapshot of the read
//! model ([`read::widgets`]), then follows the Events: it looks at the highest
//! Event id every [`TICK`] and re-reads when it moved, at most once per
//! [`COALESCE`]; the load average is re-read at least every [`REFRESH`], since
//! it is no Event. It makes no write of its own, but for what a click on an owned PR row asks of `cpd session open`. Its one
//! key is `q`; mouse capture is on.
//!
//! - [`column`]: the drawing of the machine, the PRs, git and Jira;
//! - [`legend`]: the key legend, `cpd tui view legend`, which the tree's `?` and
//!   the StatusBar's button (`cpd tui action legend`) open in a tmux popup (the columns are narrower than the legend);
//! - the colours and icons are [`crate::palette`]'s.

mod column;
pub mod legend;

use std::process::Command;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind, MouseButton, MouseEventKind,
};
use ratatui::layout::Position;

use super::read::{self, Widgets};
use super::{RunCommand, not_a_terminal, on_a_terminal};
use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode};
use cpd_core::events::highest_id;
use cpd_core::home::CpdHome;
use cpd_core::host::layout::MAIN_WINDOW;
use cpd_core::host::{Pane, Tmux, chat_slot};
use cpd_core::ids::SessionId;
use cpd_core::session::PROGRAM;
use cpd_core::output::Output;
use cpd_core::store::Store;

pub use column::{Hit, Machine, draw};
pub(crate) use column::section_header;

/// How often the view looks at the Events and at keys.
const TICK: Duration = Duration::from_millis(200);
/// At most one snapshot per this long, whatever the Event rate (§17).
const COALESCE: Duration = Duration::from_millis(500);
/// A snapshot and the load at least this often: the load average is no Event.
const REFRESH: Duration = Duration::from_secs(2);

pub(super) fn run(run_command: RunCommand) -> Result<Output, CpdError> {
    if !on_a_terminal() {
        return Err(not_a_terminal("tui view widgets"));
    }
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    let machine = Machine { name: hostname()?, load: load_average()? };
    // The column shows no Event: none is asked for.
    let first = read::widgets(&mut store, config.concurrency, 0)?;
    // From here on this process owns the terminal (commands.md §1.2).
    let mut terminal = ratatui::init();
    let tmux = Tmux::new(config.tmux.socket.clone());
    let lead = Opener { run_command, tmux: &tmux, cockpit: &config.tmux.cockpit_session };
    let result = ratatui::crossterm::execute!(std::io::stdout(), EnableMouseCapture)
        .map_err(terminal_failed)
        .and_then(|()| show(&mut terminal, &mut store, config.concurrency, first, machine, &lead));
    let _ = ratatui::crossterm::execute!(std::io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result.map(|()| Output::Owned)
}

fn terminal_failed(e: std::io::Error) -> CpdError {
    CpdError::new(ErrorCode::NotATerminal, format!("The terminal of cpd tui view widgets failed: {e}."))
        .with("command", "tui view widgets")
}

/// What a click on an owned PR row runs: `session open <lead>` through `run_command`, then the focus on the chat slot.
struct Opener<'a> {
    run_command: RunCommand,
    tmux: &'a Tmux,
    cockpit: &'a str,
}

/// Draws and follows until `q`. Reading failures end the view with their error. A left click on an owned PR row (not on its
/// `↗`) opens its Lead in the chat slot; a refusal is the `note` in the column's last row until the next click.
fn show(
    terminal: &mut ratatui::DefaultTerminal,
    store: &mut Store,
    concurrency: u32,
    mut widgets: Widgets,
    mut machine: Machine,
    opener: &Opener,
) -> Result<(), CpdError> {
    let mut read_at = Instant::now();
    let mut hits = Vec::new();
    let mut note = String::new();
    loop {
        terminal.draw(|f| hits = draw(f, f.area(), &widgets, &machine, &note)).map_err(terminal_failed)?;
        if event::poll(TICK).map_err(terminal_failed)? {
            match event::read().map_err(terminal_failed)? {
                Event::Key(key) if quits(&key) => return Ok(()),
                Event::Mouse(m) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                    note.clear();
                    let at = Position::new(m.column, m.row);
                    if let Some(hit) = hits.iter().find(|h| h.contains(at)) {
                        if let Err(e) = open_lead(opener.run_command, opener.tmux, opener.cockpit, &hit.lead) {
                            note = e.message().to_owned();
                        }
                    }
                }
                _ => {}
            }
        }
        if read_at.elapsed() >= COALESCE {
            let moved = store.read(highest_id)? != widgets.events_cursor;
            if moved || read_at.elapsed() >= REFRESH {
                widgets = read::widgets(store, concurrency, 0)?;
                machine.load = load_average()?;
                read_at = Instant::now();
            }
        }
    }
}

/// A click on a tab or on an owned PR row: `session open <id>`, which moves that Lead into the chat slot, then `select-pane`
/// on the chat. The plan view's tabs and this column's rows share it.
pub(crate) fn open_lead(run_command: RunCommand, tmux: &Tmux, cockpit: &str, session: &SessionId) -> Result<(), CpdError> {
    let argv = [PROGRAM, "session", "open", session.as_str()].map(std::ffi::OsString::from).into();
    run_command(argv)?;
    if let Some(chat) = chat_slot(&main_panes(tmux, cockpit)?, cockpit) {
        tmux.run(&["select-pane", "-t", chat.pane.as_str()].map(str::to_owned))?;
    }
    Ok(())
}

/// The panes of the cockpit's `main` window.
pub(crate) fn main_panes(tmux: &Tmux, cockpit: &str) -> Result<Vec<Pane>, CpdError> {
    let all = tmux.list_panes()?;
    Ok(all.into_iter().filter(|p| p.session_name == cockpit && p.window_name == MAIN_WINDOW).collect())
}

/// `q` is the view's only key (commands.md §13.2): `?` belongs to the tree and the StatusBar.
fn quits(key: &KeyEvent) -> bool {
    key.kind == KeyEventKind::Press && key.code == KeyCode::Char('q')
}

fn host_failed(what: &str, why: impl std::fmt::Display) -> CpdError {
    CpdError::new(ErrorCode::SpawnFailed, format!("Reading {what} for the Dispatch column failed: {why}."))
}

/// The machine's name, as `hostname` prints it.
fn hostname() -> Result<String, CpdError> {
    let out = Command::new("hostname").output().map_err(|e| host_failed("the hostname", e))?;
    let name = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    if !out.status.success() || name.is_empty() {
        return Err(host_failed("the hostname", format!("`hostname` exited with {}", out.status)));
    }
    Ok(name)
}

/// The first three fields of the load average: `/proc/loadavg` on Linux, `sysctl -n vm.loadavg` on macOS.
fn load_average() -> Result<Vec<String>, CpdError> {
    let text = if cfg!(target_os = "macos") {
        let out = Command::new("sysctl").args(["-n", "vm.loadavg"]).output().map_err(|e| host_failed("the load", e))?;
        String::from_utf8_lossy(&out.stdout).into_owned()
    } else {
        std::fs::read_to_string("/proc/loadavg").map_err(|e| host_failed("the load", e))?
    };
    parse_load(&text).ok_or_else(|| host_failed("the load", format!("{:?} has no three numbers", text.trim())))
}

/// The three numbers that open `text` (`16.89 24.21 25.00 6/3484 1221694`, or sysctl's `{ 0.52 0.58 0.59 }`).
fn parse_load(text: &str) -> Option<Vec<String>> {
    let fields: Vec<String> = text.split_whitespace().filter(|f| *f != "{").take(3).map(str::to_owned).collect();
    (fields.len() == 3 && fields.iter().all(|f| f.parse::<f64>().is_ok())).then_some(fields)
}

/// The legend in a tmux popup over the cockpit, sized to it and at most the client's size.
pub(crate) fn open_legend(tmux: &Tmux) -> Result<(), CpdError> {
    let clients = tmux.run(&["list-clients", "-F", "#{client_activity} #{client_width} #{client_height}"].map(str::to_owned))?;
    let client = newest_client(&clients).ok_or_else(|| {
        CpdError::new(ErrorCode::TmuxFailed, "No tmux client is attached to the cockpit.").with("socket", tmux.socket())
    })?;
    tmux.run(&legend::popup_args(client)).map(drop)
}

/// The size `(columns, rows)` of the most recently active client in `list-clients`'
/// `#{client_activity} #{client_width} #{client_height}` lines.
pub(crate) fn newest_client(listing: &str) -> Option<(u16, u16)> {
    listing
        .lines()
        .filter_map(|line| {
            let n: Vec<u64> = line.split(' ').filter_map(|x| x.parse().ok()).collect();
            Some((*n.first()?, u16::try_from(*n.get(1)?).ok()?, u16::try_from(*n.get(2)?).ok()?))
        })
        .max()
        .map(|(_, w, h)| (w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_legend_popup_is_sized_by_the_most_recently_active_client() {
        assert_eq!(newest_client("100 80 24\n300 215 62\n200 120 30\n"), Some((215, 62)));
        assert_eq!(newest_client(""), None);
        assert_eq!(newest_client("junk\n"), None);
    }

    #[test]
    fn q_is_the_only_key() {
        // Mouse clicks are handled apart, in `show`; no other key does anything.
        let press = |code| KeyEvent::new(code, ratatui::crossterm::event::KeyModifiers::NONE);
        assert!(quits(&press(KeyCode::Char('q'))));
        for code in [KeyCode::Char('?'), KeyCode::Esc, KeyCode::Enter, KeyCode::Char('j'), KeyCode::Char('Q')] {
            assert!(!quits(&press(code)), "{code:?}");
        }
    }

    #[test]
    fn the_load_is_the_first_three_numbers_of_proc_loadavg_or_sysctl() {
        let load = |text| parse_load(text).map(|l| l.join(" "));
        assert_eq!(load("16.89 24.21 25.00 6/3484 1221694\n").as_deref(), Some("16.89 24.21 25.00"));
        assert_eq!(load("{ 0.52 0.58 0.59 }\n").as_deref(), Some("0.52 0.58 0.59"));
        assert_eq!(load("1.0 2.0"), None);
        assert_eq!(load("a b c"), None);
        assert_eq!(load(""), None);
    }
}
