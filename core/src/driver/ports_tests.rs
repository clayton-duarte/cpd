//! Red tests for the driver's loops on virtual time and `FakeSpawn` (Plan tests-mock, Job `ports2-drv-tests`): A4's poll and
//! its look at the Worker's pane, the A9 kill ladder, the stall rule by mtime, the grace between `SIGTERM` and `SIGKILL`, and
//! A3's wait for the Lead. Time is the `ManualClock` (`advance`, `naps`, `elapsed`) and every pid is the `FakeSpawn`'s: no
//! test sleeps, reads the wall clock or starts a process. They are red until `ports2-drv-impl` threads `Ports` through the
//! `_with` forms they call.
//!
//! Ported from existing tests of loops that waited for real (`old -> new`):
//! `worker::tests::a_kill_that_fails_three_times_reports_the_error` ->
//!   `a_kill_that_fails_three_times_reports_the_error_and_naps_between_tries_only`;
//! `worker::tests::a_kill_that_fails_twice_then_works_is_ok` -> `a_kill_that_fails_twice_naps_the_pause_twice_then_works`;
//! `worker::tests::a_window_already_gone_is_success_without_a_retry` -> `a_window_already_gone_is_success_with_no_nap`;
//! `stall::tests::a_stall_is_killed_unless_a_check_span_is_open` -> `an_open_check_span_is_not_a_stall_and_the_quiet_restarts_from_it`
//!   and `a_tool_call_quiet_for_the_limit_of_virtual_time_has_its_descendants_terminated`;
//! `stall::tests::descendants_are_found_and_the_root_is_not_one` (its `terminate` half) ->
//!   `a_process_that_ends_on_term_is_not_killed_and_costs_no_nap`.

use std::io;
use std::path::{Path, PathBuf};
use std::process::ExitStatus;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use time::OffsetDateTime;

use super::run::{Look, PaneWatch, poll_with};
use super::stall::Watch;
use super::worker::{kill_attempt_window_with, retry_kill_with};
use crate::clock::{Clock, ManualClock};
use crate::config::{Catalog, Config, TierModel};
use crate::error::{CpdError, ErrorCode};
use crate::fakes::git::{GitRoute, Head, Repo, Universe};
use crate::home::CpdHome;
use crate::host::layout::attempt_window;
use crate::host::runner::{Call, Reply};
use crate::host::spawn::{FakeSpawn, Launch, Pid, Signal, Spawn};
use crate::host::{PaneId, Tmux};
use crate::ids::{AttemptId, JobKey, PlanId};
use crate::ports::{Fakes, Ports};

use super::{Allocation, StartCtx, start_with, start_with_allocation};
use crate::store::Store;

const SECOND: Duration = Duration::from_secs(1);

fn failure() -> CpdError {
    CpdError::new(ErrorCode::TmuxFailed, "kill failed")
}

pub(crate) fn config() -> Config {
    Config::parse(
        "version = 1\nmachine = \"test\"\n",
        Path::new("config.toml"),
    )
    .unwrap()
    .0
}

/// A `Clock` over a `ManualClock` that calls `hook(n)` after its `n`th nap: how a test changes the world between two polls
/// of a loop that it cannot step itself (a Worker that ends, a cancel that arrives).
pub(crate) struct HookClock {
    inner: ManualClock,
    naps: AtomicUsize,
    hook: Box<dyn Fn(usize) + Send + Sync>,
}

impl HookClock {
    pub(crate) fn new(
        inner: &ManualClock,
        hook: impl Fn(usize) + Send + Sync + 'static,
    ) -> HookClock {
        HookClock {
            inner: inner.clone(),
            naps: AtomicUsize::new(0),
            hook: Box::new(hook),
        }
    }
}

impl Clock for HookClock {
    fn now(&self) -> std::time::Instant {
        self.inner.now()
    }
    fn sleep(&self, d: Duration) {
        self.inner.sleep(d);
        let n = self.naps.fetch_add(1, Ordering::SeqCst) + 1;
        (self.hook)(n);
    }
    fn utc(&self) -> OffsetDateTime {
        self.inner.utc()
    }
}

/// A `Spawn` over a `FakeSpawn` whose processes ignore `SIGTERM` (recorded in `ignored`) and end on `SIGKILL`.
pub(crate) struct Stubborn {
    pub(crate) inner: Arc<FakeSpawn>,
    pub(crate) ignored: Mutex<Vec<Pid>>,
}

