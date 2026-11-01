//! The fake tmux as a route of the `FakeRunner` (ADR 0013): `TmuxRoute` is the [`Handler`] of the program `tmux`.
//! It maps a [`Call`] to an [`Invocation`](super::Invocation), tries a scripted reply (what a shell `tmux` wrapper
//! did) before the fake, and answers `-u -V` itself. STUB: the red tests of `route_tests` pin the behaviour.
//!
//! # Fault injections (the engine tests; each has a test in `route_tests`)
//!
//! - The first `kill-window` (or any call) fails: `FakeRunner::once("tmux", |call| call.args.iter().any(|a| a == "kill-window"),
//!   Reply::exit(1, "", "can't find window\n"))`. The rule answers once, then the call goes to the route again. `on` is the
//!   rule for every call; its matcher can read a flag the test flips to make tmux work again.
//! - The first `list-panes` fails: the same, matching `"list-panes"`.
//! - Tabs printed as `_` (a tmux that cannot print a tab, which the adapter refuses as `tmux_failed`): a `once` or `on`
//!   reply whose stdout has the rows with `_` between the fields.
//! - The command of a window exits with 7, or a signal killed it: [`TmuxRoute::new_panes_end`] with [`PaneEnd::Status`]`(7)` or
//!   [`PaneEnd::Signal`] (no status, `#{pane_dead_status}` empty), or `ctl pane-exit <pane> [--status N | --signal]` on a pane that
//!   exists.
//!
//! # What the engine reads of `FakeSpawn` (core/src/host/spawn.rs)
//!
//! - E1 (escalations): `alive(Pid(asker_pid))`; a pid the test made with `spawn` or `add_process` and has not `exit`ed is alive,
//!   any other is not.
//! - E3 (start a Job): `self_exe()` (the engine's executable, the driver's `argv[0]`) and `spawn(&Launch)`, one launch per
//!   driver, whose argv a test reads from `launches()`.
//! - E10 (titles): `self_exe()`, `spawn(&Launch)` for `session retitle <session> --width <w>`, then `try_wait(pid)` at each
//!   later tick: the test runs the launch (`Fx::children`), which marks the pid exited.

use super::{FakeTmux, Invocation, VERSION};
use crate::clock::ManualClock;
use crate::host::runner::{Call, FakeRunner, Handler, Reply, Runner};
use std::sync::Arc;

/// A matcher on the argv of a call (everything after `tmux`, `-u -L <socket> -f /dev/null` included).
type Matcher = Box<dyn Fn(&[String]) -> bool + Send + Sync>;

/// How the command of a new pane ends ([`TmuxRoute::new_panes_end`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaneEnd {
    /// It exits with this status.
    Status(i32),
    /// A signal killed it: tmux has no status for it.
    Signal,
}

/// The `tmux` route: scripted replies first, then the in-memory fake.
pub struct TmuxRoute {
    fake: FakeTmux,
    scripted: Vec<(Matcher, Reply)>,
    new_panes_end: Option<PaneEnd>,
}

impl TmuxRoute {
    /// A route over a fresh [`FakeTmux`] with no scripted reply and no nested `Runner`.
    pub fn new() -> TmuxRoute {
        TmuxRoute {
            fake: FakeTmux::new(),
            scripted: Vec::new(),
            new_panes_end: None,
        }
    }
    /// The `Runner` that `run-shell`, a hook or a binding calls with `/bin/sh -c <command>`.
    pub fn with_runner(mut self, runner: Arc<dyn Runner>) -> TmuxRoute {
        self.fake = self.fake.with_runner(runner);
        self
    }
    /// A scripted reply: a call whose argv `matches` is answered with `reply`, before the fake sees it (the
    /// first match answers).
    pub fn script(
        mut self,
        matches: impl Fn(&[String]) -> bool + Send + Sync + 'static,
        reply: Reply,
    ) -> TmuxRoute {
        self.scripted.push((Box::new(matches), reply));
        self
    }
    /// The command of every pane a later `new-session`, `new-window`, `split-window` or `respawn-pane` makes ends at once
    /// as `end` says: the pane is dead, with the status (none for a signal), and the `pane-died` hook runs.
    pub fn new_panes_end(mut self, end: PaneEnd) -> TmuxRoute {
        self.new_panes_end = Some(end);
        self
    }
    /// The fake behind the route: its state, `set_screen`, `on_popup`.
    pub fn fake(&self) -> &FakeTmux {
        &self.fake
    }
}

