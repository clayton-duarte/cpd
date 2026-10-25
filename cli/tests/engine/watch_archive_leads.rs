//! Engine step E14, the archive of an asleep Lead (contract §3.7, §2.2 "Archived", §8.2
//! `timeouts.archive_sleep_seconds`; hs-d630, hs-d633), on a prepared database with the fake tmux.
//! E14 archives a Lead (or Manager) Session whose `asleep_at` is older than the configured threshold, taking its
//! whole Plan through the `plan archive` cascade, and leaves a Lead asleep inside the threshold alone. Every test
//! has a control: a Lead that E14 does archive, so that "the recent one stays" is asserted after E14 showed it can
//! archive.

use crate::common;

use std::fs;
use std::process::{Child, Stdio};

use common::{CPD, TestEnv, wait_until};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::store::{Store, WriteTxn};
use time::OffsetDateTime;
use time::macros::format_description;

/// A time `seconds` ago, as cpd writes times (contract §1.3).
fn ago(seconds: i64) -> String {
    (OffsetDateTime::now_utc() - time::Duration::seconds(seconds))
        .format(format_description!("[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z"))
        .unwrap()
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

struct Engine(Child);

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Context `ctx`, a tick of `common::TICK_SECONDS`, `timeouts.archive_sleep_seconds` as given (the default when
/// `None`), the fake tmux with one session `x` and a fake `gh` that finds no pull requests.
struct Fx {
    env: TestEnv,
}

fn fx(archive_sleep_seconds: Option<u32>) -> Fx {
    let env = TestEnv::new();
    let threshold = archive_sleep_seconds.map(|n| format!("archive_sleep_seconds = {n}\n")).unwrap_or_default();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\n[watch]\ntick_seconds = {}\n[timeouts]\nsleep_seconds = 1\n{threshold}[tmux]\nsocket = \"{}\"\n",
            common::TICK_SECONDS,
            env.tmux_socket()
        ),
    )
    .unwrap();
    env.fake("cpd", &format!("exec '{CPD}' \"$@\""));
    env.fake_gh(r#"echo '{"data":{"search":{"issueCount":0,"nodes":[]},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}'"#);
    env.fake_tmux();
    let out = env.tmux(&["new-session", "-d", "-s", "x", "-P", "-F", "#{pane_id}", "sleep", "600"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let fx = Fx { env };
    fx.fixture("INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');");
    fx
}

impl Fx {
    fn store(&self) -> Store {
        Store::open(&CpdHome::resolve(Some(self.env.cpd_home().into()), None).unwrap(), 5000).unwrap()
    }

    fn fixture(&self, sql: &str) {
        self.store().write("fixture", |tx| exec(tx, sql)).unwrap();
    }

    /// Plan `plan` of `ctx`, done two hours ago, its Lead `<plan>-lead` and one side fork `<plan>-fork`, both
    /// `stopped` and asleep `asleep_seconds` ago.
    fn plan(&self, plan: &str, asleep_seconds: i64) {
        let asleep = ago(asleep_seconds);
        self.fixture(&format!(
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, asleep_at)
               VALUES ('{plan}-lead', 'lead', 'ctx', '{plan}', 'Lead {plan}', '/', 'O3', 'stopped', '{asleep}', 't', '{asleep}');
             INSERT INTO sessions (id, role, context, plan, forked_from, name, cwd, tier, state, state_at, created_at, asleep_at)
               VALUES ('{plan}-fork', 'lead', 'ctx', '{plan}', '{plan}-lead', 'Fork {plan}', '/', 'O3', 'stopped', '{asleep}', 't', '{asleep}');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
               VALUES ('{plan}', 'ctx', '{plan}', 'done', '{plan}-lead', 't', 't', '{done}');",
            done = ago(120 * 60)
        ));
    }

    fn start(&self) -> Engine {
        let child = self.env.command(CPD).arg("watch").envs([common::PR_POLL_EVERY_TICK]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let engine = Engine(child);
        wait_until("watch-started", || self.env.query::<i64>("SELECT count(*) FROM events WHERE kind = 'watch-started'") == 1);
        engine
    }

    fn plan_archived_at(&self, plan: &str) -> Option<String> {
        self.env.query(&format!("SELECT archived_at FROM plans WHERE id = '{plan}'"))
    }

    fn session_archived_at(&self, session: &str) -> Option<String> {
        self.env.query(&format!("SELECT archived_at FROM sessions WHERE id = '{session}'"))
    }

    fn session_asleep_at(&self, session: &str) -> Option<String> {
        self.env.query(&format!("SELECT asleep_at FROM sessions WHERE id = '{session}'"))
    }

    /// The Plans E14 archived, in order.
    fn plan_archived_events(&self) -> Vec<String> {
        self.env.read(|conn| {
            let mut stmt = conn.prepare("SELECT plan FROM events WHERE kind = 'plan-archived' ORDER BY id")?;
            Ok(stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?)
        })
    }

    /// The Sessions E14 archived, in order.
    fn session_archived_events(&self) -> Vec<String> {
        self.env.read(|conn| {
            let mut stmt = conn.prepare("SELECT session FROM events WHERE kind = 'session-archived' ORDER BY id")?;
            Ok(stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?)
        })
    }
}

#[test]
fn a_lead_asleep_past_the_threshold_archives_its_done_plan() {
    let fx = fx(None);
    fx.plan("pay", 2 * 24 * 3600);
    fx.plan("recent", 60);
    let _engine = fx.start();

    wait_until("the done Plan of a Lead asleep past the threshold is archived", || fx.plan_archived_at("pay").is_some());
    fx.env.wait_whole_ticks(3);

    // The Plan, its Lead and its side fork are archived; archiving wakes the Lead (`asleep_at` cleared).
    assert!(fx.plan_archived_at("pay").is_some());
    assert!(fx.session_archived_at("pay-lead").is_some());
    assert!(fx.session_archived_at("pay-fork").is_some());
    assert_eq!(fx.session_asleep_at("pay-lead"), None, "archiving wakes the Session (contract §2.2 'Asleep')");
    assert_eq!(fx.session_asleep_at("pay-fork"), None);
    assert_eq!(fx.plan_archived_events(), ["pay"]);
    let mut sessions = fx.session_archived_events();
    sessions.sort();
    assert_eq!(sessions, ["pay-fork", "pay-lead"]);

    // The Lead asleep inside the threshold stays asleep and unarchived.
    assert_eq!(fx.plan_archived_at("recent"), None);
    assert_eq!(fx.session_archived_at("recent-lead"), None);
    assert!(fx.session_asleep_at("recent-lead").is_some(), "a Lead asleep inside the threshold stays");
}

#[test]
fn the_configured_threshold_decides() {
    // `archive_sleep_seconds = 1`: a Lead asleep 2 s is past the threshold and its Plan is archived.
    let short = fx(Some(1));
    short.plan("past", 2);
    let _short_engine = short.start();
    wait_until("the Plan of a Lead asleep 2 s under a 1 s threshold is archived", || short.plan_archived_at("past").is_some());
    assert!(short.session_archived_at("past-lead").is_some());
    assert_eq!(short.session_asleep_at("past-lead"), None);

    // The default (86400): the same 2 s of sleep is inside it, so the Plan stays.
    let default = fx(None);
    default.plan("past", 2);
    let _default_engine = default.start();
    default.env.wait_whole_ticks(3);
    assert_eq!(default.plan_archived_at("past"), None, "the default threshold does not archive a Lead asleep 2 s");
    assert_eq!(default.session_archived_at("past-lead"), None);
    assert!(default.session_asleep_at("past-lead").is_some());
    assert!(default.plan_archived_events().is_empty());
}