impl Spawn for Stubborn {
    fn spawn(&self, l: &Launch) -> io::Result<Pid> {
        self.inner.spawn(l)
    }
    fn try_wait(&self, p: Pid) -> io::Result<Option<ExitStatus>> {
        self.inner.try_wait(p)
    }
    fn signal(&self, p: Pid, s: Signal) -> io::Result<()> {
        match s {
            Signal::Term => {
                self.ignored.lock().unwrap().push(p);
                Ok(())
            }
            Signal::Kill | Signal::Usr1 => self.inner.signal(p, s),
        }
    }
    fn alive(&self, p: Pid) -> bool {
        self.inner.alive(p)
    }
    fn exe_of(&self, p: Pid) -> Option<PathBuf> {
        self.inner.exe_of(p)
    }
    fn with_env(&self, key: &str, value: &str) -> Vec<Pid> {
        self.inner.with_env(key, value)
    }
    fn cmdline(&self, p: Pid) -> Option<String> {
        self.inner.cmdline(p)
    }
    fn ppid(&self, p: Pid) -> Option<Pid> {
        self.inner.ppid(p)
    }
    fn descendants(&self, root: Pid) -> Vec<Pid> {
        self.inner.descendants(root)
    }
    fn env_of(&self, p: Pid, key: &str) -> Option<String> {
        self.inner.env_of(p, key)
    }
    fn exec(&self, l: &Launch) -> io::Error {
        self.inner.exec(l)
    }
    fn stream(&self, l: &Launch) -> io::Result<(Pid, Box<dyn std::io::BufRead + Send>)> {
        self.inner.stream(l)
    }
    fn self_exe(&self) -> Result<PathBuf, CpdError> {
        self.inner.self_exe()
    }
}

/// The fakes' `Ports` with `clock` instead of the `ManualClock`, and `spawn` instead of the `FakeSpawn`.
pub(crate) fn ports_with(fakes: &Fakes, clock: Arc<dyn Clock>, spawn: Arc<dyn Spawn>) -> Ports {
    Ports {
        clock,
        runner: fakes.ports.runner.clone(),
        spawn,
        ..Ports::real()
    }
}

/// A process the `FakeSpawn` knows, running: pids count from 1000.
pub(crate) fn process(spawn: &FakeSpawn) -> Pid {
    spawn
        .spawn(&Launch {
            program: "x".to_owned(),
            ..Launch::default()
        })
        .unwrap()
}

fn tmux(fakes: &Fakes) -> Tmux {
    Tmux::new("sock")
        .with_runner(fakes.ports.runner.clone())
        .with_clock(fakes.ports.clock.clone())
}

fn pane() -> PaneId {
    PaneId::parse("%1").unwrap()
}

fn runs(call: &Call, verb: &str) -> bool {
    call.args.iter().any(|a| a == verb)
}

/// One `list-panes` line (`LIST_PANES_FORMAT`): a pane `%1` of window `@1` in session `session`.
fn pane_line(session: &str, window: &str, dead: bool, status: &str, pid: Option<u32>) -> String {
    let pid = pid.map_or_else(String::new, |p| p.to_string());
    format!(
        "%1\t@1\t{session}\t{window}\t{}\t{status}\t{pid}\t\t\n",
        u8::from(dead)
    )
}

fn count(fakes: &Fakes, verb: &str) -> usize {
    fakes
        .runner
        .calls()
        .iter()
        .filter(|c| runs(c, verb))
        .count()
}

fn attempt() -> (PlanId, AttemptId) {
    (
        PlanId::parse("pay").unwrap(),
        AttemptId::parse("a-a1").unwrap(),
    )
}

// ---- A9: the kill ladder -------------------------------------------------------------------------------------------

#[test]
fn a_kill_that_fails_twice_naps_the_pause_twice_then_works() {
    let clock = ManualClock::new();
    let mut calls = 0;
    let result = retry_kill_with(
        &clock,
        3,
        SECOND,
        || {
            calls += 1;
            if calls < 3 { Err(failure()) } else { Ok(()) }
        },
        || false,
    );
    assert!(result.is_ok());
    assert_eq!(calls, 3);
    assert_eq!(clock.naps(), [SECOND, SECOND]);
}

#[test]
fn a_kill_that_fails_three_times_reports_the_error_and_naps_between_tries_only() {
    let clock = ManualClock::new();
    let mut calls = 0;
    let result = retry_kill_with(
        &clock,
        3,
        SECOND,
        || {
            calls += 1;
            Err(failure())
        },
        || false,
    );
    assert_eq!(result.unwrap_err().code(), ErrorCode::TmuxFailed);
    assert_eq!(calls, 3);
    assert_eq!(clock.naps(), [SECOND, SECOND], "no nap after the last try");
}

#[test]
fn a_window_already_gone_is_success_with_no_nap() {
    let clock = ManualClock::new();
    let mut calls = 0;
    let result = retry_kill_with(
        &clock,
        3,
        SECOND,
        || {
            calls += 1;
            Err(failure())
        },
        || true,
    );
    assert!(result.is_ok());
    assert_eq!(calls, 1);
    assert!(clock.naps().is_empty());
}

