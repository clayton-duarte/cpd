//! `cpd tui [--attach] [--respawn]` (commands.md §13.1; contract §6.1, §6.2,
//! §6.4 with amendment 9): builds the cockpit's `main` window when it is
//! missing, respawns a dead pane of its layout and adds each pane of it that is
//! gone, sets §6.1's server options and the cockpit session's status line (the StatusBar) again, with
//! `--respawn` restarts every live view, starts the engine, and with
//! `--attach` replaces itself with `tmux attach`. When the server runs an older tmux than the
//! installed one it marks a restart pending (contract §6.1; the engine's restart, passed in as [`Restart`]).
//! Also reset-layout (commands.md §13.2), and `cpd tui action`, what a click on a StatusBar
//! button runs.
//!
//! The layout, from the spike's `cockpit.sh` (branch `cpd/cpd-ui`):
//!
//! ```text
//! +-------+-------------------+---------+
//! | tree  | plan (25 %)       | widgets |
//! | 25 %  +-------------------+  25 %   |
//! |       | chat slot (75 %)  |         |
//! +-------+-------------------+---------+
//! ```
//!
//! Below it, tmux's status line of the cockpit session is the StatusBar
//! (contract §6.1); it has no pane.
//!
//! A new `main` is reset to these sizes once it is built (A3); an existing one
//! is not, unless it lacked a pane of the layout or still had an older cpd's
//! StatusBar pane: the panes added, or the kill, are followed by the reset
//! (amendment 15).

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use rusqlite::OptionalExtension;
use serde_json::{Map, Value, json};

use super::view_ports::RunFn;
use super::widgets::legend;
use super::{
    Restart, RestartOutcome, RunCommand, RunOn, logs, merge, not_a_terminal, on_a_terminal,
};
use cpd_core::config::Config;
use cpd_core::error::{CpdError, ErrorCode, Exit};
use cpd_core::home::CpdHome;
use cpd_core::host::exe::started_as;
use cpd_core::host::layout::{LEGACY_VIEWS, MAIN_WINDOW, chat_slot, view_pane};
use cpd_core::host::spawn::Launch;
use cpd_core::host::tmux::{client, installed_version, installed_version_with, older_version};
use cpd_core::host::{Pane, PaneId, Spawn, Split, Tmux, View};
use cpd_core::ids::SessionId;
use cpd_core::output::Output;
use cpd_core::ports::Ports;
use cpd_core::session::PROGRAM;
use cpd_core::store::Store;

/// The views of `main`'s layout: a dead one is respawned (amendment 9).
const LAYOUT: [View; 4] = [View::Tree, View::Plan, View::Widgets, View::Chat];

