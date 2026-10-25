//! The fake tmux as a route of the `FakeRunner` (ADR 0013): `TmuxRoute` is the [`Handler`] of the program `tmux`.
//! It maps a [`Call`] to an [`Invocation`](super::Invocation), tries a scripted reply (what a shell `tmux` wrapper
//! did) before the fake, and answers `-u -V` itself. STUB: the red tests of `route_tests` pin the behaviour.

use super::{FakeTmux, Invocation, VERSION};
use crate::clock::ManualClock;
use crate::host::runner::{Call, FakeRunner, Handler, Reply, Runner};
use std::sync::Arc;

/// A matcher on the argv of a call (everything after `tmux`, `-u -L <socket> -f /dev/null` included).
type Matcher = Box<dyn Fn(&[String]) -> bool + Send + Sync>;

/// The `tmux` route: scripted replies first, then the in-memory fake.
pub struct TmuxRoute {
    fake: FakeTmux,
    scripted: Vec<(Matcher, Reply)>,
}

impl TmuxRoute {
    /// A route over a fresh [`FakeTmux`] with no scripted reply and no nested `Runner`.
    pub fn new() -> TmuxRoute {
        TmuxRoute { fake: FakeTmux::new(), scripted: Vec::new() }
    }
    /// The `Runner` that `run-shell`, a hook or a binding calls with `/bin/sh -c <command>`.
    pub fn with_runner(mut self, runner: Arc<dyn Runner>) -> TmuxRoute {
        self.fake = self.fake.with_runner(runner);
        self
    }
    /// A scripted reply: a call whose argv `matches` is answered with `reply`, before the fake sees it (the
    /// first match answers).
    pub fn script(mut self, matches: impl Fn(&[String]) -> bool + Send + Sync + 'static, reply: Reply) -> TmuxRoute {
        self.scripted.push((Box::new(matches), reply));
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
    let own = call.envs.iter().find(|(k, _)| k == "TMUX_TMPDIR").map(|(_, v)| v.clone());
    own.unwrap_or(inherited).filter(|d| !d.is_empty())
}

impl Handler for TmuxRoute {
    fn handle(&self, call: &Call) -> Reply {
        if let Some((_, reply)) = self.scripted.iter().find(|(matches, _)| matches(&call.args)) {
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
        let cwd = call.cwd.clone().unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| "/".into()));
        let env = call.envs.iter().filter_map(|(k, v)| v.clone().map(|v| (k.clone(), v))).collect();
        let out = self.fake.run(&Invocation { argv: call.args.clone(), tmux_tmpdir: tmux_tmpdir.into(), cwd, env });
        Reply::exit(out.code, &out.stdout, &out.stderr)
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
