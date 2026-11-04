//! Tests of E23, the PR post bump (contract §3.7 E23, §4.4): a due post gets `Bibbidi-bobbidi-bump!` as a reply in its thread,
//! broadcast to the channel, through `slack-post.ts`; the sender is a `FakeRunner`, the time a `ManualClock`.

use std::path::Path;
use std::time::Duration;

use cpd_core::clock::timestamp;
use cpd_core::host::runner::{Call, FakeRunner, Reply};
use cpd_core::ports::{Fakes, Ports};
use time::OffsetDateTime;

use super::super::engine::incident_holds;
use super::super::report::Step;
use super::super::testkit::Fx;
use super::run;

const REPEAT: u64 = 600;
const DAY: u64 = 86_400;
const OK: &str = r#"{"ok":true,"channel":"C0POLICY1","ts":"1.2","link":"l"}"#;
const FAILED: &str = r#"{"ok":false,"error":{"code":"slack_failed","message":"Slack said no."}}"#;
/// The most ticks a test spins while a detached send answers; a step that works needs a handful.
const SPINS: usize = 200_000;

/// An engine on the fake ports with a pi install on `PATH`, Plan `pay` and no post yet; `dir` keeps the pi install.
struct Rig {
    fx: Fx,
    fakes: Fakes,
    _dir: tempfile::TempDir,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let adapter = dir.path().join("adapter");
    std::fs::create_dir_all(adapter.join("lib")).unwrap();
    std::fs::write(adapter.join("package.json"), "{}").unwrap();
    std::fs::create_dir_all(dir.path().join("bin")).unwrap();
    let pi = dir.path().join("bin/pi");
    std::fs::write(&pi, "").unwrap();
    std::fs::set_permissions(
        &pi,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .unwrap();
    let install = dir.path().join("install");
    std::fs::create_dir_all(&install).unwrap();
    std::fs::write(install.join("current-version"), "1.2.0\n").unwrap();
    for package in ["pi-coding-agent", "pi-ai"] {
        let dist = install
            .join("releases/1.2.0/node_modules/@earendil-works")
            .join(package)
            .join("dist");
        std::fs::create_dir_all(&dist).unwrap();
        std::fs::write(dist.join("index.js"), "").unwrap();
    }
    let mut fx = Fx::with_plan(&format!(
        "pi_adapter = \"{}\"\n[watch]\nerror_repeat_seconds = {REPEAT}\n",
        adapter.display()
    ));
    let fakes = Fakes::new();
    fx.engine.ports = fakes.ports.clone();
    // SAFETY: nextest runs one test per process.
    unsafe {
        std::env::set_var("CPD_HOME", fx.engine.home.root());
        let path = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![dir.path().join("bin")];
        paths.extend(std::env::split_paths(&path));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    }
    Rig {
        fx,
        fakes,
        _dir: dir,
    }
}

impl Rig {
    /// The fake clock's time `seconds` ago (negative: ahead), as cpd writes times.
    fn ago(&self, seconds: i64) -> String {
        timestamp(self.fakes.clock_utc() - time::Duration::seconds(seconds))
    }

    /// Workflow `key`, open, with an open PR nobody reviewed and a post (channel `C0POLICY1`, thread `ts-<key>`) first made
    /// `seconds` ago on the fake clock; `last_posted_at` equals `posted_at`, as a first post sets it.
    fn post(&mut self, key: &str, seconds: i64) {
        let at = self.ago(seconds);
        self.fx.fixture(&format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
               VALUES ('a/b', (SELECT count(*) + 1 FROM pull_requests), 'u', 't', 0, 'open', 'none', 'none', 't', 't');
             INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at, pr_number, pr_url, pr_opened_at, pull_request)
               VALUES ('pay', '{key}', '/r', 'main', 'open', 'cpd/pay/{key}', '/r/lead', cpd_txn(), 't', 't', 7, 'u', 't', (SELECT max(id) FROM pull_requests));
             INSERT INTO pr_posts (plan, workflow, channel, ts, link, posted_at, last_posted_at, auto_merge)
               VALUES ('pay', '{key}', 'C0POLICY1', 'ts-{key}', 'l', '{at}', '{at}', 'pending');"
        ));
    }

    fn last_posted_at(&mut self, key: &str) -> String {
        self.fx.query(&format!(
            "SELECT last_posted_at FROM pr_posts WHERE workflow = '{key}'"
        ))
    }

    fn runner(&self) -> FakeRunner {
        (*self.fakes.runner).clone()
    }

    /// Every call of the sender so far.
    fn calls(&self) -> Vec<Call> {
        self.fakes.runner.calls()
    }

    fn tick(&mut self) {
        let ports: Ports = self.fakes.ports.clone();
        let engine = &mut self.fx.engine;
        Ports::scope(&ports, || run(engine));
    }

    /// Ticks until `done`; fails when it does not hold after [`SPINS`] ticks. No sleep: the detached send answers in microseconds.
    fn until(&mut self, what: &str, done: impl Fn(&mut Rig) -> bool) {
        for _ in 0..SPINS {
            self.tick();
            if done(self) {
                return;
            }
            std::thread::yield_now();
        }
        panic!("never: {what}");
    }

    /// Ticks a few times with nothing in flight and expects no new call.
    fn quiet(&mut self) {
        let before = self.calls().len();
        for _ in 0..5 {
            self.tick();
        }
        assert_eq!(self.calls().len(), before, "no call was expected");
    }

    fn advance(&self, seconds: u64) {
        self.fakes.clock.advance(Duration::from_secs(seconds));
    }
}

