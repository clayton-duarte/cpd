//! `cpd tui [--attach] [--respawn]` (commands.md §13.1; contract §6.1, §6.2,
//! §6.4 with amendment 9): builds the cockpit's `main` window when it is
//! missing, respawns a dead pane of its layout, sets §6.1's server options
//! and the cockpit session's status line (the StatusBar) again, with
//! `--respawn` restarts every live view, starts the engine, and with
//! `--attach` replaces itself with `tmux attach`. When the server runs an older tmux than the
//! installed one it marks a restart pending (contract §6.1; [`cpd_core::watch::restart`]).
//! Also reset-layout (commands.md §13.2), and `cpd tui action`, what a click on a StatusBar
//! button runs.
//!
//! The layout, from the spike's `cockpit.sh` (branch `cpd/cpd-ui`):
//!
//! ```text
//! +-------+-------------------+---------+
//! | tree  | plan (20 %)       | widgets |
//! | 20 %  +-------------------+  20 %   |
//! |       | chat slot (80 %)  |         |
//! +-------+-------------------+---------+
//! ```
//!
//! Below it, tmux's status line of the cockpit session is the StatusBar
//! (contract §6.1); it has no pane.
//!
//! A new `main` is reset to these sizes once it is built (A3); an existing one
//! is not, unless it still had an older cpd's StatusBar pane: killing it is
//! followed by the reset (amendment 15).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};

use rusqlite::OptionalExtension;
use serde_json::{Map, Value, json};

use super::widgets::legend;
use super::{RunCommand, logs, not_a_terminal, on_a_terminal};
use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode, Exit};
use cpd_core::home::CpdHome;
use cpd_core::host::exe::started_as;
use cpd_core::host::layout::{LEGACY_STATUSBAR, MAIN_WINDOW};
use cpd_core::host::tmux::{installed_version, older_version};
use cpd_core::host::{Pane, PaneId, Spawn, Split, Tmux, View};
use cpd_core::ids::SessionId;
use cpd_core::output::Output;
use cpd_core::session::PROGRAM;
use cpd_core::store::Store;
use cpd_core::watch::restart;

/// The views of `main`'s layout: a dead one is respawned (amendment 9). The
/// `worker` view is not part of it: it is opened and closed by the plan view.
const LAYOUT: [View; 4] = [View::Tree, View::Plan, View::Widgets, View::Chat];

pub(super) fn run(attach: bool, respawn: bool, run_command: RunCommand) -> Result<Output, CpdError> {
    if attach && !on_a_terminal() {
        return Err(not_a_terminal("tui --attach"));
    }
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    let tmux = Tmux::new(config.tmux.socket.clone());
    let cockpit = config.tmux.cockpit_session.as_str();

    let (built, respawned) = ensure(&tmux, &home, &mut store, cockpit, respawn, run_command)?;
    let watch = command(run_command, &["watch", "--ensure"])?;
    if respawn {
        if let Some(preview) = tmux.preview()? {
            eprintln!(
                "a preview is on ({preview}); its views keep the preview build until 'preview-tui.sh off'"
            );
        }
    }
    // An older server is restarted by the engine once nothing is in the way (contract §6.1).
    let after = match mark_if_older(&home, &tmux) {
        Ok(Some(why)) => {
            if !attach {
                eprintln!("cpd tui: the tmux server runs an older tmux than the installed one{why}.");
            }
            why
        }
        Ok(None) => String::new(),
        Err(e) => {
            eprintln!("cpd tui: cannot compare the tmux server's version with the installed tmux: {}", e.message());
            String::new()
        }
    };

    let attach_argv = ["tmux", "-L", tmux.socket(), "attach", "-t", cockpit];
    if attach {
        let program = server_tmux(&tmux, &after).unwrap_or(None);
        return Err(exec_attach(&tmux, cockpit, program.as_deref()));
    }
    let mut fields = json!({
        "built": built, "socket": tmux.socket(), "session": cockpit, "attach": attach_argv, "watch": watch,
    });
    if respawn {
        fields["respawned"] = json!(respawned.iter().map(PaneId::as_str).collect::<Vec<_>>());
    }
    Ok(Output::success(fields))
}