/// A fake tmux whose `list-panes` always lists the Attempt's window `@1` in the work session.
fn with_the_attempt_window(fakes: &Fakes, plan: &PlanId, id: &AttemptId) {
    let line = pane_line("cpd-work", &attempt_window(plan, id), false, "", None);
    (*fakes.runner)
        .clone()
        .on("tmux", |c| runs(c, "list-panes"), Reply::ok(&line));
}

#[test]
fn a_window_whose_kill_fails_twice_is_killed_after_two_one_second_naps_on_the_clock() {
    let fakes = Fakes::new();
    let (plan, id) = attempt();
    with_the_attempt_window(&fakes, &plan, &id);
    let fail = Reply::exit(1, "", "can't find window\n");
    (*fakes.runner)
        .clone()
        .once("tmux", |c| runs(c, "kill-window"), fail.clone())
        .once("tmux", |c| runs(c, "kill-window"), fail)
        .on("tmux", |c| runs(c, "kill-window"), Reply::ok(""));
    kill_attempt_window_with(&fakes.ports, &config(), &tmux(&fakes), &plan, &id).unwrap();
    assert_eq!(count(&fakes, "kill-window"), 3);
    assert_eq!(fakes.clock.naps(), [SECOND, SECOND]);
}

#[test]
fn a_window_that_cannot_be_killed_fails_after_three_tries_and_two_naps() {
    let fakes = Fakes::new();
    let (plan, id) = attempt();
    with_the_attempt_window(&fakes, &plan, &id);
    (*fakes.runner).clone().on(
        "tmux",
        |c| runs(c, "kill-window"),
        Reply::exit(1, "", "no\n"),
    );
    let err =
        kill_attempt_window_with(&fakes.ports, &config(), &tmux(&fakes), &plan, &id).unwrap_err();
    assert_eq!(err.code(), ErrorCode::TmuxFailed);
    assert_eq!(count(&fakes, "kill-window"), 3);
    assert_eq!(fakes.clock.naps(), [SECOND, SECOND]);
}

#[test]
fn a_window_that_is_gone_after_a_failed_kill_counts_as_killed_with_no_nap() {
    let fakes = Fakes::new();
    let (plan, id) = attempt();
    let line = pane_line("cpd-work", &attempt_window(&plan, &id), false, "", None);
    (*fakes.runner)
        .clone()
        .once("tmux", |c| runs(c, "list-panes"), Reply::ok(&line))
        .on("tmux", |c| runs(c, "list-panes"), Reply::ok(""))
        .on(
            "tmux",
            |c| runs(c, "kill-window"),
            Reply::exit(1, "", "can't find window\n"),
        );
    kill_attempt_window_with(&fakes.ports, &config(), &tmux(&fakes), &plan, &id).unwrap();
    assert_eq!(count(&fakes, "kill-window"), 1);
    assert!(fakes.clock.naps().is_empty());
}

// ---- A4: the poll and the look at the pane --------------------------------------------------------------------------

#[test]
fn every_poll_naps_the_driver_poll_of_half_a_second_on_the_clock() {
    let fakes = Fakes::new();
    let pi = process(&fakes.spawn);
    let line = pane_line("cpd-work", "pay--a-a1", false, "", Some(pi.0));
    (*fakes.runner)
        .clone()
        .on("tmux", |c| runs(c, "list-panes"), Reply::ok(&line));
    let tmux = tmux(&fakes);
    let mut watch = PaneWatch::default();
    for _ in 0..3 {
        poll_with(&fakes.ports, &mut watch, &tmux, &pane()).unwrap();
    }
    assert_eq!(fakes.clock.naps(), [Duration::from_millis(500); 3]);
}

#[test]
fn a_pane_whose_pid_runs_is_seen_alive_without_asking_tmux_again() {
    let fakes = Fakes::new();
    let pi = process(&fakes.spawn);
    let line = pane_line("cpd-work", "pay--a-a1", false, "", Some(pi.0));
    (*fakes.runner)
        .clone()
        .on("tmux", |c| runs(c, "list-panes"), Reply::ok(&line));
    let tmux = tmux(&fakes);
    let mut watch = PaneWatch::default();
    assert_eq!(
        poll_with(&fakes.ports, &mut watch, &tmux, &pane()).unwrap(),
        Look::Alive(Some(pi.0))
    );
    assert_eq!(
        poll_with(&fakes.ports, &mut watch, &tmux, &pane()).unwrap(),
        Look::Alive(Some(pi.0))
    );
    assert_eq!(
        count(&fakes, "list-panes"),
        1,
        "the second look is `alive` on the FakeSpawn, with no fork"
    );
}