pub(super) fn run(
    attach: bool,
    respawn: bool,
    version: &str,
    run_command: RunCommand,
    restart: Restart,
) -> Result<Output, CpdError> {
    if attach && !on_a_terminal() {
        return Err(not_a_terminal("tui --attach"));
    }
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    let tmux = Tmux::new(config.tmux.socket.clone());
    let cockpit = config.tmux.cockpit_session.as_str();

    let (built, respawned) = ensure(
        &tmux,
        &home,
        &mut store,
        cockpit,
        config.machine.as_str(),
        respawn,
        version,
        run_command,
    )?;
    let watch = command(&run_command, &["watch", "--ensure"])?;
    if respawn && let Some(preview) = tmux.preview()? {
        eprintln!(
            "a preview is on ({preview}); its views keep the preview build until 'preview-tui.sh off'"
        );
    }
    // An older server is restarted by the engine once nothing is in the way (contract §6.1).
    let after = match mark_if_older(&home, &tmux, restart) {
        Ok(Some(why)) => {
            if !attach {
                eprintln!(
                    "cpd tui: the tmux server runs an older tmux than the installed one{why}."
                );
            }
            why
        }
        Ok(None) => String::new(),
        Err(e) => {
            eprintln!(
                "cpd tui: cannot compare the tmux server's version with the installed tmux: {}",
                e.message()
            );
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
/// removes an older cpd's StatusBar pane; sets the server options, the
/// status line and `@cpd_machine` again; brings the last chat
/// Lead into a new chat slot. Returns whether it built `main` and the panes
/// restarted by `respawn`.
#[allow(clippy::too_many_arguments)] // takes every input of the ensure step; a signature change is out of scope
fn ensure(
    tmux: &Tmux,
    home: &CpdHome,
    store: &mut Store,
    cockpit: &str,
    machine: &str,
    respawn: bool,
    version: &str,
    run_command: RunCommand,
) -> Result<(bool, Vec<PaneId>), CpdError> {
    let main = main_panes(tmux, cockpit)?;
    let built = main.is_empty();
    let mut respawned = Vec::new();
    let mut chat_added = built;
    if built {
        build(tmux, home, cockpit)?;
    } else {
        respawn_dead(tmux, home, &main)?;
        if respawn {
            respawned = respawn_live_views(tmux, home, &main)?;
        }
        chat_added = add_missing_panes(tmux, home, cockpit, &main)?.contains(&View::Chat);
        remove_old_panes(tmux, cockpit)?;
    }
    tmux.apply_server_options()?;
    // The window title's machine name (contract §6.1, hs-d300): SERVER_OPTIONS is static.
    tmux.run(&[
        "set".to_owned(),
        "-g".to_owned(),
        "@cpd_machine".to_owned(),
        machine.to_owned(),
    ])?;
    tmux.apply_status_line(cockpit, version)?;
    if chat_added && let Some(lead) = store.read(last_chat_lead)? {
        open_lead(run_command, &lead)?;
    }
    Ok((built, respawned))
}

/// The panes of `<cockpit>:main`.
fn main_panes(tmux: &Tmux, cockpit: &str) -> Result<Vec<Pane>, CpdError> {
    Ok(tmux
        .list_panes()?
        .into_iter()
        .filter(|p| p.session_name == cockpit && p.window_name == MAIN_WINDOW)
        .collect())
}

/// What a view's pane runs: `cpd tui view <view>` (see [`home_spawn`]).
fn view_spawn(home: &CpdHome, view: View) -> Spawn {
    home_spawn(
        home,
        [PROGRAM, "tui", "view", view.as_str()]
            .map(str::to_owned)
            .into(),
    )
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
    let plan = tmux.split_window(&tree, Split::Right, None, &spawn(View::Plan))?;
    tmux.set_pane_view(&plan, View::Plan)?;
    let widgets = tmux.split_window(&plan, Split::Right, None, &spawn(View::Widgets))?;
    tmux.set_pane_view(&widgets, View::Widgets)?;
    let chat = tmux.split_window(&plan, Split::Below, None, &spawn(View::Chat))?;
    tmux.set_pane_view(&chat, View::Chat)?;
    reset_layout(tmux, cockpit)?;
    // The keyboard goes to the chat slot, as in the spike.
    tmux.run(&[
        "select-pane".to_owned(),
        "-t".to_owned(),
        chat.as_str().to_owned(),
    ])?;
    Ok(())
}

/// Adds each pane of the layout that `main` lacks (the User killed it, or a
/// build stopped half-way), each where it goes, then reset-layout once so the
/// panes added are taken from the layout and the others get their sizes back
/// (amendment 15). The chat slot counts as present when it holds a Lead. Adds
/// nothing to a layout that is whole. Returns the views added.
fn add_missing_panes(
    tmux: &Tmux,
    home: &CpdHome,
    cockpit: &str,
    main: &[Pane],
) -> Result<Vec<View>, CpdError> {
    let Some(any) = main.first() else {
        return Ok(Vec::new());
    };
    let found = |view| view_pane(main, cockpit, view).map(|p| p.pane.clone());
    let mut added = Vec::new();
    let mut add = |view: View,
                   target: &PaneId,
                   flags: &[&str],
                   size: Option<&str>|
     -> Result<PaneId, CpdError> {
        let pane = split(tmux, home, view, target, flags, size)?;
        added.push(view);
        Ok(pane)
    };
    // The tree and the widgets span the window's height: `-f` puts them at its edges.
    let tree = match found(View::Tree) {
        Some(tree) => tree,
        None => add(View::Tree, &any.pane, &["-h", "-b", "-f"], Some(TREE_WIDTH))?,
    };
    if found(View::Widgets).is_none() {
        add(View::Widgets, &any.pane, &["-h", "-f"], Some(WIDGETS_WIDTH))?;
    }
    let (plan, slot) = (
        found(View::Plan),
        chat_slot(main, cockpit).map(|p| p.pane.clone()),
    );
    match (plan, slot) {
        (Some(_), Some(_)) => {}
        (None, Some(slot)) => drop(add(View::Plan, &slot, &["-v", "-b"], Some(PLAN_HEIGHT))?),
        (Some(plan), None) => drop(add(View::Chat, &plan, &["-v"], None)?),
        (None, None) => {
            let plan = add(View::Plan, &tree, &["-h"], None)?;
            add(View::Chat, &plan, &["-v"], None)?;
        }
    }
    if !added.is_empty() {
        reset_layout(tmux, cockpit)?;
    }
    Ok(added)
}

/// `split-window` with the flags the layout needs, which [`Tmux::split_window`] has no [`Split`] for, running
/// `cpd tui view <view>` as [`view_spawn`] does, with `@cpd_view` set. Returns the new pane.
fn split(
    tmux: &Tmux,
    home: &CpdHome,
    view: View,
    target: &PaneId,
    flags: &[&str],
    size: Option<&str>,
) -> Result<PaneId, CpdError> {
    let spawn = view_spawn(home, view);
    let mut args: Vec<String> = ["split-window", "-d", "-t", target.as_str()]
        .map(str::to_owned)
        .into();
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
        CpdError::new(
            ErrorCode::TmuxFailed,
            format!("tmux split-window printed {out:?}, not a pane id."),
        )
    })?;
    tmux.set_pane_view(&pane, view)?;
    Ok(pane)
}

/// An existing `main` that still has an older cpd's StatusBar or Worker pane
/// loses it once (`kill-pane`), then gets reset-layout, so the other panes take the
/// freed row and get their sizes back (commands.md §13.1, amendment 15).
fn remove_old_panes(tmux: &Tmux, cockpit: &str) -> Result<(), CpdError> {
    let old: Vec<PaneId> = main_views(tmux, cockpit)?
        .into_iter()
        .filter(|(_, view)| LEGACY_VIEWS.contains(&view.as_str()))
        .map(|(pane, _)| pane)
        .collect();
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
    let args = [
        "list-panes",
        "-a",
        "-F",
        "#{pane_id}\t#{session_name}\t#{window_name}\t#{@cpd_view}",
    ]
    .map(str::to_owned);
    let out = tmux.run(&args)?;
    let mut panes = Vec::new();
    for line in out.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let [pane, session, window, view] = fields[..] else {
            return Err(unexpected_listing(tmux, line));
        };
        if session == cockpit && window == MAIN_WINDOW {
            panes.push((
                PaneId::parse(pane).ok_or_else(|| unexpected_listing(tmux, line))?,
                view.to_owned(),
            ));
        }
    }
    Ok(panes)
}