impl Default for TmuxRoute {
    fn default() -> TmuxRoute {
        TmuxRoute::new()
    }
}

/// What the `tmux` client prints when the `TMUX_TMPDIR` it needs is not set.
const NO_TMPDIR: &str = "error creating /tmp/tmux-<uid> (TMUX_TMPDIR is not set)\n";

/// The `TMUX_TMPDIR` a real tmux would see: the call's own entry (`None` is a removed variable), else the one the
/// process `inherited`, as a child does. Empty is unset.
pub(super) fn tmux_tmpdir(call: &Call, inherited: Option<String>) -> Option<String> {
    let own = call
        .envs
        .iter()
        .find(|(k, _)| k == "TMUX_TMPDIR")
        .map(|(_, v)| v.clone());
    own.unwrap_or(inherited).filter(|d| !d.is_empty())
}

impl Handler for TmuxRoute {
    fn handle(&self, call: &Call) -> Reply {
        if let Some((_, reply)) = self
            .scripted
            .iter()
            .find(|(matches, _)| matches(&call.args))
        {
            return reply.clone();
        }
        if call.args == ["-u", "-V"] {
            return Reply::ok(&format!("tmux {VERSION}\n"));
        }
        let tmpdir = tmux_tmpdir(call, std::env::var("TMUX_TMPDIR").ok());
        let Some(tmux_tmpdir) = tmpdir.as_deref() else {
            return Reply::exit(1, "", NO_TMPDIR);
        };
        // A command with no directory of its own runs in the process's.
        let cwd = call
            .cwd
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| "/".into()));
        let env = call
            .envs
            .iter()
            .filter_map(|(k, v)| v.clone().map(|v| (k.clone(), v)))
            .collect();
        let inv = Invocation {
            argv: call.args.clone(),
            tmux_tmpdir: tmux_tmpdir.into(),
            cwd,
            env,
        };
        let out = self.fake.run(&inv);
        if let (0, Some(end)) = (out.code, self.new_panes_end) {
            self.end_new_pane(&inv, end);
        }
        Reply::exit(out.code, &out.stdout, &out.stderr)
    }
}

impl TmuxRoute {
    /// Ends the pane `inv` made (`new_panes_end`): the newest pane of its server, or the one a `respawn-pane` names.
    fn end_new_pane(&self, inv: &Invocation, end: PaneEnd) {
        let args = &inv.argv;
        let value = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
        };
        let Some(socket) = value("-L") else { return };
        // The commands of a call are split on `;`: the last one that makes a pane is the call's.
        let makes = |name: &str| args.iter().any(|a| a == name);
        let pane = if makes("respawn-pane") {
            match value("-t") {
                Some(pane) => pane.clone(),
                None => return,
            }
        } else if makes("new-session") || makes("new-window") || makes("split-window") {
            let found = self.fake.on_socket(socket);
            let [only] = &found[..] else { return };
            match super::State::of(only.clone()).panes().last() {
                Some(pane) => pane.id.clone(),
                None => return,
            }
        } else {
            return;
        };
        let how = match end {
            PaneEnd::Status(n) => vec!["--status".to_owned(), n.to_string()],
            PaneEnd::Signal => vec!["--signal".to_owned()],
        };
        let mut argv = vec![
            "-L".to_owned(),
            socket.clone(),
            "__ctl".to_owned(),
            "pane-exit".to_owned(),
            pane,
        ];
        argv.extend(how);
        let out = self.fake.run(&Invocation {
            argv,
            tmux_tmpdir: inv.tmux_tmpdir.clone(),
            cwd: inv.cwd.clone(),
            env: inv.env.clone(),
        });
        assert_eq!(
            out.code, 0,
            "the fake could not end the new pane: {}",
            out.stderr
        );
    }
}

impl FakeRunner {
    /// A runner with the `tmux` route on a fresh [`TmuxRoute`], on its own [`ManualClock`]; the route's
    /// `run-shell` calls come back to this runner.
    pub fn with_tmux() -> FakeRunner {
        let runner = FakeRunner::new(ManualClock::new());
        // Weak: the route lives in the runner's script, a strong handle back would be a cycle.
        let route = TmuxRoute::new().with_runner(runner.weak());
        runner.route("tmux", Arc::new(route))
    }
}