trait ClockUtc {
    fn clock_utc(&self) -> OffsetDateTime;
}

impl ClockUtc for Fakes {
    fn clock_utc(&self) -> OffsetDateTime {
        self.ports.clock.utc()
    }
}

fn answers_ok(rig: &Rig) {
    rig.runner().on("node", |_| true, Reply::ok(OK));
}

fn has_pair(call: &Call, name: &str, value: &str) -> bool {
    call.args.windows(2).any(|w| w[0] == name && w[1] == value)
}

#[test]
fn a_due_post_is_bumped_with_thread_ts_and_broadcast_and_last_posted_at_moves() {
    let mut rig = rig();
    rig.post("a", (DAY + 60) as i64);
    answers_ok(&rig);
    let before = rig.last_posted_at("a");

    rig.until("the post was bumped", |r| r.last_posted_at("a") != before);

    let calls = rig.calls();
    assert_eq!(calls.len(), 1, "one send: {calls:?}");
    let call = &calls[0];
    assert!(
        call.args
            .iter()
            .any(|a| Path::new(a).ends_with("lib/slack-post.ts")),
        "the helper is slack-post.ts: {:?}",
        call.args
    );
    assert!(has_pair(call, "--channel", "C0POLICY1"), "{:?}", call.args);
    assert!(has_pair(call, "--thread-ts", "ts-a"), "{:?}", call.args);
    assert!(
        has_pair(call, "--reply-broadcast", "true"),
        "{:?}",
        call.args
    );
    assert_eq!(call.stdin.as_deref(), Some(&b"Bibbidi-bobbidi-bump!"[..]));
    assert_eq!(call.deadline, Duration::from_secs(120));
    let first: String = rig
        .fx
        .query("SELECT posted_at FROM pr_posts WHERE workflow = 'a'");
    assert_ne!(first, rig.last_posted_at("a"), "posted_at is never changed");
}

#[test]
fn a_post_younger_than_24h_is_not_bumped() {
    let mut rig = rig();
    rig.post("a", (DAY - 60) as i64);
    answers_ok(&rig);
    let before = rig.last_posted_at("a");

    rig.quiet();

    assert!(rig.calls().is_empty(), "{:?}", rig.calls());
    assert_eq!(rig.last_posted_at("a"), before);
}

#[test]
fn a_failed_send_leaves_last_posted_at_and_is_not_retried_before_error_repeat_seconds() {
    let mut rig = rig();
    rig.post("a", (DAY + 60) as i64);
    rig.runner()
        .on("node", |_| true, Reply::exit(1, FAILED, ""));
    let before = rig.last_posted_at("a");

    rig.until("the failure was reported", |r| r.fx.count("error") == 1);
    assert_eq!(rig.calls().len(), 1);
    assert_eq!(
        rig.last_posted_at("a"),
        before,
        "a failure leaves the row due"
    );

    rig.advance(REPEAT - 1);
    rig.quiet();
    assert_eq!(rig.calls().len(), 1, "not retried before the repeat period");

    rig.advance(1);
    rig.until("the row was retried", |r| r.calls().len() == 2);
    assert_eq!(rig.last_posted_at("a"), before);
}

#[test]
fn one_failing_row_does_not_block_the_next_due_row() {
    let mut rig = rig();
    rig.post("a", (DAY + 600) as i64);
    rig.post("b", (DAY + 60) as i64);
    rig.runner().on(
        "node",
        |c| has_pair(c, "--thread-ts", "ts-a"),
        Reply::exit(1, FAILED, ""),
    );
    answers_ok(&rig);
    let (a, b) = (rig.last_posted_at("a"), rig.last_posted_at("b"));

    rig.until("b was bumped", |r| r.last_posted_at("b") != b);

    assert_eq!(rig.last_posted_at("a"), a, "a failed and stays due");
    let calls = rig.calls();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert!(
        has_pair(&calls[0], "--thread-ts", "ts-a"),
        "the oldest goes first"
    );
    assert!(has_pair(&calls[1], "--thread-ts", "ts-b"));
}

#[test]
fn incident_mode_holds_the_step() {
    let mut rig = rig();
    rig.post("a", (DAY + 60) as i64);
    answers_ok(&rig);
    rig.fx.fixture(
        "INSERT INTO incident (id, started_at, started_by, reason, started_txn) VALUES (1, 't', 'mgr', 'why', cpd_txn());",
    );
    let before = rig.last_posted_at("a");

    assert!(incident_holds(&mut rig.fx.engine, Step::E23));
    rig.quiet();

    assert!(rig.calls().is_empty(), "{:?}", rig.calls());
    assert_eq!(rig.last_posted_at("a"), before);
}

#[test]
fn a_second_bump_is_sent_24h_after_the_first() {
    let mut rig = rig();
    rig.post("a", (DAY + 60) as i64);
    answers_ok(&rig);
    let before = rig.last_posted_at("a");
    rig.until("the first bump", |r| r.last_posted_at("a") != before);
    assert_eq!(rig.calls().len(), 1);
    // The store stamps a transaction with the system's time; this test lives on the fake clock, so the first bump's
    // time is set to the fake clock's now.
    let now = rig.ago(0);
    rig.fx.fixture(&format!(
        "UPDATE pr_posts SET last_posted_at = '{now}' WHERE workflow = 'a'"
    ));
    rig.advance(DAY - 1);
    rig.quiet();
    assert_eq!(rig.calls().len(), 1, "not due before 24h");

    rig.advance(1);
    rig.until("the second bump", |r| r.calls().len() == 2);
}