fn unexpected_listing(tmux: &Tmux, line: &str) -> CpdError {
    CpdError::new(
        ErrorCode::TmuxFailed,
        "tmux list-panes printed unexpected output.",
    )
    .with("socket", tmux.socket())
    .with("stdout", line.to_owned())
}

/// The tree's width on reset.
const TREE_WIDTH: &str = "20%";
/// The widgets' width on reset.
const WIDGETS_WIDTH: &str = "30%";
/// The Plan view's height on reset.
const PLAN_HEIGHT: &str = "30%";

/// Reset-layout (commands.md §13.2): the views resized, in order, with the
/// `resize-pane` flag and size of each. tmux takes a percentage as a share of
/// the window. The chat slot is never resized: it takes what is left.
const RESET_ORDER: [(View, &str, &str); 3] = [
    (View::Tree, "-x", TREE_WIDTH),
    (View::Widgets, "-x", WIDGETS_WIDTH),
    (View::Plan, "-y", PLAN_HEIGHT),
];

/// The ordered `resize-pane` arguments that reset the window, given each pane
/// as `(pane, view)`.
fn reset_steps(panes: &[(PaneId, View)]) -> Vec<Vec<String>> {
    RESET_ORDER
        .iter()
        .filter_map(|(view, flag, size)| {
            let (id, _) = panes.iter().find(|(_, v)| v == view)?;
            Some(
                ["resize-pane", "-t", id.as_str(), flag, size]
                    .map(String::from)
                    .to_vec(),
            )
        })
        .collect()
}