/// The cockpit part of `cpd tui` and of bare `cpd`: builds `main` when it is
/// missing, else respawns its dead panes, with `respawn` every live view, and
/// removes an older cpd's StatusBar pane; sets the server options and the
/// status line again; brings the last chat
/// Lead into a new chat slot. Returns whether it built `main` and the panes
/// restarted by `respawn`.
fn ensure(
    tmux: &Tmux,
    home: &CpdHome,
    store: &mut Store,
    cockpit: &str,
    respawn: bool,
    run_command: RunCommand,
) -> Result<(bool, Vec<PaneId>), CpdError> {
    let main = main_panes(tmux, cockpit)?;
    let built = main.is_empty();
    let mut respawned = Vec::new();
    if built {
        build(tmux, home, cockpit)?;
    } else {
        respawn_dead(tmux, home, &main)?;
        if respawn {
            respawned = respawn_live_views(tmux, home, &main)?;
        }
        remove_old_statusbar(tmux, cockpit)?;
    }
    tmux.apply_server_options()?;
    tmux.apply_status_line(cockpit)?;
    if built {
        if let Some(lead) = store.read(last_chat_lead)? {
            open_lead(run_command, &lead)?;
        }
    }
    Ok((built, respawned))
}

/// The panes of `<cockpit>:main`.
fn main_panes(tmux: &Tmux, cockpit: &str) -> Result<Vec<Pane>, CpdError> {
    Ok(tmux.list_panes()?.into_iter().filter(|p| p.session_name == cockpit && p.window_name == MAIN_WINDOW).collect())
}

/// What a view's pane runs: `cpd tui view <view>` (see [`home_spawn`]).
fn view_spawn(home: &CpdHome, view: View) -> Spawn {
    home_spawn(home, [PROGRAM, "tui", "view", view.as_str()].map(str::to_owned).into())
}

/// `argv` in `$CPD_HOME`, with `CPD_HOME` passed with `-e`, since a server
/// started by another process does not have this process's environment.
fn home_spawn(home: &CpdHome, argv: Vec<String>) -> Spawn {
    Spawn {
        cwd: Some(home.root().to_owned()),
        env: BTreeMap::from([("CPD_HOME".to_owned(), home.root().display().to_string())]),
        argv,
    }
}

/// Builds `<cockpit>:main` with its four panes, each marked with `@cpd_view`,
/// starting the server and the cockpit session when they are missing. A
/// cockpit session that exists without `main` (`plan new` creates it with
/// the first Lead's park window, commands.md §4.1) gets `main` as a new window.
fn build(tmux: &Tmux, home: &CpdHome, cockpit: &str) -> Result<(), CpdError> {
    let spawn = |view| view_spawn(home, view);
    let tree = match tmux.create_session(cockpit, MAIN_WINDOW, &spawn(View::Tree))? {
        Some((_, pane)) => pane,
        None => tmux.new_window(cockpit, MAIN_WINDOW, &spawn(View::Tree))?.1,
    };
    tmux.set_pane_view(&tree, View::Tree)?;
    let plan = tmux.split_window(&tree, Split::Right, Some("80%"), &spawn(View::Plan))?;
    tmux.set_pane_view(&plan, View::Plan)?;
    // 25 % of the centre's 80 % is 20 % of the window.
    let widgets = tmux.split_window(&plan, Split::Right, Some("25%"), &spawn(View::Widgets))?;
    tmux.set_pane_view(&widgets, View::Widgets)?;
    let chat = tmux.split_window(&plan, Split::Below, Some("80%"), &spawn(View::Chat))?;
    tmux.set_pane_view(&chat, View::Chat)?;
    reset_layout(tmux, cockpit)?;
    // The keyboard goes to the chat slot, as in the spike.
    tmux.run(&["select-pane".to_owned(), "-t".to_owned(), chat.as_str().to_owned()])?;
    Ok(())
}

