//! Future paired creation and unchanged historical orphans (air-d174, air-d175).
//! Commands run in-process on fake ports. SQLite and temporary files are the authored
//! store's fixture, never a real tmux, subprocess, network, or production home.

use std::ffi::OsString;
use std::sync::{Arc, Barrier, Mutex, MutexGuard};

use clap::Parser;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::host::runner::Reply;
use cpd_core::output::Output;
use cpd_core::ports::{Fakes, Ports};
use cpd_core::store::Store;
use rusqlite::types::Value;

static ENV_LOCK: Mutex<()> = Mutex::new(());
const CONTEXT_ARGS: &[&str] = &["new", "ctx", "--title", "Context", "--owner", "acme"];
#[derive(Parser)]
struct PlanCli {
    #[command(subcommand)]
    command: cpd_core::plan::Command,
}
#[derive(Parser)]
struct ContextCli {
    #[command(subcommand)]
    command: cpd_core::context::Command,
}
#[derive(Parser)]
struct JobCli {
    #[command(subcommand)]
    command: cpd_core::job::Command,
}

struct Fixture {
    home: CpdHome,
    fakes: Fakes,
    saved: Vec<(&'static str, Option<OsString>)>,
    _dir: tempfile::TempDir,
    _lock: MutexGuard<'static, ()>,
}
impl Fixture {
    fn new(manager_runner: &str) -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
        std::fs::create_dir_all(home.root().join("policy/acme")).unwrap();
        std::fs::write(home.root().join("policy/acme/owner.toml"), "").unwrap();
        std::fs::write(
            home.config(),
            format!("version = 1\nmachine = \"m1\"\nmanager_runner = \"{manager_runner}\"\nlead_runner = \"durable\"\n"),
        )
        .unwrap();
        let saved = [
            "CPD_HOME",
            "HOME",
            "CPD_SESSION",
            "PI_SESSION_ID",
            "CPD_PLAN",
            "CPD_ATTEMPT",
        ]
        .into_iter()
        .map(|key| (key, std::env::var_os(key)))
        .collect();
        // Only these two tests use this binary's environment, serialized by ENV_LOCK;
        // spawned threads only read it, and are joined before the fixture is dropped.
        unsafe {
            std::env::set_var("CPD_HOME", home.root());
            std::env::set_var("HOME", dir.path());
            for key in ["CPD_SESSION", "PI_SESSION_ID", "CPD_PLAN", "CPD_ATTEMPT"] {
                std::env::remove_var(key);
            }
        }
        let fixture = Self {
            home,
            fakes: Fakes::new(),
            saved,
            _dir: dir,
            _lock: lock,
        };
        drop(fixture.store());
        fixture
    }
    fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }
    fn sql(&self, sql: &str) {
        self.store()
            .write("fixture", |tx| {
                tx.execute_batch(sql)?;
                Ok(())
            })
            .unwrap();
    }
    fn rows(&self, sql: &str) -> Vec<Vec<Value>> {
        self.store()
            .read(|conn| {
                let mut stmt = conn.prepare(sql)?;
                let n = stmt.column_count();
                let rows = stmt.query_map([], |row| (0..n).map(|i| row.get(i)).collect())?;
                Ok(rows.collect::<rusqlite::Result<Vec<Vec<Value>>>>()?)
            })
            .unwrap()
    }
    fn count(&self, sql: &str) -> i64 {
        self.store()
            .read(|conn| Ok(conn.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }
    fn context(&self, args: &[&str]) -> Result<Output, CpdError> {
        context(&self.fakes.ports, args)
    }
    fn plan(&self, args: &[&str]) -> Result<Output, CpdError> {
        plan(&self.fakes.ports, args)
    }
    fn job(&self, args: &[&str]) -> Result<Output, CpdError> {
        let cmd = JobCli::parse_from(std::iter::once("cpd").chain(args.iter().copied())).command;
        Ports::scope(&self.fakes.ports, || cpd_core::job::run(cmd))
    }
    fn context_fixture(&self) {
        self.sql("INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx','Context','acme','t','m1');");
    }
    fn orphan(&self) {
        self.context_fixture();
        self.sql(&format!(
            "INSERT INTO plans (id, context, title, phase, lead_session, created_at, done_at) VALUES ('legacy','ctx','Existing plan','done',NULL,'t','t');
             INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, kind, status, appended, fingerprint, created_txn, created_at)
             VALUES ('legacy',NULL,'scout','Scout','S2','g','c','[\"x\"]','[\"y\"]','[]','scout','queued',1,'{}',cpd_txn(),'t');
             INSERT INTO events (txn, at, kind, context, plan, text, data) VALUES (cpd_txn(),'t','plan-created','ctx','legacy','historical event','{{}}');
             INSERT INTO context_records (context,id,kind,at,machine,by_plan,text) VALUES ('ctx','m1-n1','note','t','m1','legacy','historical checkpoint');",
            "a".repeat(64)
        ));
    }
    fn manager(&self) -> String {
        self.store().read(|c| Ok(c.query_row(
            "SELECT s.id FROM contexts c JOIN sessions s ON s.context=c.id WHERE c.id='ctx' AND s.role='manager' AND s.plan IS NULL AND s.archived_at IS NULL", [], |r| r.get(0)
        )?)).expect("the new Context must have a real Manager Session")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for (key, value) in &self.saved {
            unsafe {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
}
fn plan(ports: &Ports, args: &[&str]) -> Result<Output, CpdError> {
    let cmd = PlanCli::parse_from(std::iter::once("cpd").chain(args.iter().copied())).command;
    Ports::scope(ports, || cpd_core::plan::run(cmd))
}
fn context(ports: &Ports, args: &[&str]) -> Result<Output, CpdError> {
    let cmd = ContextCli::try_parse_from(std::iter::once("cpd").chain(args.iter().copied()))
        .expect("context creation must accept the documented flags")
        .command;
    Ports::scope(ports, || cpd_core::context::run(cmd))
}
fn assert_success(result: Result<Output, CpdError>) {
    if let Err(e) = result {
        panic!("expected success, got {e:?}");
    }
}

#[test]
fn paired_future_context_creation() {
    // An actual database transaction failure rolls back the Context, Manager and events.
    {
        let f = Fixture::new("durable");
        // Exhaust the history AUTOINCREMENT key: Context insertion cannot complete.
        f.sql("UPDATE sqlite_sequence SET seq=9223372036854775807 WHERE name='history'");
        assert!(
            f.context(CONTEXT_ARGS).is_err(),
            "the injected database failure must be reported"
        );
        assert_eq!(f.count("SELECT count(*) FROM contexts WHERE id='ctx'"), 0);
        assert_eq!(
            f.count("SELECT count(*) FROM sessions WHERE context='ctx'"),
            0
        );
        assert_eq!(
            f.count("SELECT count(*) FROM events WHERE context='ctx'"),
            0
        );
        f.sql("UPDATE sqlite_sequence SET seq=(SELECT max(id) FROM history) WHERE name='history'");
        assert_success(f.context(CONTEXT_ARGS));
        f.manager();
    }
    // A matching retry keeps both rows and all history. Conflicts never change them.
    {
        let f = Fixture::new("durable");
        assert_success(f.context(CONTEXT_ARGS));
        let manager = f.manager();
        let events = f.rows("SELECT * FROM events WHERE context='ctx' ORDER BY id");
        assert_success(f.context(CONTEXT_ARGS));
        assert_eq!(f.manager(), manager);
        assert_eq!(
            f.rows("SELECT * FROM events WHERE context='ctx' ORDER BY id"),
            events
        );
        assert_eq!(
            f.count("SELECT count(*) FROM sessions WHERE context='ctx' AND role='manager'"),
            1
        );
        assert!(
            f.context(&["new", "ctx", "--title", "Different", "--owner", "acme"])
                .is_err()
        );
        assert_eq!(f.manager(), manager);
        assert_eq!(
            f.rows("SELECT * FROM events WHERE context='ctx' ORDER BY id"),
            events
        );
    }
    // External provisioning is outside the transaction: failure retains a retryable pair.
    {
        let f = Fixture::new("pi");
        (*f.fakes.runner)
            .clone()
            .on(
                "tmux",
                |call| call.args.iter().any(|arg| arg == "has-session"),
                Reply::ok(""),
            )
            .on(
                "tmux",
                |call| call.args.iter().any(|arg| arg == "list-panes"),
                Reply::ok(""),
            )
            .on(
                "tmux",
                |call| call.args.iter().any(|arg| arg == "new-window"),
                Reply::exit(1, "", "injected tmux failure"),
            );
        assert!(
            f.context(CONTEXT_ARGS).is_err(),
            "fake tmux failure must be reported"
        );
        let manager = f.manager();
        assert!(f.context(CONTEXT_ARGS).is_err());
        assert_eq!(f.manager(), manager);
        assert_eq!(
            f.count("SELECT count(*) FROM sessions WHERE context='ctx' AND role='manager'"),
            1
        );
    }
    // Concurrent matching creation uses one pair, with no duplicate registration history.
    {
        let f = Fixture::new("durable");
        let barrier = Arc::new(Barrier::new(3));
        std::thread::scope(|scope| {
            let mut threads = Vec::new();
            for _ in 0..2 {
                let barrier = barrier.clone();
                let ports = f.fakes.ports.clone();
                threads.push(scope.spawn(move || {
                    barrier.wait();
                    context(&ports, CONTEXT_ARGS)
                }));
            }
            barrier.wait();
            for thread in threads {
                assert_success(thread.join().unwrap());
            }
        });
        f.manager();
        assert_eq!(f.count("SELECT count(*) FROM contexts WHERE id='ctx'"), 1);
        assert_eq!(
            f.count("SELECT count(*) FROM sessions WHERE context='ctx' AND role='manager'"),
            1
        );
        assert_eq!(
            f.count("SELECT count(*) FROM events WHERE context='ctx' AND kind='context-created'"),
            1
        );
        assert_eq!(
            f.count(
                "SELECT count(*) FROM events WHERE context='ctx' AND kind='session-registered'"
            ),
            1
        );
    }
    // Web's tree creation requests by-user: no first-message or started turn before a prompt.
    {
        let f = Fixture::new("durable");
        let args = [
            "new",
            "ctx",
            "--title",
            "Context",
            "--owner",
            "acme",
            "--by-user",
        ];
        assert_success(f.context(&args));
        let manager = f.manager();
        assert_eq!(f.count("SELECT count(*) FROM sessions WHERE context='ctx' AND role='manager' AND by_user=1 AND state='stopped' AND file IS NULL"), 1);
        assert!(!f.home.root().join("run/ctx/manager-message.md").exists());
        let events = f.rows("SELECT * FROM events WHERE context='ctx' ORDER BY id");
        assert_success(f.context(&args));
        assert_eq!(f.manager(), manager);
        assert_eq!(
            f.rows("SELECT * FROM events WHERE context='ctx' ORDER BY id"),
            events
        );
        assert!(
            f.context(CONTEXT_ARGS).is_err(),
            "a retry cannot silently change by-user mode"
        );
    }
}

#[test]
fn paired_future_shaping_creation() {
    // Historical done orphans, including every field and their original history, are not repaired.
    {
        let f = Fixture::new("durable");
        f.orphan();
        let before = f.rows("SELECT * FROM plans WHERE id='legacy'");
        let jobs = f.rows("SELECT * FROM jobs WHERE plan='legacy' ORDER BY key");
        let events = f.rows("SELECT * FROM events WHERE plan='legacy' ORDER BY id");
        let checkpoints =
            f.rows("SELECT * FROM context_records WHERE by_plan='legacy' ORDER BY seq");
        let history = f.rows("SELECT * FROM history ORDER BY id");
        let args = [
            "new",
            "legacy",
            "--context",
            "ctx",
            "--title",
            "Existing plan",
            "--runner",
            "durable",
        ];
        let _ = f.plan(&args);
        assert_eq!(f.rows("SELECT * FROM plans WHERE id='legacy'"), before);
        assert_eq!(
            f.rows("SELECT * FROM jobs WHERE plan='legacy' ORDER BY key"),
            jobs
        );
        assert_eq!(
            f.rows("SELECT * FROM events WHERE plan='legacy' ORDER BY id"),
            events
        );
        assert_eq!(
            f.rows("SELECT * FROM context_records WHERE by_plan='legacy' ORDER BY seq"),
            checkpoints
        );
        assert_eq!(f.rows("SELECT * FROM history ORDER BY id"), history);
        assert_eq!(
            f.count("SELECT count(*) FROM sessions WHERE plan='legacy' AND role='lead'"),
            0
        );
        assert_success(f.plan(&["show", "--plan", "legacy"]));
    }
    // Only the same Context's live Manager creates a shaping Plan and real Lead, atomically.
    {
        let f = Fixture::new("durable");
        f.context_fixture();
        let manager = "00000000-0000-4000-8000-000000000001";
        f.sql(&format!("INSERT INTO sessions (id,role,context,plan,name,cwd,tier,state,state_at,created_at) VALUES ('{manager}','manager','ctx',NULL,'ctx','/','O3','stopped','t','t');"));
        let args = [
            "new",
            "fresh",
            "--context",
            "ctx",
            "--title",
            "Fresh",
            "--shaping",
        ];
        assert!(
            f.plan(&args).is_err(),
            "a User is not the Context's Manager"
        );
        unsafe { std::env::set_var("CPD_SESSION", manager) };
        f.sql("UPDATE sqlite_sequence SET seq=9223372036854775807 WHERE name='history'");
        assert!(f.plan(&args).is_err());
        assert_eq!(f.count("SELECT count(*) FROM plans WHERE id='fresh'"), 0);
        assert_eq!(
            f.count("SELECT count(*) FROM sessions WHERE plan='fresh'"),
            0
        );
        assert_eq!(f.count("SELECT count(*) FROM events WHERE plan='fresh'"), 0);
        f.sql("UPDATE sqlite_sequence SET seq=(SELECT max(id) FROM history) WHERE name='history'");
        assert_success(f.plan(&args));
        assert_eq!(f.count("SELECT count(*) FROM plans p JOIN sessions s ON s.id=p.lead_session WHERE p.id='fresh' AND p.phase='shaping' AND s.role='lead' AND s.context=p.context AND s.plan=p.id"), 1);
        let pair = f.rows("SELECT p.id,p.lead_session,s.by_user FROM plans p JOIN sessions s ON s.id=p.lead_session WHERE p.id='fresh'");
        let events = f.rows("SELECT * FROM events WHERE plan='fresh' ORDER BY id");
        assert_success(f.plan(&args));
        assert_eq!(f.rows("SELECT p.id,p.lead_session,s.by_user FROM plans p JOIN sessions s ON s.id=p.lead_session WHERE p.id='fresh'"), pair);
        assert_eq!(
            f.rows("SELECT * FROM events WHERE plan='fresh' ORDER BY id"),
            events
        );
        assert_eq!(
            f.count("SELECT count(*) FROM sessions WHERE role='lead' AND plan='fresh'"),
            1
        );
        // Rename is shaping work: allowed to the same Context's Manager only in shaping.
        // tmux list-panes is mocked, never a real server.
        (*f.fakes.runner)
            .clone()
            .on("tmux", |_| true, Reply::ok(""));
        assert_success(f.plan(&["rename", "--plan", "fresh", "--title", "Shaped"]));
        let err = f.job(&["list", "--plan", "fresh"]).unwrap_err();
        assert_eq!(err.code(), cpd_core::error::ErrorCode::PlanHasLead);
        let other = "00000000-0000-4000-8000-000000000002";
        f.sql(&format!("INSERT INTO contexts (id,title,owner,created_at,created_on) VALUES ('other','Other','acme','t','m1'); INSERT INTO sessions (id,role,context,plan,name,cwd,tier,state,state_at,created_at) VALUES ('{other}','manager','other',NULL,'other','/','O3','stopped','t','t');"));
        unsafe { std::env::set_var("CPD_SESSION", other) };
        let err = f
            .plan(&["rename", "--plan", "fresh", "--title", "Cross context"])
            .unwrap_err();
        assert_eq!(err.code(), cpd_core::error::ErrorCode::PlanOtherContext);
        assert_eq!(
            f.count("SELECT count(*) FROM plans WHERE id='fresh' AND title='Shaped'"),
            1
        );
        unsafe { std::env::set_var("CPD_SESSION", manager) };
        // Move out of shaping through the supported lifecycle transition.
        f.sql("UPDATE plans SET phase='done', done_at='t', locked_at='t' WHERE id='fresh'");
        assert!(
            f.plan(&["rename", "--plan", "fresh", "--title", "Too late"])
                .is_err(),
            "Manager shaping must stop after shaping phase"
        );
        assert_eq!(
            f.count("SELECT count(*) FROM plans WHERE id='fresh' AND title='Shaped'"),
            1
        );
    }
}
