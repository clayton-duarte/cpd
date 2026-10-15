//! `cpd2 tui [--attach]` (commands.md §13.1; contract §6.1, §6.2, §6.4 with
//! amendment 9): builds the cockpit's `main` window when it is missing,
//! respawns a dead pane of its layout, sets the TUI's own server options,
//! starts the engine, and with `--attach` replaces itself with `tmux attach`.
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

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::process::CommandExt;

use rusqlite::OptionalExtension;
use serde_json::{Map, Value, json};

use super::{not_a_terminal, on_a_terminal};
use crate::config::Config;
use crate::error::{CpdError, ErrorCode, Exit};
use crate::home::CpdHome;
use crate::host::layout::MAIN_WINDOW;
use crate::host::{Pane, Spawn, Split, Tmux, View};
use crate::ids::SessionId;
use crate::output::Output;
use crate::session::PROGRAM;
use crate::store::Store;

/// The views of `main`'s layout: a dead one is respawned (amendment 9). The
/// `worker` view is not part of it: it is opened and closed by the plan view.
const LAYOUT: [View; 4] = [View::Tree, View::Plan, View::Widgets, View::Chat];

/// The pane border titles: the spike's column titles, keyed by `@cpd_view`.
/// A Lead's pane (`@cpd_session`) and the placeholder have none, since pi and
/// the placeholder say what they are; any other pane shows its command.
const BORDER_FORMAT: &str = "#{?#{@cpd_session},,#{?#{==:#{@cpd_view},chat},,\
#{?#{==:#{@cpd_view},tree}, Context ,#{?#{==:#{@cpd_view},plan}, Plan ,\
#{?#{==:#{@cpd_view},widgets}, Dispatch ,#{?#{==:#{@cpd_view},worker}, Worker (read-only) ,\
#{pane_current_command}}}}}}}";

/// The TUI's own global options, from the spike's `tmux.conf`, set on every
/// run (each is a plain `set -g`, so rerunning is safe). The server's own
/// options are contract §6.1's, set by `host` when it starts the server. Not
/// ported: the spike's `pane-died` respawn hook (it would also restart Lead
/// and engine panes, whose death the engine must see, E2) and its status-bar
/// mouse binding (cpd 2's cockpit has no status-bar pane).
const TUI_OPTIONS: &[[&str; 4]] = &[
    ["set", "-g", "focus-events", "on"],
    ["set", "-g", "pane-active-border-style", "fg=blue"],
    ["set", "-g", "pane-border-format", BORDER_FORMAT],
];

pub(super) fn run(attach: bool) -> Result<Output, CpdError> {
    if attach && !on_a_terminal() {
        return Err(not_a_terminal("tui --attach"));
    }
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    let tmux = Tmux::new(config.tmux.socket.clone());
    let cockpit = config.tmux.cockpit_session.as_str();

    let main: Vec<Pane> =
        tmux.list_panes()?.into_iter().filter(|p| p.session_name == cockpit && p.window_name == MAIN_WINDOW).collect();
    let built = main.is_empty();
    if built {
        build(&tmux, &home, cockpit)?;
    } else {
        respawn_dead(&tmux, &home, &main)?;
    }
    for option in TUI_OPTIONS {
        tmux.run(&option.map(str::to_owned))?;
    }
    if built {
        if let Some(lead) = store.read(last_chat_lead)? {
            open_lead(&lead)?;
        }
    }
    let watch = command(&["watch", "--ensure"])?;

    let attach_argv = ["tmux", "-L", tmux.socket(), "attach", "-t", cockpit];
    if attach {
        return Err(exec_attach(&tmux, cockpit));
    }
    Ok(Output::success(json!({
        "built": built, "socket": tmux.socket(), "session": cockpit, "attach": attach_argv, "watch": watch,
    })))
}

/// What a view's pane runs: `cpd2 tui view <view>` in `$CPD_HOME`, with
/// `CPD_HOME` passed with `-e`, since a server started by another process does
/// not have this process's environment.
fn view_spawn(home: &CpdHome, view: View) -> Spawn {
    Spawn {
        cwd: Some(home.root().to_owned()),
        env: BTreeMap::from([("CPD_HOME".to_owned(), home.root().display().to_string())]),
        argv: [PROGRAM, "tui", "view", view.as_str()].map(str::to_owned).into(),
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
    // The keyboard goes to the chat slot, as in the spike.
    tmux.run(&["select-pane".to_owned(), "-t".to_owned(), chat.as_str().to_owned()])?;
    Ok(())
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
fn open_lead(lead: &SessionId) -> Result<(), CpdError> {
    match command(&["session", "open", lead.as_str()]) {
        Ok(_) => Ok(()),
        Err(e) if e.exit() == Exit::Refused => {
            eprintln!("cpd2 tui: Lead {lead} stays out of the chat slot: {}", e.message());
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Runs another area's command (`cpd2 <words>`) in this process, through the
/// command tree, and returns its success fields: its arguments and its effects
/// stay that area's own, as with a subprocess.
fn command(words: &[&str]) -> Result<Map<String, Value>, CpdError> {
    let argv = std::iter::once(PROGRAM).chain(words.iter().copied()).map(OsString::from);
    match crate::cli::run(argv)? {
        Output::Success(fields) => Ok(fields),
        Output::Owned => unreachable!("cpd2 {words:?} prints one JSON value"),
    }
}

/// Replaces this process with `tmux -L <socket> attach -t <cockpit>`; returns
/// only when that fails.
fn exec_attach(tmux: &Tmux, cockpit: &str) -> CpdError {
    let argv = tmux.argv(&["attach".to_owned(), "-t".to_owned(), cockpit.to_owned()]);
    let error = std::process::Command::new(&argv[0]).args(&argv[1..]).env_remove("TMUX").env_remove("TMUX_PANE").exec();
    CpdError::new(ErrorCode::TmuxFailed, format!("Cannot run tmux attach: {error}."))
        .with("argv", argv)
        .with("exit", Value::Null)
        .with("stderr", error.to_string())
}
