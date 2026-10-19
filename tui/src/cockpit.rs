//! `cpd tui [--attach] [--respawn]` (commands.md §13.1; contract §6.1, §6.2,
//! §6.4 with amendment 9): builds the cockpit's `main` window when it is
//! missing, respawns a dead pane of its layout and adds each pane of it that
//! is gone, sets §6.1's server options again, with `--respawn` restarts every live view, starts the engine, and
//! with `--attach` replaces itself with `tmux attach`.
//!
//! The layout, from the spike's `cockpit.sh` (branch `cpd/cpd-ui`):
//!
//! ```text
//! +-------+-------------------+---------+
//! | tree  | plan (20 %)       | widgets |
//! | 20 %  +-------------------+  20 %   |
//! |       | chat slot (80 %)  |         |
//! +-------+-------------------+---------+
//! | StatusBar: one row across the width  |
//! +--------------------------------------+
//! ```
//!
//! A new `main` is reset to these sizes once it is built (A3); an existing one
//! is not, unless it lacked a pane of the layout: the panes added are followed
//! by the reset (amendment 15).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::process::CommandExt;

use rusqlite::OptionalExtension;
use serde_json::{Map, Value, json};

use super::{RunCommand, not_a_terminal, on_a_terminal, statusbar};
use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode, Exit};
use cpd_core::home::CpdHome;
use cpd_core::host::layout::{MAIN_WINDOW, chat_slot, view_pane};
use cpd_core::host::{Pane, PaneId, Spawn, Split, Tmux, View};
use cpd_core::ids::SessionId;
use cpd_core::output::Output;
use cpd_core::session::PROGRAM;
use cpd_core::store::Store;

/// The views of `main`'s layout: a dead one is respawned (amendment 9). The
/// `worker` view is not part of it: it is opened and closed by the plan view.
const LAYOUT: [View; 5] = [View::Tree, View::Plan, View::Widgets, View::Chat, View::Statusbar];

pub(super) fn run(attach: bool, respawn: bool, run_command: RunCommand) -> Result<Output, CpdError> {
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
    let mut respawned = Vec::new();
    let mut chat_added = built;
    if built {
        build(&tmux, &home, cockpit)?;
    } else {
        respawn_dead(&tmux, &home, &main)?;
        if respawn {
            respawned = respawn_live_views(&tmux, &home, &main)?;
        }
        chat_added = add_missing_panes(&tmux, &home, cockpit, &main)?.contains(&View::Chat);
    }
    tmux.apply_server_options()?;
    if chat_added {
        if let Some(lead) = store.read(last_chat_lead)? {
            open_lead(run_command, &lead)?;
        }
    }
    let watch = command(run_command, &["watch", "--ensure"])?;

    let attach_argv = ["tmux", "-L", tmux.socket(), "attach", "-t", cockpit];
    if attach {
        return Err(exec_attach(&tmux, cockpit));
    }
    let mut fields = json!({
        "built": built, "socket": tmux.socket(), "session": cockpit, "attach": attach_argv, "watch": watch,
    });
    if respawn {
        fields["respawned"] = json!(respawned.iter().map(PaneId::as_str).collect::<Vec<_>>());
    }
    Ok(Output::success(fields))
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

/// Builds `<cockpit>:main` with its five panes, each marked with `@cpd_view`,
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
    add_statusbar(tmux, home, &tree)?;
    statusbar::layout::reset(tmux, cockpit)?;
    // The keyboard goes to the chat slot, as in the spike.
    tmux.run(&["select-pane".to_owned(), "-t".to_owned(), chat.as_str().to_owned()])?;
    Ok(())
}

/// Adds the StatusBar: a one-row pane across the full width at the bottom of
/// the window of `target`.
fn add_statusbar(tmux: &Tmux, home: &CpdHome, target: &PaneId) -> Result<(), CpdError> {
    let bar = tmux.split_window(target, Split::Bottom, Some("1"), &view_spawn(home, View::Statusbar))?;
    tmux.set_pane_view(&bar, View::Statusbar)
}