#[test]
fn a_worker_that_ends_between_two_polls_is_seen_gone_at_the_next_one() {
    let fakes = Fakes::new();
    let pi = process(&fakes.spawn);
    let alive = pane_line("cpd-work", "pay--a-a1", false, "", Some(pi.0));
    let dead = pane_line("cpd-work", "pay--a-a1", true, "0", None);
    (*fakes.runner)
        .clone()
        .once("tmux", |c| runs(c, "list-panes"), Reply::ok(&alive))
        .on("tmux", |c| runs(c, "list-panes"), Reply::ok(&dead));
    let tmux = tmux(&fakes);
    let mut watch = PaneWatch::default();
    assert_eq!(
        poll_with(&fakes.ports, &mut watch, &tmux, &pane()).unwrap(),
        Look::Alive(Some(pi.0))
    );
    fakes.clock.advance(Duration::from_secs(30));
    fakes.spawn.exit(pi, 0);
    let look = poll_with(&fakes.ports, &mut watch, &tmux, &pane()).unwrap();
    assert_eq!(
        look,
        Look::Gone("The Worker's pane died (exit status 0) before it submitted.".to_owned())
    );
    assert_eq!(
        fakes.clock.elapsed(),
        Duration::from_secs(31),
        "two polls of 500 ms and the 30 s the test advanced"
    );
}

#[test]
fn a_pane_missing_from_the_list_is_gone() {
    let fakes = Fakes::new();
    (*fakes.runner)
        .clone()
        .on("tmux", |c| runs(c, "list-panes"), Reply::ok(""));
    let mut watch = PaneWatch::default();
    let look = poll_with(&fakes.ports, &mut watch, &tmux(&fakes), &pane()).unwrap();
    assert_eq!(look, Look::Gone("The Worker's pane is gone.".to_owned()));
}

#[test]
fn a_failed_list_panes_is_not_known_once_and_an_error_the_second_time() {
    let fakes = Fakes::new();
    (*fakes.runner)
        .clone()
        .on(
            "tmux",
            |c| runs(c, "list-panes"),
            Reply::exit(1, "", "boom\n"),
        )
        .on("tmux", |c| runs(c, "list-sessions"), Reply::ok("$0\n"));
    let tmux = tmux(&fakes);
    let mut watch = PaneWatch::default();
    assert_eq!(
        poll_with(&fakes.ports, &mut watch, &tmux, &pane()).unwrap(),
        Look::Unknown
    );
    let err = poll_with(&fakes.ports, &mut watch, &tmux, &pane()).unwrap_err();
    assert_eq!(err.code(), ErrorCode::TmuxFailed);
    assert_eq!(fakes.clock.naps(), [Duration::from_millis(500); 2]);
}

// ---- A4: the stall rule, by mtime on virtual time --------------------------------------------------------------------

const SESSION: &str = r#"{"type":"session"}"#;
const CALL: &str = r#"{"type":"message","message":{"role":"assistant","content":[{"type":"toolCall","id":"c1","name":"bash"}],"stopReason":"toolUse"}}"#;
const TEXT: &str = r#"{"type":"message","message":{"role":"assistant","content":[{"type":"text","text":"done"}],"stopReason":"stop"}}"#;

/// Makes `file`'s modification time the virtual time `at` (real files in a temp dir, virtual time on them).
pub(crate) fn modified_at(file: &Path, at: OffsetDateTime) {
    let at = UNIX_EPOCH + Duration::from_secs(u64::try_from(at.unix_timestamp()).unwrap());
    std::fs::File::options()
        .write(true)
        .open(file)
        .unwrap()
        .set_modified(at)
        .unwrap();
}

/// A session file inside a tool call, last written at the clock's now.
fn stalled(dir: &tempfile::TempDir, clock: &ManualClock) -> PathBuf {
    let file = dir.path().join("session.jsonl");
    std::fs::write(&file, [SESSION, CALL].join("\n")).unwrap();
    modified_at(&file, clock.utc());
    file
}

/// pi and two descendants, all running on the fake.
fn tree(spawn: &FakeSpawn) -> (Pid, Pid, Pid) {
    let (pi, a, b) = (process(spawn), process(spawn), process(spawn));
    spawn.set_cmdline(a, "sleep 600");
    spawn.set_cmdline(b, "cargo test");
    (pi, a, b)
}

#[test]
fn a_tool_call_quiet_for_less_than_the_limit_of_virtual_time_is_not_killed() {
    let fakes = Fakes::new();
    let dir = tempfile::tempdir().unwrap();
    let file = stalled(&dir, &fakes.clock);
    let (pi, a, b) = tree(&fakes.spawn);
    fakes.clock.advance(Duration::from_secs(59));
    let kids = move |_: u32| vec![a.0, b.0];
    assert_eq!(
        Watch::new(60).check_with(&fakes.ports, &kids, &file, pi.0, false),
        None
    );
    assert!(fakes.spawn.signals().is_empty());
}