/// An existing `main` that still has an older cpd's StatusBar pane loses it
/// once (`kill-pane`), then gets reset-layout, so the other panes take the
/// freed row and get their sizes back (commands.md §13.1, amendment 15).
fn remove_old_statusbar(tmux: &Tmux, cockpit: &str) -> Result<(), CpdError> {
    let old: Vec<PaneId> =
        main_views(tmux, cockpit)?.into_iter().filter(|(_, view)| view == LEGACY_STATUSBAR).map(|(pane, _)| pane).collect();
    if old.is_empty() {
        return Ok(());
    }
    for pane in &old {
        tmux.kill_pane(pane)?;
    }
    reset_layout(tmux, cockpit)
}

/// Each pane of `<cockpit>:main` with its raw `@cpd_view` (empty when unset).
/// Raw, because [`Tmux::list_panes`] skips a pane whose value is no [`View`]
/// (an older cpd's StatusBar, a newer cpd's view), and warns on stderr about a
/// newer one, which `cpd tui action` must never print.
fn main_views(tmux: &Tmux, cockpit: &str) -> Result<Vec<(PaneId, String)>, CpdError> {
    let args = ["list-panes", "-a", "-F", "#{pane_id}\t#{session_name}\t#{window_name}\t#{@cpd_view}"].map(str::to_owned);
    let out = tmux.run(&args)?;
    let mut panes = Vec::new();
    for line in out.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let [pane, session, window, view] = fields[..] else {
            return Err(unexpected_listing(tmux, line));
        };
        if session == cockpit && window == MAIN_WINDOW {
            panes.push((PaneId::parse(pane).ok_or_else(|| unexpected_listing(tmux, line))?, view.to_owned()));
        }
    }
    Ok(panes)
}

fn unexpected_listing(tmux: &Tmux, line: &str) -> CpdError {
    CpdError::new(ErrorCode::TmuxFailed, "tmux list-panes printed unexpected output.")
        .with("socket", tmux.socket())
        .with("stdout", line.to_owned())
}

/// Reset-layout (commands.md §13.2): the views resized, in order, with the
/// `resize-pane` flag and size of each. tmux takes a percentage as a share of
/// the window. The chat slot is never resized: it takes what is left.
const RESET_ORDER: [(View, &str, &str); 4] =
    [(View::Tree, "-x", "20%"), (View::Widgets, "-x", "20%"), (View::Plan, "-y", "20%"), (View::Worker, "-x", "30%")];

/// The ordered `resize-pane` arguments that reset the window, given each pane
/// as `(pane, view)`. The Worker's step is only there when a Worker pane is open.
fn reset_steps(panes: &[(PaneId, View)]) -> Vec<Vec<String>> {
    RESET_ORDER
        .iter()
        .filter_map(|(view, flag, size)| {
            let (id, _) = panes.iter().find(|(_, v)| v == view)?;
            Some(["resize-pane", "-t", id.as_str(), flag, size].map(String::from).to_vec())
        })
        .collect()
}

/// Resets the layout of `<cockpit>:main`: `cpd tui` runs it after building
/// `main`, `cpd tui action reset` on the StatusBar's ⧉ click.
fn reset_layout(tmux: &Tmux, cockpit: &str) -> Result<(), CpdError> {
    let panes: Vec<(PaneId, View)> =
        main_views(tmux, cockpit)?.into_iter().filter_map(|(pane, view)| Some((pane, View::parse(&view)?))).collect();
    for step in reset_steps(&panes) {
        tmux.run(&step)?;
    }
    Ok(())
}

/// What a StatusBar button does: the range names of contract §6.1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Reset,
    Legend,
    Logs,
}

impl Action {
    fn parse(s: &str) -> Option<Action> {
        match s {
            "reset" => Some(Action::Reset),
            "legend" => Some(Action::Legend),
            "logs" => Some(Action::Logs),
            _ => None,
        }
    }
}