/// Adds each pane of the layout that `main` lacks (the User killed it, or a
/// build stopped half-way), each where it goes, then reset-layout once so the
/// panes added are taken from the layout and the others get their sizes back
/// (amendment 15). The chat slot counts as present when it holds a Lead. Adds
/// nothing to a layout that is whole. Returns the views added.
fn add_missing_panes(tmux: &Tmux, home: &CpdHome, cockpit: &str, main: &[Pane]) -> Result<Vec<View>, CpdError> {
    let Some(any) = main.first() else { return Ok(Vec::new()) };
    let found = |view| view_pane(main, cockpit, view).map(|p| p.pane.clone());
    let mut added = Vec::new();
    let mut add = |view: View, target: &PaneId, flags: &[&str], size: Option<&str>| -> Result<PaneId, CpdError> {
        let pane = split(tmux, home, view, target, flags, size)?;
        added.push(view);
        Ok(pane)
    };
    // The tree and the widgets span the window's height: `-f` puts them at its edges.
    let tree = match found(View::Tree) {
        Some(tree) => tree,
        None => add(View::Tree, &any.pane, &["-h", "-b", "-f"], Some("20%"))?,
    };
    let widgets = match found(View::Widgets) {
        Some(widgets) => widgets,
        None => add(View::Widgets, &any.pane, &["-h", "-f"], Some("20%"))?,
    };
    let (plan, slot) = (found(View::Plan), chat_slot(main, cockpit).map(|p| p.pane.clone()));
    match (plan, slot) {
        (Some(_), Some(_)) => {}
        (None, Some(slot)) => drop(add(View::Plan, &slot, &["-v", "-b"], Some("20%"))?),
        (Some(plan), None) => drop(add(View::Chat, &plan, &["-v"], Some("80%"))?),
        (None, None) => {
            let plan = add(View::Plan, &tree, &["-h"], None)?;
            add(View::Chat, &plan, &["-v"], Some("80%"))?;
        }
    }
    match found(View::Statusbar) {
        None => drop(add(View::Statusbar, &any.pane, &["-v", "-f"], Some("1"))?),
        // A tree or widgets added across the whole height has cut the row short: it spans the width again.
        Some(bar) if added.iter().any(|v| matches!(v, View::Tree | View::Widgets)) => {
            let anchor = if added.contains(&View::Tree) { tree } else { widgets };
            let args = ["join-pane", "-d", "-f", "-v", "-l", "1", "-s", bar.as_str(), "-t", anchor.as_str()];
            tmux.run(&args.map(str::to_owned))?;
        }
        Some(_) => {}
    }
    if !added.is_empty() {
        statusbar::layout::reset(tmux, cockpit)?;
    }
    Ok(added)
}

/// `split-window` with the flags the layout needs, which [`Tmux::split_window`] has no [`Split`] for, running
/// `cpd tui view <view>` as [`view_spawn`] does, with `@cpd_view` set. Returns the new pane.
fn split(tmux: &Tmux, home: &CpdHome, view: View, target: &PaneId, flags: &[&str], size: Option<&str>) -> Result<PaneId, CpdError> {
    let spawn = view_spawn(home, view);
    let mut args: Vec<String> = ["split-window", "-d", "-t", target.as_str()].map(str::to_owned).into();
    args.extend(flags.iter().map(|f| (*f).to_owned()));
    if let Some(size) = size {
        args.extend(["-l".to_owned(), size.to_owned()]);
    }
    args.extend(["-c".to_owned(), home.root().display().to_string()]);
    for (key, value) in &spawn.env {
        args.extend(["-e".to_owned(), format!("{key}={value}")]);
    }
    args.extend(["-P", "-F", "#{pane_id}", "--"].map(str::to_owned));
    args.extend(spawn.argv);
    let out = tmux.run(&args)?;
    let pane = PaneId::parse(out.trim_end_matches('\n')).ok_or_else(|| {
        CpdError::new(ErrorCode::TmuxFailed, format!("tmux split-window printed {out:?}, not a pane id."))
    })?;
    tmux.set_pane_view(&pane, view)?;
    Ok(pane)
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