#[test]
fn a_tool_call_quiet_for_the_limit_of_virtual_time_has_its_descendants_terminated() {
    let fakes = Fakes::new();
    let dir = tempfile::tempdir().unwrap();
    let file = stalled(&dir, &fakes.clock);
    let (pi, a, b) = tree(&fakes.spawn);
    fakes.clock.advance(Duration::from_secs(60));
    let kids = move |root: u32| {
        if root == pi.0 {
            vec![a.0, b.0]
        } else {
            Vec::new()
        }
    };
    let line = Watch::new(60)
        .check_with(&fakes.ports, &kids, &file, pi.0, false)
        .expect("a stall");
    assert_eq!(
        line,
        format!(
            "A4: stall: the Worker's tool call has been idle 60 s; killed pid {} (sleep 600), pid {} (cargo test)",
            a.0, b.0
        )
    );
    assert_eq!(
        fakes.spawn.signals(),
        [(a, Signal::Term), (b, Signal::Term)],
        "pi itself is never signalled"
    );
}

#[test]
fn a_process_that_ends_on_term_is_not_killed_and_costs_no_nap() {
    let fakes = Fakes::new();
    let dir = tempfile::tempdir().unwrap();
    let file = stalled(&dir, &fakes.clock);
    let (pi, a, b) = tree(&fakes.spawn);
    fakes.clock.advance(Duration::from_secs(60));
    let kids = move |_: u32| vec![a.0, b.0];
    Watch::new(60)
        .check_with(&fakes.ports, &kids, &file, pi.0, false)
        .expect("a stall");
    assert!(
        fakes.clock.naps().is_empty(),
        "the FakeSpawn ends a pid on SIGTERM: nothing to wait for"
    );
    assert!(!fakes.spawn.alive(a) && !fakes.spawn.alive(b));
}

#[test]
fn a_stall_is_killed_once_and_a_new_write_makes_a_new_stall() {
    let fakes = Fakes::new();
    let dir = tempfile::tempdir().unwrap();
    let file = stalled(&dir, &fakes.clock);
    let (pi, a, b) = tree(&fakes.spawn);
    let kids = std::cell::RefCell::new(vec![a.0]);
    let list = |_: u32| kids.borrow().clone();
    let mut watch = Watch::new(60);
    fakes.clock.advance(Duration::from_secs(60));
    assert!(
        watch
            .check_with(&fakes.ports, &list, &file, pi.0, false)
            .is_some()
    );
    fakes.clock.advance(Duration::from_secs(600));
    assert_eq!(
        watch.check_with(&fakes.ports, &list, &file, pi.0, false),
        None,
        "the same write is killed once"
    );
    assert_eq!(fakes.spawn.signals(), [(a, Signal::Term)]);
    // pi wrote again, and stalled again with a new child.
    modified_at(&file, fakes.clock.utc());
    *kids.borrow_mut() = vec![b.0];
    fakes.clock.advance(Duration::from_secs(60));
    let line = watch
        .check_with(&fakes.ports, &list, &file, pi.0, false)
        .expect("a new stall");
    assert!(
        line.contains(&format!("killed pid {} (cargo test)", b.0)),
        "{line}"
    );
}

#[test]
fn an_open_check_span_is_not_a_stall_and_the_quiet_restarts_from_it() {
    let fakes = Fakes::new();
    let dir = tempfile::tempdir().unwrap();
    let file = stalled(&dir, &fakes.clock);
    let (pi, a, _b) = tree(&fakes.spawn);
    let kids = move |_: u32| vec![a.0];
    let mut watch = Watch::new(60);
    // The file has been quiet for an hour, all of it inside `attempt submit`'s span.
    fakes.clock.advance(Duration::from_secs(3600));
    assert_eq!(
        watch.check_with(&fakes.ports, &kids, &file, pi.0, true),
        None,
        "an open span is not a stall"
    );
    // The span ended a moment ago: the quiet counts from the last tick that saw it.
    fakes.clock.advance(Duration::from_secs(59));
    assert_eq!(
        watch.check_with(&fakes.ports, &kids, &file, pi.0, false),
        None
    );
    assert!(fakes.spawn.signals().is_empty());
    fakes.clock.advance(SECOND);
    let line = watch
        .check_with(&fakes.ports, &kids, &file, pi.0, false)
        .expect("60 s after the span");
    assert!(line.contains("idle 60 s"), "{line}");
}

#[test]
fn a_limit_of_zero_never_stalls() {
    let fakes = Fakes::new();
    let dir = tempfile::tempdir().unwrap();
    let file = stalled(&dir, &fakes.clock);
    let (pi, a, _b) = tree(&fakes.spawn);
    fakes.clock.advance(Duration::from_secs(86_400));
    let kids = move |_: u32| vec![a.0];
    assert_eq!(
        Watch::new(0).check_with(&fakes.ports, &kids, &file, pi.0, false),
        None
    );
    assert!(fakes.spawn.signals().is_empty());
}