/// Resets the layout of `<cockpit>:main`: `cpd tui` runs it after building
/// `main`, `cpd tui action reset` on the StatusBar's 󰨝 (U+F0A1D) click.
fn reset_layout(tmux: &Tmux, cockpit: &str) -> Result<(), CpdError> {
    let panes: Vec<(PaneId, View)> = main_views(tmux, cockpit)?
        .into_iter()
        .filter_map(|(pane, view)| Some((pane, View::parse(&view)?)))
        .collect();
    for step in reset_steps(&panes) {
        tmux.run(&step)?;
    }
    Ok(())
}

/// What a `cpd tui action` does: a StatusBar button's (the range names of
/// contract §6.1), or a merge started at once (hs-d473).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Action {
    Reset,
    Legend,
    Logs,
    Costs,
    Runner,
    Merge,
    Host,
    Incident,
}

impl Action {
    fn parse(s: &str) -> Option<Action> {
        match s {
            "reset" => Some(Action::Reset),
            "legend" => Some(Action::Legend),
            "logs" => Some(Action::Logs),
            "costs" => Some(Action::Costs),
            "runner" => Some(Action::Runner),
            "merge" => Some(Action::Merge),
            "host" => Some(Action::Host),
            "incident" => Some(Action::Incident),
            _ => None,
        }
    }
}

/// `cpd tui action <reset|legend|logs|costs|runner|host|incident> --client <name>` and `cpd tui action
/// merge <owner>/<repo>#<n> --client <name>` (commands.md §13.2): runs the
/// click of a StatusBar button, or the merge a click started, on
/// client `client`. It prints nothing, on stdout or stderr, and always exits
/// 0: tmux's `run-shell -b` puts the active pane into view mode on a failure
/// or any output. A failure is shown with `display-message -c <client>`
/// instead, and if even that fails, it is dropped (a failed merge is first
/// the merge action's own, [`merge::action`]).
pub(super) fn action(
    ports: &Ports,
    name: Option<&str>,
    pull: Option<&str>,
    client: &str,
    run_command: RunCommand,
    run_on: RunOn,
) -> Output {
    if let Err(e) = run_action(ports, name, pull, client, run_command, run_on) {
        {
            let tmux = failure_tmux(ports);
            let message = format!("cpd tui action: {}", e.message());
            // `-l`: the message is shown as is, not expanded as a format.
            let _ = tmux.run(&[
                "display-message".to_owned(),
                "-l".to_owned(),
                "-c".to_owned(),
                client.to_owned(),
                message,
            ]);
        }
    }
    Output::Owned
}

/// The tmux server of the cockpit, from config `tmux.socket`.
fn action_tmux(ports: &Ports) -> Result<Tmux, CpdError> {
    Ok(Tmux::new(Config::load(&CpdHome::from_env()?)?.tmux.socket)
        .with_runner(ports.runner.clone()))
}

/// The tmux a failure is shown on when the config does not parse (so names no socket): the default socket `cpd`.
fn failure_tmux(ports: &Ports) -> Tmux {
    action_tmux(ports)
        .unwrap_or_else(|_| Tmux::new("cpd".to_owned()).with_runner(ports.runner.clone()))
}