/// `cpd tui action <reset|legend|logs> --client <name>` (commands.md §13.2):
/// runs the click of a StatusBar button on client `client`. It prints
/// nothing, on stdout or stderr, and always exits 0: tmux's `run-shell -b`
/// puts the active pane into view mode on a failure or any output. A failure
/// is shown with `display-message -c <client>` instead, and if even that
/// fails, it is dropped.
pub(super) fn action(name: Option<&str>, client: &str) -> Output {
    if let Err(e) = run_action(name, client) {
        if let Ok(tmux) = action_tmux() {
            let message = format!("cpd tui action: {}", e.message());
            // `-l`: the message is shown as is, not expanded as a format.
            let _ = tmux.run(&["display-message".to_owned(), "-l".to_owned(), "-c".to_owned(), client.to_owned(), message]);
        }
    }
    Output::Owned
}

/// The tmux server of the cockpit, from config `tmux.socket`.
fn action_tmux() -> Result<Tmux, CpdError> {
    Ok(Tmux::new(Config::load(&CpdHome::from_env()?)?.tmux.socket))
}

fn run_action(name: Option<&str>, client: &str) -> Result<(), CpdError> {
    let action = name.and_then(Action::parse).ok_or_else(|| {
        CpdError::new(ErrorCode::Usage, format!("unknown action {:?}: reset, legend or logs.", name.unwrap_or("")))
    })?;
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let tmux = Tmux::new(config.tmux.socket.clone());
    let popup = match action {
        Action::Reset => return reset_layout(&tmux, &config.tmux.cockpit_session),
        Action::Legend => legend::popup_args(client_size(&tmux, client)?),
        Action::Logs => logs::popup_args(client_size(&tmux, client)?, None),
    };
    tmux.run(&on_client(popup, client)).map(drop)
}

/// `(columns, rows)` of client `client` (`display -p -c`). tmux prints an
/// empty line, and exits 0, for a client that does not exist.
fn client_size(tmux: &Tmux, client: &str) -> Result<(u16, u16), CpdError> {
    let out = tmux.run(&["display", "-p", "-c", client, "#{client_width} #{client_height}"].map(str::to_owned))?;
    let size = out.trim().split_once(' ').and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)));
    size.ok_or_else(|| CpdError::new(ErrorCode::TmuxFailed, format!("no tmux client {client:?}.")).with("socket", tmux.socket()))
}

/// `display-popup …` aimed at `client` (`-c`), so it opens there only.
fn on_client(mut popup: Vec<String>, client: &str) -> Vec<String> {
    popup.splice(1..1, ["-c".to_owned(), client.to_owned()]);
    popup
}

/// `--respawn`: restarts every live pane of `main` that has `@cpd_view` (the
/// views, the chat placeholder and a Worker view) in place with `respawn-pane
/// -k`, so each runs the installed binary again; a Lead's or the engine's pane
/// is never touched. Each reruns its own command (a Worker view keeps its
/// Attempt), with `CPD_HOME` passed again since `-e` does not outlive a
/// respawn. Returns the panes restarted.
fn respawn_live_views(tmux: &Tmux, home: &CpdHome, main: &[Pane]) -> Result<Vec<PaneId>, CpdError> {
    let rerun = home_spawn(home, Vec::new());
    let views: Vec<PaneId> = main.iter().filter(|p| !p.dead && p.cpd_view.is_some()).map(|p| p.pane.clone()).collect();
    for pane in &views {
        tmux.respawn_pane_killing(pane, &rerun)?;
    }
    Ok(views)
}

/// Respawns each dead pane of `main`'s layout in place (`respawn-pane`,
/// amendment 9): never a duplicate, and its `@cpd_view` stays.
fn respawn_dead(tmux: &Tmux, home: &CpdHome, main: &[Pane]) -> Result<(), CpdError> {
    for pane in main.iter().filter(|p| p.dead) {
        if let Some(view) = pane.cpd_view.filter(|v| LAYOUT.contains(v)) {
            tmux.respawn_pane(&pane.pane, &view_spawn(home, view))?;
        }
    }
    Ok(())
}