#[test]
fn a_stall_with_no_descendants_kills_nothing() {
    let fakes = Fakes::new();
    let dir = tempfile::tempdir().unwrap();
    let file = stalled(&dir, &fakes.clock);
    let (pi, _a, _b) = tree(&fakes.spawn);
    fakes.clock.advance(Duration::from_secs(60));
    let none = |_: u32| Vec::new();
    assert_eq!(
        Watch::new(60).check_with(&fakes.ports, &none, &file, pi.0, false),
        None
    );
    assert!(fakes.spawn.signals().is_empty());
}

#[test]
fn a_session_file_that_is_not_inside_a_tool_call_is_not_a_stall() {
    let fakes = Fakes::new();
    let dir = tempfile::tempdir().unwrap();
    let file = stalled(&dir, &fakes.clock);
    std::fs::write(&file, [SESSION, TEXT].join("\n")).unwrap();
    modified_at(&file, fakes.clock.utc());
    let (pi, a, _b) = tree(&fakes.spawn);
    fakes.clock.advance(Duration::from_secs(3600));
    let kids = move |_: u32| vec![a.0];
    assert_eq!(
        Watch::new(60).check_with(&fakes.ports, &kids, &file, pi.0, false),
        None
    );
    assert!(fakes.spawn.signals().is_empty());
}

#[test]
fn a_process_that_ignores_term_gets_kill_after_ten_virtual_seconds_of_grace() {
    let fakes = Fakes::new();
    let stubborn = Arc::new(Stubborn {
        inner: fakes.spawn.clone(),
        ignored: Mutex::default(),
    });
    let ports = ports_with(&fakes, fakes.ports.clock.clone(), stubborn.clone());
    let dir = tempfile::tempdir().unwrap();
    let file = stalled(&dir, &fakes.clock);
    let (pi, a, _b) = tree(&fakes.spawn);
    fakes.clock.advance(Duration::from_secs(60));
    let kids = move |_: u32| vec![a.0];
    let line = Watch::new(60)
        .check_with(&ports, &kids, &file, pi.0, false)
        .expect("a stall");
    assert!(
        line.ends_with(&format!("; SIGKILL after 10 s to pid {}", a.0)),
        "{line}"
    );
    assert_eq!(*stubborn.ignored.lock().unwrap(), [a], "SIGTERM first");
    assert_eq!(fakes.spawn.signals(), [(a, Signal::Kill)], "then SIGKILL");
    assert_eq!(
        fakes.clock.naps(),
        [Duration::from_millis(100); 100],
        "polled every 100 ms for the 10 s"
    );
    assert_eq!(fakes.clock.elapsed(), Duration::from_secs(70));
}

#[test]
fn a_process_that_ends_during_the_grace_is_not_killed() {
    let fakes = Fakes::new();
    let stubborn = Arc::new(Stubborn {
        inner: fakes.spawn.clone(),
        ignored: Mutex::default(),
    });
    let (pi, a, _b) = tree(&fakes.spawn);
    let spawn = fakes.spawn.clone();
    let clock: Arc<dyn Clock> = Arc::new(HookClock::new(&fakes.clock, move |n| {
        if n == 3 {
            spawn.exit(a, 0);
        }
    }));
    let ports = ports_with(&fakes, clock, stubborn);
    let dir = tempfile::tempdir().unwrap();
    let file = stalled(&dir, &fakes.clock);
    fakes.clock.advance(Duration::from_secs(60));
    let kids = move |_: u32| vec![a.0];
    let line = Watch::new(60)
        .check_with(&ports, &kids, &file, pi.0, false)
        .expect("a stall");
    assert!(!line.contains("SIGKILL"), "{line}");
    assert!(fakes.spawn.signals().is_empty());
    assert_eq!(fakes.clock.naps(), [Duration::from_millis(100); 3]);
}

// ---- The workflow-worktree model (hs-d680/hs-d682/hs-d683, `wlinear-tests`) ------------------------------------------

/// A temporary store holding Plan `pay`, its Workflow `api` (`cpd/pay/api`, Lead worktree `/repos/r.lead`) and the
/// queued Jobs `a` and `b`; `live` seeds a running Attempt `a-a1` of Job `a` (a live sibling).
struct StartRig {
    _dir: tempfile::TempDir,
    home: CpdHome,
    store: Store,
}