fn run_action(
    ports: &Ports,
    name: Option<&str>,
    pull: Option<&str>,
    client: &str,
    run_command: RunCommand,
    run_on: RunOn,
) -> Result<(), CpdError> {
    let action = name.and_then(Action::parse).ok_or_else(|| {
        CpdError::new(
            ErrorCode::Usage,
            format!(
                "unknown action {:?}: reset, legend, logs, costs, runner, host or incident.",
                name.unwrap_or("")
            ),
        )
    })?;
    let home = CpdHome::from_env()?;
    // The runner click reads the config itself: a config that does not parse is its error, not a failed load here.
    if let (Action::Runner, None) = (action, pull) {
        let tmux = failure_tmux(ports);
        return toggle_runner(&home, &tmux, client);
    }
    let config = Config::load(&home)?;
    let tmux = Tmux::new(config.tmux.socket.clone()).with_runner(ports.runner.clone());
    let popup = match (action, pull) {
        (Action::Merge, Some(pull)) => {
            let pull = merge::Pull::parse(pull).map_err(|e| CpdError::new(ErrorCode::Usage, e))?;
            return merge::action_with(ports, &home, &tmux, &pull, client, &|argv| {
                run_on(ports, argv)
            });
        }
        (Action::Merge, None) => {
            return Err(CpdError::new(
                ErrorCode::Usage,
                "merge needs <owner>/<repo>#<n>.",
            ));
        }
        (_, Some(extra)) => {
            return Err(CpdError::new(
                ErrorCode::Usage,
                format!("unexpected argument {extra:?}."),
            ));
        }
        (Action::Host, None) => {
            return super::host_click::run(&home, &config, &tmux, client, run_command);
        }
        (Action::Incident, None) => {
            return super::incident_click::run(&home, &config, &tmux, run_command);
        }
        (Action::Reset, None) => return reset_layout(&tmux, &config.tmux.cockpit_session),
        (Action::Legend, None) => legend::popup_args(client_size(&tmux, client)?),
        (Action::Costs, None) => return toggle_costs(&tmux, &config.tmux.cockpit_session),
        (Action::Runner, None) => unreachable!("handled above"),
        (Action::Logs, None) => logs::popup_args(client_size(&tmux, client)?, None),
    };
    tmux.run(&on_client(popup, client)).map(drop)
}

/// The runner pill (pro-d199): flips `worker_runner` in config.toml, sets `@cpd_runner` at once and redraws the client.
fn toggle_runner(home: &CpdHome, tmux: &Tmux, client: &str) -> Result<(), CpdError> {
    let to = cpd_core::config::toggle(home)?;
    tmux.run(&[
        "set".to_owned(),
        "-g".to_owned(),
        cpd_core::host::tmux::RUNNER_OPTION.to_owned(),
        to.as_str().to_owned(),
    ])?;
    tmux.run(&["refresh-client", "-S", "-c", client].map(str::to_owned))
        .map(drop)
}

/// The costs button (pro-d189, pro-d196): sends the literal key `$` to the live tree pane, whose view flips its cost
/// flag. With no live tree pane it fails like every failed action.
fn toggle_costs(tmux: &Tmux, cockpit: &str) -> Result<(), CpdError> {
    let live: Vec<Pane> = main_panes(tmux, cockpit)?
        .into_iter()
        .filter(|p| !p.dead)
        .collect();
    let pane = view_pane(&live, cockpit, View::Tree)
        .ok_or_else(|| CpdError::new(ErrorCode::TmuxFailed, "the tree view is not running"))?;
    tmux.run(&[
        "send-keys".to_owned(),
        "-t".to_owned(),
        pane.pane.to_string(),
        "-l".to_owned(),
        "$".to_owned(),
    ])
    .map(drop)
}

/// `(columns, rows)` of client `client` (`display -p -c`). tmux prints an
/// empty line, and exits 0, for a client that does not exist.
pub(crate) fn client_size(tmux: &Tmux, client: &str) -> Result<(u16, u16), CpdError> {
    let out = tmux.run(
        &[
            "display",
            "-p",
            "-c",
            client,
            "#{client_width} #{client_height}",
        ]
        .map(str::to_owned),
    )?;
    let size = out
        .trim()
        .split_once(' ')
        .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)));
    size.ok_or_else(|| {
        CpdError::new(ErrorCode::TmuxFailed, format!("no tmux client {client:?}."))
            .with("socket", tmux.socket())
    })
}

/// `display-popup …` aimed at `client` (`-c`), so it opens there only. The popup's argument vector stays as it
/// was, with `-c` at index 1; the shared builder (`crate::popup::args`) places `-c` after `-B` for the merge, so
/// the splice cannot move to the builder without changing the legend's, Logs' and the dialogs' vectors.
pub(crate) fn on_client(mut popup: Vec<String>, client: &str) -> Vec<String> {
    popup.splice(1..1, ["-c".to_owned(), client.to_owned()]);
    popup
}