/// The Lead to bring into a new chat slot: the Session of the newest
/// `session-moved` Event with `place: "chat"` among Leads with a Plan (what
/// `session open` accepts; Escalation `tui-cockpit-a1-q3`).
fn last_chat_lead(conn: &rusqlite::Connection) -> Result<Option<SessionId>, CpdError> {
    let id: Option<String> = conn
        .query_row(
            "SELECT e.session FROM events e JOIN sessions s ON s.id = e.session
              WHERE e.kind = 'session-moved' AND json_extract(e.data, '$.place') = 'chat'
                AND s.role = 'lead' AND s.plan IS NOT NULL
              ORDER BY e.id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    id.map(|id| {
        SessionId::parse(&id)
            .map_err(|e| CpdError::new(ErrorCode::StoreFailed, format!("The database holds an invalid Session id: {}", e.message())))
    })
    .transpose()
}

/// Moves `lead` into the chat slot as `session open` does, by running it. A
/// Lead `session open` refuses stays where it is and the placeholder keeps
/// the slot: the cockpit is built either way (Escalation `tui-cockpit-a1-q3`).
fn open_lead(run_command: RunCommand, lead: &SessionId) -> Result<(), CpdError> {
    match command(run_command, &["session", "open", lead.as_str()]) {
        Ok(_) => Ok(()),
        Err(e) if e.exit() == Exit::Refused => {
            eprintln!("cpd tui: Lead {lead} stays out of the chat slot: {}", e.message());
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Runs another area's command (`cpd <words>`) in this process, through the
/// command tree, and returns its success fields: its arguments and its effects
/// stay that area's own, as with a subprocess.
fn command(run_command: RunCommand, words: &[&str]) -> Result<Map<String, Value>, CpdError> {
    let argv = std::iter::once(PROGRAM).chain(words.iter().copied()).map(OsString::from).collect();
    match run_command(argv)? {
        Output::Success(fields) => Ok(fields),
        Output::Owned => unreachable!("cpd {words:?} prints one JSON value"),
    }
}

/// Replaces this process with `tmux -L <socket> attach -t <cockpit>`, run by
/// `program` (default: `tmux` on `PATH`); returns only when that fails.
fn exec_attach(tmux: &Tmux, cockpit: &str, program: Option<&Path>) -> CpdError {
    let argv = tmux.argv(&["attach".to_owned(), "-t".to_owned(), cockpit.to_owned()]);
    let program = program.map_or_else(|| PathBuf::from(&argv[0]), Path::to_path_buf);
    let error =
        std::process::Command::new(program).args(&argv[1..]).env_remove("TMUX").env_remove("TMUX_PANE").exec();
    CpdError::new(ErrorCode::TmuxFailed, format!("Cannot run tmux attach: {error}."))
        .with("argv", argv)
        .with("exit", Value::Null)
        .with("stderr", error.to_string())
}

/// `cpd` with no arguments (commands.md §13.3): the follow-ups of an update,
/// then attaches this terminal to the cockpit. Each follow-up's failure is one
/// line on stderr and the attach still happens when the cockpit exists (A3).
/// Inside the cockpit's own server it attaches nothing (A2).
pub(super) fn bare(run_command: RunCommand) -> Result<Output, CpdError> {
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    let tmux = Tmux::new(config.tmux.socket.clone());
    let cockpit = config.tmux.cockpit_session.as_str();
    let inside = std::env::var("TMUX").is_ok_and(|v| names_socket(&v, tmux.socket()));

    // 1. The engine.
    if let Err(e) = command(run_command, &["watch", "--ensure"]) {
        eprintln!("cpd: the engine did not start: {}", e.message());
    }
    // 2. The cockpit.
    let built = match ensure(&tmux, &home, &mut store, cockpit, false, run_command) {
        Ok((built, _)) => built,
        Err(e) if matches!(tmux.has_session(cockpit), Ok(true)) => {
            eprintln!("cpd: the cockpit is not complete: {}", e.message());
            false
        }
        Err(e) => return Err(e),
    };
    // 3. The views on an old binary (a view just built runs the installed one).
    if !built {
        let own = if inside { std::env::var("TMUX_PANE").ok() } else { None };
        if let Err(e) = respawn_old_views(&tmux, &home, cockpit, own.as_deref()) {
            eprintln!("cpd: the views were not restarted: {}", e.message());
        }
    }
    if inside {
        eprintln!("cpd: this terminal is inside the cockpit's tmux server: switch to the cockpit with `tmux switch-client -t {cockpit}`.");
        return Ok(Output::Owned);
    }
    // 4. The tmux version: an older server is restarted when it is safe, else the restart is marked
    // pending; the server's own tmux attaches while it runs another version than the installed one.
    let after = tmux_version(&home, &config, &mut store, &tmux);
    let program = server_tmux(&tmux, &after).unwrap_or_else(|e| {
        eprintln!("cpd: cannot compare the tmux server's version with the installed tmux: {}", e.message());
        None
    });
    Err(exec_attach(&tmux, cockpit, program.as_deref()))
}

/// The tmux version follow-up of bare `cpd` (commands.md §13.3): a server older than the installed
/// tmux is restarted now when it is safe, else the restart is marked pending (the engine holds new
/// Attempts, then restarts it, contract §6.1). Returns what to add to the attach line about a restart
/// still to come; a failure is one line on stderr.
fn tmux_version(home: &CpdHome, config: &Config, store: &mut Store, tmux: &Tmux) -> String {
    let (server, installed) = match (tmux.server_version(), installed_version()) {
        (Ok(server), Ok(installed)) => (server, installed),
        // `server_tmux` says why it cannot compare.
        _ => return String::new(),
    };
    if !older_version(&server, &installed) {
        // A restart that is no longer needed (done by hand, or by the engine): a failure it left is said once.
        if server == installed {
            match restart::read(home) {
                Ok(Some(pending)) if pending.failed.is_some() => eprintln!("cpd: {}", pending.describe()),
                Ok(_) => {}
                Err(e) => eprintln!("cpd: {}", e.message()),
            }
            if let Err(e) = restart::clear(home) {
                eprintln!("cpd: {}", e.message());
            }
        }
        return String::new();
    }
    match restart::restart(home, config, store, &mut |_| ()) {
        Ok(restart::Outcome::Restarted { reopened }) => {
            eprintln!("cpd: restarted the tmux server onto tmux {installed}; {reopened} Session(s) reopened.");
            String::new()
        }
        Ok(restart::Outcome::Busy(why)) => match restart::mark(home, &server, &installed) {
            Ok(_) => format!("; the restart happens once nothing is in the way ({})", why.join("; ")),
            Err(e) => {
                eprintln!("cpd: cannot mark the tmux restart pending: {}", e.message());
                String::new()
            }
        },
        Ok(restart::Outcome::Running) => "; a restart is running".to_owned(),
        Ok(restart::Outcome::NotNeeded) => String::new(),
        Err(e) => {
            eprintln!("cpd: the tmux restart failed: {} It is tried again at the next cpd.", e.message());
            String::new()
        }
    }
}

/// `cpd tui`: when the server runs an older tmux than the installed one, marks a restart pending.
/// Returns what to say on the attach line.
fn mark_if_older(home: &CpdHome, tmux: &Tmux) -> Result<Option<String>, CpdError> {
    let (server, installed) = (tmux.server_version()?, installed_version()?);
    if !older_version(&server, &installed) {
        return Ok(None);
    }
    restart::mark(home, &server, &installed)?;
    Ok(Some("; the restart happens once no Attempt runs".to_owned()))
}

/// Whether `$TMUX` (`<socket path>,<server pid>,<session>`) names the server on socket `socket`.
fn names_socket(tmux_var: &str, socket: &str) -> bool {
    let mut parts = tmux_var.rsplitn(3, ',');
    let path = parts.nth(2);
    path.is_some_and(|p| Path::new(p).file_name().is_some_and(|name| name == socket))
}

/// `--respawn` for the views that run another binary than the installed one
/// (the one that started this process): when any does, every live view is
/// restarted as `cpd tui --respawn` does it. The pane `skip` (the one this
/// process runs in, inside the cockpit) is never touched.
fn respawn_old_views(tmux: &Tmux, home: &CpdHome, cockpit: &str, skip: Option<&str>) -> Result<(), CpdError> {
    let installed = std::fs::metadata(started_as()?).map_err(|e| {
        CpdError::new(ErrorCode::SpawnFailed, format!("Cannot stat the installed cpd: {e}."))
    })?;
    let main: Vec<Pane> = main_panes(tmux, cockpit)?.into_iter().filter(|p| Some(p.pane.as_str()) != skip).collect();
    let old = main.iter().filter(|p| !p.dead && p.cpd_view.is_some()).any(|p| {
        // A pane whose binary cannot be read (not Linux, gone) is not known to be old.
        p.pid
            .and_then(|pid| std::fs::metadata(format!("/proc/{pid}/exe")).ok())
            .is_some_and(|exe| (exe.dev(), exe.ino()) != (installed.dev(), installed.ino()))
    });
    if old {
        respawn_live_views(tmux, home, &main)?;
    }
    Ok(())
}

/// The tmux program to attach with when the server runs another version than
/// the installed tmux (a client of another version cannot attach, hs-n224):
/// the server's own binary, with one line on stderr saying so; `after` ends the line.
fn server_tmux(tmux: &Tmux, after: &str) -> Result<Option<PathBuf>, CpdError> {
    let (server, installed) = (tmux.server_version()?, installed_version()?);
    if server == installed {
        return Ok(None);
    }
    let exe = tmux.server_exe()?;
    eprintln!("cpd: the tmux server runs tmux {server}, the installed tmux is {installed}: attaching with the server's own tmux{after}.");
    Ok(Some(exe))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panes(views: &[Option<View>]) -> Vec<(PaneId, View)> {
        views.iter().enumerate().filter_map(|(i, v)| Some((PaneId::parse(&format!("%{i}"))?, (*v)?))).collect()
    }

    fn joined(steps: Vec<Vec<String>>) -> Vec<String> {
        steps.iter().map(|s| s.join(" ")).collect()
    }

    #[test]
    fn the_reset_applies_the_sizes_in_order() {
        // %0 tree, %1 chat, %2 widgets, %3 plan
        let p = panes(&[Some(View::Tree), Some(View::Chat), Some(View::Widgets), Some(View::Plan)]);
        assert_eq!(joined(reset_steps(&p)), ["resize-pane -t %0 -x 20%", "resize-pane -t %2 -x 20%", "resize-pane -t %3 -y 20%"]);
    }

    #[test]
    fn an_open_worker_pane_is_resized_last_to_30_percent() {
        let p = panes(&[Some(View::Tree), Some(View::Chat), Some(View::Worker), Some(View::Widgets), Some(View::Plan)]);
        let s = joined(reset_steps(&p));
        assert_eq!(s.len(), 4);
        assert_eq!(s[3], "resize-pane -t %2 -x 30%");
    }

    #[test]
    fn the_actions_are_the_status_line_ranges() {
        assert_eq!(["reset", "legend", "logs"].map(Action::parse), [Some(Action::Reset), Some(Action::Legend), Some(Action::Logs)]);
        assert_eq!(Action::parse(""), None);
        assert_eq!(Action::parse("Reset"), None);
    }

    #[test]
    fn a_popup_opens_on_the_clicking_client() {
        let popup = on_client(legend::popup_args((215, 62)), "/dev/pts/4");
        assert_eq!(popup[..5], ["display-popup", "-c", "/dev/pts/4", "-E", "-T"]);
        assert!(popup.contains(&" legend ".to_owned()));
    }

    #[test]
    fn tmux_names_the_socket_of_its_server() {
        assert!(names_socket("/tmp/tmux-1000/cpd,4242,0", "cpd"));
        assert!(names_socket("/tmp/tmux-1000/cpd,4242,12", "cpd"));
        assert!(!names_socket("/tmp/tmux-1000/default,4242,0", "cpd"));
        assert!(!names_socket("/tmp/tmux-1000/cpd-work,4242,0", "cpd"));
        assert!(!names_socket("", "cpd"));
        assert!(!names_socket("cpd", "cpd"));
    }
}