fn start_rig(live: bool) -> StartRig {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let placeholder = "f".repeat(64);
    store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/repos/r.lead', 'O3', 'working', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, locked_at, created_at)
                   VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
                 INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, started_at, created_txn, created_at)
                   VALUES ('pay', 'api', '/repos/r', 'main', 'dispatch', 'cpd/pay/api', '/repos/r.lead', 't', cpd_txn(), 't');
                 INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
                   VALUES ('pay', 'api', 'a', 'A', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'queued', 0, '{placeholder}', cpd_txn(), 't'),
                          ('pay', 'api', 'b', 'B', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'queued', 0, '{placeholder}', cpd_txn(), 't');",
            ))?;
            Ok(())
        })
        .unwrap();
    // The Fingerprint of §2.3, computed from the stored row.
    let plan = PlanId::parse("pay").unwrap();
    let jobs = store
        .read(|conn| crate::plan::jobs::load(conn, &plan))
        .unwrap();
    let fingerprints: Vec<(String, String)> = jobs
        .iter()
        .map(|j| (j.key.clone(), crate::plan::locked_fingerprint(j)))
        .collect();
    store
        .write("fixture", |tx| {
            for (key, fingerprint) in &fingerprints {
                tx.execute(
                    "UPDATE jobs SET fingerprint = ?2 WHERE plan = 'pay' AND key = ?1",
                    rusqlite::params![key, fingerprint],
                )?;
            }
            Ok(())
        })
        .unwrap();
    if live {
        store
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, driver_pid, started_at)
                     VALUES ('pay', 'a-a1', 'a', 1, 'S2', 'ladder', 'running', 'cpd/pay--a-a1', '/repos/r.pay--a-a1', 4242, 't')",
                    [],
                )?;
                tx.execute("UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = 'a'", [])?;
                Ok(())
            })
            .unwrap();
    }
    StartRig {
        _dir: dir,
        home,
        store,
    }
}

/// A catalog the starter can climb without pi.
fn catalog() -> Catalog {
    let model = |m: &str| TierModel {
        model: m.to_owned(),
        efforts: vec![1, 2, 3],
    };
    Catalog {
        j: crate::config::TierModel {
            model: String::new(),
            efforts: Vec::new(),
        },
        pi_version: "1.0.0".to_owned(),
        o: model("o/m"),
        s: model("s/m"),
        h: model("h/m"),
        g: TierModel {
            model: "g/m".to_owned(),
            efforts: vec![],
        },
    }
}

/// Starts the next Attempt of Job `key` of Plan `pay`, through the fake spawn.
fn start_attempt(rig: &mut StartRig, fakes: &Fakes, key: &str) {
    fakes.spawn.set_self_exe(std::env::current_exe().unwrap());
    let config = config();
    let catalog = catalog();
    let ctx = StartCtx {
        home: &rig.home,
        config: &config,
        catalog: &catalog,
        command: "test",
    };
    start_with(
        &fakes.ports,
        &mut rig.store,
        &ctx,
        &PlanId::parse("pay").unwrap(),
        &JobKey::parse(key).unwrap(),
    )
    .unwrap();
}

/// The `(branch, worktree)` the starter recorded for Attempt `id`.
fn allocated(rig: &mut StartRig, id: &str) -> (String, String) {
    rig.store
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT branch, worktree FROM attempts WHERE plan = 'pay' AND id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap()
}

#[test]
fn a_linear_attempt_takes_the_workflow_worktree() {
    let mut rig = start_rig(false);
    let fakes = Fakes::new();
    start_attempt(&mut rig, &fakes, "a");
    assert_eq!(
        allocated(&mut rig, "a-a1"),
        ("cpd/pay/api".to_owned(), "/repos/r.lead".to_owned()),
        "the first live Attempt of the Workflow reuses the Lead worktree on its Lead Branch"
    );
}

/// Start Job `key` with `allocation`; the result.
fn start_with_alloc(
    rig: &mut StartRig,
    fakes: &Fakes,
    key: &str,
    allocation: Option<Allocation>,
) -> Result<(), CpdError> {
    fakes.spawn.set_self_exe(std::env::current_exe().unwrap());
    let config = config();
    let catalog = catalog();
    let ctx = StartCtx {
        home: &rig.home,
        config: &config,
        catalog: &catalog,
        command: "test",
    };
    start_with_allocation(
        &fakes.ports,
        &mut rig.store,
        &ctx,
        &PlanId::parse("pay").unwrap(),
        &JobKey::parse(key).unwrap(),
        allocation,
    )
    .map(|_| ())
}

fn attempt_rows(rig: &mut StartRig, id: &str) -> i64 {
    rig.store
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT count(*) FROM attempts WHERE plan = 'pay' AND id = ?1",
                [id],
                |r| r.get(0),
            )?)
        })
        .unwrap()
}

#[test]
fn a_live_linear_attempt_refuses_the_start_of_a_sibling_cas_lost() {
    let mut rig = start_rig(false);
    let fakes = Fakes::new();
    start_attempt(&mut rig, &fakes, "a");
    let err = start_with_alloc(&mut rig, &fakes, "b", None).unwrap_err();
    assert_eq!(err.code(), ErrorCode::CasLost);
    assert_eq!(attempt_rows(&mut rig, "b-a1"), 0);
}

#[test]
fn a_live_linear_attempt_refuses_an_explicit_parallel_allocation_cas_lost() {
    let mut rig = start_rig(false);
    let fakes = Fakes::new();
    start_attempt(&mut rig, &fakes, "a");
    let err = start_with_alloc(&mut rig, &fakes, "b", Some(Allocation::Parallel)).unwrap_err();
    assert_eq!(err.code(), ErrorCode::CasLost);
    assert_eq!(attempt_rows(&mut rig, "b-a1"), 0);
}