/// `--respawn`: restarts every live pane of `main` that has `@cpd_view` (the
/// views and the chat placeholder) in place with `respawn-pane
/// -k`, so each runs the installed binary again; a Lead's or the engine's pane
/// is never touched. Each reruns its own command, with `CPD_HOME` passed again since `-e` does not outlive a
/// respawn. Returns the panes restarted.
fn respawn_live_views(tmux: &Tmux, home: &CpdHome, main: &[Pane]) -> Result<Vec<PaneId>, CpdError> {
    let rerun = home_spawn(home, Vec::new());
    let views: Vec<PaneId> = main
        .iter()
        .filter(|p| !p.dead && p.cpd_view.is_some())
        .map(|p| p.pane.clone())
        .collect();
    // One at a time; a pane that fails does not stop the others (hs-d463): the views that could be restarted
    // are better than views that all keep the old binary. The command then fails after the last.
    let mut restarted = Vec::new();
    let mut failed = Vec::new();
    let mut first = None;
    for pane in &views {
        match tmux.respawn_pane_killing(pane, &rerun) {
            Ok(()) => restarted.push(pane.as_str()),
            Err(e) => {
                failed.push(pane.as_str());
                first.get_or_insert(e);
            }
        }
    }
    match first {
        None => Ok(views),
        Some(e) => Err(CpdError::new(
            ErrorCode::TmuxFailed,
            format!(
                "{} of {} views were not restarted: {}",
                failed.len(),
                views.len(),
                e.message()
            ),
        )
        .with("failed", failed)
        .with("respawned", restarted)),
    }
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
        SessionId::parse(&id).map_err(|e| {
            CpdError::new(
                ErrorCode::StoreFailed,
                format!("The database holds an invalid Session id: {}", e.message()),
            )
        })
    })
    .transpose()
}

/// Moves `lead` into the chat slot as `session open` does, by running it. A
/// Lead `session open` refuses stays where it is and the placeholder keeps
/// the slot: the cockpit is built either way (Escalation `tui-cockpit-a1-q3`).
fn open_lead(run_command: RunCommand, lead: &SessionId) -> Result<(), CpdError> {
    open_lead_with(&run_command, lead)
}

