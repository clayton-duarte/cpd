//! The fake tmux as a route of the `FakeRunner` (ADR 0013): `TmuxRoute` is the [`Handler`] of the program `tmux`.
//! It maps a [`Call`] to an [`Invocation`](super::Invocation), tries a scripted reply (what a shell `tmux` wrapper
//! did) before the fake, and answers `-u -V` itself. STUB: the red tests of `route_tests` pin the behaviour.

use super::FakeTmux;
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

impl Handler for TmuxRoute {
    fn handle(&self, _call: &Call) -> Reply {
        todo!("TmuxRoute::handle: Call -> Invocation -> FakeTmux::run -> Reply")
    }
}

impl FakeRunner {
    /// A runner with the `tmux` route on a fresh [`TmuxRoute`], on its own [`ManualClock`]; the route's
    /// `run-shell` calls come back to this runner.
    pub fn with_tmux() -> FakeRunner {
        let _ = ManualClock::new();
        todo!("FakeRunner::with_tmux")
    }
}