#[test]
fn a_live_parallel_attempt_takes_a_parallel_sibling() {
    let mut rig = start_rig(true);
    let fakes = Fakes::new();
    start_attempt(&mut rig, &fakes, "b");
    assert_eq!(
        allocated(&mut rig, "b-a1"),
        ("cpd/pay--b-a1".to_owned(), "/repos/r.pay--b-a1".to_owned()),
        "a sibling of a live parallel Attempt gets its own worktree and branch"
    );
}

#[test]
fn a_failed_linear_attempt_resets_the_lead_branch() {
    let fakes = Fakes::new();
    let mut universe = Universe::new(fakes.clock.clone());
    universe.set_global("user.name", "cpd test");
    universe.set_global("user.email", "test@cpd.invalid");
    let universe = Arc::new(Mutex::new(universe));
    let route = Arc::new(GitRoute::new(universe.clone()));
    let _ = (*fakes.runner).clone().with_git(&route);
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let repo = Repo::init(&universe, &root.join("r"));
    repo.write("f.txt", "base\n");
    let base = repo.commit("base");
    let lead = "cpd/pay/api";
    repo.branch(lead);
    // A failed linear Attempt left a commit on the Lead Branch, ahead of its `base_sha`.
    {
        let mut u = universe.lock().unwrap();
        let blob = u.blob(repo.id(), b"failed\n");
        let tree = u.tree(repo.id(), &[("f.txt", &blob)]);
        let commit = u.commit(
            repo.id(),
            &tree,
            std::slice::from_ref(&base),
            "the failed Attempt",
        );
        u.set_ref(
            repo.id(),
            &format!("refs/heads/{lead}"),
            &commit,
            "commit: failed",
        );
    }
    let worktree = root.join("r.lead");
    universe
        .lock()
        .unwrap()
        .add_worktree(repo.id(), &worktree, Head::Branch(lead.to_owned()));
    let ports = Ports {
        clock: fakes.ports.clock.clone(),
        runner: fakes.ports.runner.clone(),
        spawn: fakes.ports.spawn.clone(),
        ..Ports::real()
    };
    let result = Ports::scope(&ports, || {
        super::run::reset_linear_lead_branch(&worktree, lead, base.as_str())
    });
    assert!(result.is_ok(), "the reset failed: {result:?}");
    assert_eq!(
        universe
            .lock()
            .unwrap()
            .ref_oid(repo.id(), &format!("refs/heads/{lead}")),
        Some(base),
        "a failed linear Attempt resets the Lead Branch to its base_sha"
    );
}

#[test]
fn a_failed_linear_attempt_removes_the_untracked_files_it_left() {
    let fakes = Fakes::new();
    let mut universe = Universe::new(fakes.clock.clone());
    universe.set_global("user.name", "cpd test");
    universe.set_global("user.email", "test@cpd.invalid");
    let universe = Arc::new(Mutex::new(universe));
    let route = Arc::new(GitRoute::new(universe.clone()));
    let _ = (*fakes.runner).clone().with_git(&route);
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let repo = Repo::init(&universe, &root.join("r"));
    repo.write("f.txt", "base\n");
    repo.write(".gitignore", "ignored.log\n");
    let base = repo.commit("base");
    let lead = "cpd/pay/api";
    repo.branch(lead);
    let worktree = root.join("r.lead");
    universe
        .lock()
        .unwrap()
        .add_worktree(repo.id(), &worktree, Head::Branch(lead.to_owned()));
    // The failed Attempt left an untracked file, an untracked directory and an ignored file.
    for (path, text) in [
        ("new.rs", "n\n"),
        ("newdir/x.txt", "x\n"),
        ("ignored.log", "i\n"),
    ] {
        let file = worktree.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    let ports = Ports {
        clock: fakes.ports.clock.clone(),
        runner: fakes.ports.runner.clone(),
        spawn: fakes.ports.spawn.clone(),
        ..Ports::real()
    };
    let result = Ports::scope(&ports, || {
        super::run::reset_linear_lead_branch(&worktree, lead, base.as_str())
    });
    assert!(result.is_ok(), "the reset failed: {result:?}");
    assert!(
        !worktree.join("new.rs").exists(),
        "the untracked file new.rs is still there"
    );
    assert!(
        !worktree.join("newdir/x.txt").exists(),
        "the untracked file newdir/x.txt is still there"
    );
    assert!(
        worktree.join("ignored.log").exists(),
        "the ignored file was removed"
    );
    let status = Ports::scope(&ports, || {
        crate::git::run::ok(&worktree, &["--no-optional-locks", "status", "--porcelain"])
    });
    assert_eq!(status.unwrap(), "", "the worktree is not clean");
}

// Dirty Lead worktree setup reporting regression.