/// [`open_lead`] with the command run by `run`.
pub(crate) fn open_lead_with(run: RunFn, lead: &SessionId) -> Result<(), CpdError> {
    match command(run, &["session", "open", lead.as_str()]) {
        Ok(_) => Ok(()),
        Err(e) if e.exit() == Exit::Refused => {
            eprintln!(
                "cpd tui: Lead {lead} stays out of the chat slot: {}",
                e.message()
            );
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Runs another area's command (`cpd <words>`) in this process, through the
/// command tree, and returns its success fields: its arguments and its effects
/// stay that area's own, as with a subprocess.
fn command(run: RunFn, words: &[&str]) -> Result<Map<String, Value>, CpdError> {
    let argv = std::iter::once(PROGRAM)
        .chain(words.iter().copied())
        .map(OsString::from)
        .collect();
    match run(argv)? {
        Output::Success(fields) => Ok(fields),
        Output::Owned => unreachable!("cpd {words:?} prints one JSON value"),
    }
}

/// Replaces this process with `tmux -L <socket> attach -t <cockpit>`, run by
/// `program` (default: `tmux` on `PATH`); returns only when that fails.
fn exec_attach(tmux: &Tmux, cockpit: &str, program: Option<&Path>) -> CpdError {
    exec_attach_with(&Ports::current(), tmux, cockpit, program)
}

/// [`exec_attach`] through `ports.spawn.exec`.
pub(crate) fn exec_attach_with(
    ports: &Ports,
    tmux: &Tmux,
    cockpit: &str,
    program: Option<&Path>,
) -> CpdError {
    let argv = tmux.argv(&["attach".to_owned(), "-t".to_owned(), cockpit.to_owned()]);
    let program = program.map_or_else(|| PathBuf::from(&argv[0]), Path::to_path_buf);
    // `client` puts `-u` first and removes the variables of the cockpit (hs-d463); `argv` is the same line, for the error.
    // The attach is interactive: `exec` keeps this process's terminal as its stdin.
    let mut tmux_client = client(&program);
    tmux_client.args(&argv[2..]);
    let launch = Launch {
        program: program.to_string_lossy().into_owned(),
        args: tmux_client
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect(),
        env_remove: tmux_client
            .get_envs()
            .filter(|(_, value)| value.is_none())
            .map(|(key, _)| key.to_string_lossy().into_owned())
            .collect(),
        ..Launch::default()
    };
    let error = ports.spawn.exec(&launch);
    CpdError::new(
        ErrorCode::TmuxFailed,
        format!("Cannot run tmux attach: {error}."),
    )
    .with("argv", argv)
    .with("exit", Value::Null)
    .with("stderr", error.to_string())
}

/// `cpd` with no arguments (commands.md §13.3): the follow-ups of an update,
/// then attaches this terminal to the cockpit. Each follow-up's failure is one
/// line on stderr and the attach still happens when the cockpit exists (A3).
/// Inside the cockpit's own server it attaches nothing (A2).
pub(super) fn bare(
    version: &str,
    run_command: RunCommand,
    restart: Restart,
) -> Result<Output, CpdError> {
    let home = CpdHome::from_env()?;
    let config = Config::load(&home)?;
    let mut store = Store::open(&home, u64::from(config.timeouts.store_busy_ms))?;
    let tmux = Tmux::new(config.tmux.socket.clone());
    let cockpit = config.tmux.cockpit_session.as_str();
    let inside = std::env::var("TMUX").is_ok_and(|v| names_socket(&v, tmux.socket()));

    // 1. The engine.
    if let Err(e) = command(&run_command, &["watch", "--ensure"]) {
        eprintln!("cpd: the engine did not start: {}", e.message());
    }
    // 2. The cockpit.
    let built = match ensure(
        &tmux,
        &home,
        &mut store,
        cockpit,
        config.machine.as_str(),
        false,
        version,
        run_command,
    ) {
        Ok((built, _)) => built,
        Err(e) if matches!(tmux.has_session(cockpit), Ok(true)) => {
            eprintln!("cpd: the cockpit is not complete: {}", e.message());
            false
        }
        Err(e) => return Err(e),
    };
    // 3. The views on an old binary (a view just built runs the installed one).
    if !built {
        let own = if inside {
            std::env::var("TMUX_PANE").ok()
        } else {
            None
        };
        if let Err(e) = respawn_old_views(&tmux, &home, cockpit, own.as_deref()) {
            eprintln!("cpd: the views were not restarted: {}", e.message());
        }
    }
    if inside {
        eprintln!(
            "cpd: this terminal is inside the cockpit's tmux server: switch to the cockpit with `tmux switch-client -t {cockpit}`."
        );
        return Ok(Output::Owned);
    }
    // 4. The tmux version: an older server is restarted when it is safe, else the restart is marked
    // pending; the server's own tmux attaches while it runs another version than the installed one.
    let after = tmux_version(&home, &config, &mut store, &tmux, restart);
    let program = server_tmux(&tmux, &after).unwrap_or_else(|e| {
        eprintln!(
            "cpd: cannot compare the tmux server's version with the installed tmux: {}",
            e.message()
        );
        None
    });
    Err(exec_attach(&tmux, cockpit, program.as_deref()))
}

/// The tmux version follow-up of bare `cpd` (commands.md §13.3): a server older than the installed
/// tmux is restarted now when it is safe, else the restart is marked pending (the engine holds new
/// Attempts, then restarts it, contract §6.1). Returns what to add to the attach line about a restart
/// still to come; a failure is one line on stderr.
fn tmux_version(
    home: &CpdHome,
    config: &Config,
    store: &mut Store,
    tmux: &Tmux,
    restart: Restart,
) -> String {
    tmux_version_with(&Ports::current(), home, config, store, tmux, restart)
}

/// [`tmux_version`] with the installed tmux asked of `ports.runner`.
pub(crate) fn tmux_version_with(
    ports: &Ports,
    home: &CpdHome,
    config: &Config,
    store: &mut Store,
    tmux: &Tmux,
    restart: Restart,
) -> String {
    let (server, installed) = match (
        tmux.server_version(),
        installed_version_with(&*ports.runner),
    ) {
        (Ok(server), Ok(installed)) => (server, installed),
        // `server_tmux` says why it cannot compare.
        _ => return String::new(),
    };
    if !older_version(&server, &installed) {
        // A restart that is no longer needed (done by hand, or by the engine): a failure it left is said once.
        if server == installed {
            match (restart.failed)(home, &ports.clock.zone()) {
                Ok(Some(failed)) => eprintln!("cpd: {failed}"),
                Ok(None) => {}
                Err(e) => eprintln!("cpd: {}", e.message()),
            }
            if let Err(e) = (restart.clear)(home) {
                eprintln!("cpd: {}", e.message());
            }
        }
        return String::new();
    }
    match (restart.now)(home, config, store) {
        Ok(RestartOutcome::Restarted { reopened }) => {
            eprintln!(
                "cpd: restarted the tmux server onto tmux {installed}; {reopened} Session(s) reopened."
            );
            String::new()
        }
        Ok(RestartOutcome::Busy(why)) => match (restart.mark)(home, &server, &installed) {
            Ok(_) => format!(
                "; the restart happens once nothing is in the way ({})",
                why.join("; ")
            ),
            Err(e) => {
                eprintln!("cpd: cannot mark the tmux restart pending: {}", e.message());
                String::new()
            }
        },
        Ok(RestartOutcome::Running) => "; a restart is running".to_owned(),
        Ok(RestartOutcome::NotNeeded) => String::new(),
        Err(e) => {
            eprintln!(
                "cpd: the tmux restart failed: {} It is tried again at the next cpd.",
                e.message()
            );
            String::new()
        }
    }
}

/// `cpd tui`: when the server runs an older tmux than the installed one, marks a restart pending.
/// Returns what to say on the attach line.
fn mark_if_older(
    home: &CpdHome,
    tmux: &Tmux,
    restart: Restart,
) -> Result<Option<String>, CpdError> {
    let (server, installed) = (tmux.server_version()?, installed_version()?);
    if !older_version(&server, &installed) {
        return Ok(None);
    }
    (restart.mark)(home, &server, &installed)?;
    Ok(Some(
        "; the restart happens once no Attempt runs".to_owned(),
    ))
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
fn respawn_old_views(
    tmux: &Tmux,
    home: &CpdHome,
    cockpit: &str,
    skip: Option<&str>,
) -> Result<(), CpdError> {
    let installed = std::fs::metadata(started_as()?).map_err(|e| {
        CpdError::new(
            ErrorCode::SpawnFailed,
            format!("Cannot stat the installed cpd: {e}."),
        )
    })?;
    let main: Vec<Pane> = main_panes(tmux, cockpit)?
        .into_iter()
        .filter(|p| Some(p.pane.as_str()) != skip)
        .collect();
    let old = main
        .iter()
        .filter(|p| !p.dead && p.cpd_view.is_some())
        .any(|p| {
            // A pane whose binary cannot be read (gone) is not known to be old.
            p.pid
                .and_then(Tmux::pid_exe)
                .and_then(|path| std::fs::metadata(path).ok())
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
    eprintln!(
        "cpd: the tmux server runs tmux {server}, the installed tmux is {installed}: attaching with the server's own tmux{after}."
    );
    Ok(Some(exe))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn panes(views: &[Option<View>]) -> Vec<(PaneId, View)> {
        views
            .iter()
            .enumerate()
            .filter_map(|(i, v)| Some((PaneId::parse(&format!("%{i}"))?, (*v)?)))
            .collect()
    }

    fn joined(steps: Vec<Vec<String>>) -> Vec<String> {
        steps.iter().map(|s| s.join(" ")).collect()
    }

    #[test]
    fn the_reset_applies_the_sizes_in_order() {
        // %0 tree, %1 chat, %2 widgets, %3 plan
        let p = panes(&[
            Some(View::Tree),
            Some(View::Chat),
            Some(View::Widgets),
            Some(View::Plan),
        ]);
        assert_eq!(
            joined(reset_steps(&p)),
            [
                "resize-pane -t %0 -x 20%",
                "resize-pane -t %2 -x 30%",
                "resize-pane -t %3 -y 30%"
            ]
        );
    }

    #[test]
    fn the_actions_are_the_status_line_ranges() {
        assert_eq!(
            ["reset", "legend", "logs", "costs"].map(Action::parse),
            [
                Some(Action::Reset),
                Some(Action::Legend),
                Some(Action::Logs),
                Some(Action::Costs)
            ]
        );
        assert_eq!(Action::parse(""), None);
        assert_eq!(Action::parse("Reset"), None);
    }

    #[test]
    fn a_popup_opens_on_the_clicking_client() {
        let popup = on_client(legend::popup_args((215, 62)), "/dev/pts/4");
        assert_eq!(
            popup[..5],
            ["display-popup", "-c", "/dev/pts/4", "-E", "-T"]
        );
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
