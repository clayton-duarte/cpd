//! Shaping pair provisioning recovers without real processes or external services.

use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard};

use clap::Parser;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::ids::PlanId;
use cpd_core::output::Output;
use cpd_core::ports::{Fakes, Ports};
use cpd_core::store::Store;
use rusqlite::types::Value;

static ENV_LOCK: Mutex<()> = Mutex::new(());
const MANAGER: &str = "00000000-0000-4000-8000-000000000001";

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: cpd_core::plan::Command,
}

struct Fixture {
    home: CpdHome,
    fakes: Fakes,
    saved: Vec<(&'static str, Option<OsString>)>,
    _dir: tempfile::TempDir,
    _lock: MutexGuard<'static, ()>,
}

impl Fixture {
    fn new(runner: &str) -> Self {
        let lock = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
        let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
        std::fs::create_dir_all(home.root().join("policy/acme")).unwrap();
        std::fs::write(home.root().join("policy/acme/owner.toml"), "").unwrap();
        std::fs::write(
            home.config(),
            format!("version = 1\nmachine = \"m1\"\nlead_runner = \"{runner}\"\n"),
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
        // This binary serializes environment access and starts no threads.
        unsafe {
            std::env::set_var("CPD_HOME", home.root());
            std::env::set_var("HOME", dir.path());
            std::env::set_var("CPD_SESSION", MANAGER);
            for key in ["PI_SESSION_ID", "CPD_PLAN", "CPD_ATTEMPT"] {
                std::env::remove_var(key);
            }
        }
        let f = Self {
            home,
            fakes: Fakes::new(),
            saved,
            _dir: dir,
            _lock: lock,
        };
        f.store().write("fixture", |tx| {
            tx.execute_batch(&format!("INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx','Context','acme','t','m1'); INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at) VALUES ('{MANAGER}','manager','ctx','ctx','/','O3','stopped','t','t');"))?;
            Ok(())
        }).unwrap();
        f
    }
    fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }
    fn rows(&self, sql: &str) -> Vec<Vec<Value>> {
        self.store()
            .read(|conn| {
                let mut stmt = conn.prepare(sql)?;
                let n = stmt.column_count();
                let rows = stmt.query_map([], |r| (0..n).map(|i| r.get(i)).collect())?;
                Ok(rows.collect::<rusqlite::Result<_>>()?)
            })
            .unwrap()
    }
    fn plan(&self, flags: &[&str]) -> Result<Output, CpdError> {
        let args = [
            "cpd",
            "new",
            "fresh",
            "--context",
            "ctx",
            "--title",
            "Fresh",
            "--shaping",
        ];
        let command = Cli::try_parse_from(args.into_iter().chain(flags.iter().copied()))
            .unwrap()
            .command;
        Ports::scope(&self.fakes.ports, || cpd_core::plan::run(command))
    }
    fn pair(&self) -> Vec<Vec<Value>> {
        self.rows("SELECT p.id,p.lead_session,s.by_user,s.runner FROM plans p JOIN sessions s ON s.id=p.lead_session WHERE p.id='fresh'")
    }
    fn message(&self) -> std::path::PathBuf {
        self.home.lead_message(&PlanId::parse("fresh").unwrap())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        for (key, value) in &self.saved {
            unsafe {
                match value {
                    Some(v) => std::env::set_var(key, v),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
}

#[test]
fn shaping_message_failure_retries_same_pair() {
    let f = Fixture::new("durable");
    std::fs::create_dir_all(f.message()).unwrap();
    let error = f
        .plan(&[])
        .expect_err("first-message failure must be reported");
    assert!(error.message().to_lowercase().contains("retry"));
    let pair = f.pair();
    assert_eq!(pair.len(), 1);
    let events = f.rows("SELECT * FROM events WHERE plan='fresh' ORDER BY id");
    std::fs::remove_dir(f.message()).unwrap();
    let Output::Success(out) = f.plan(&[]).unwrap() else {
        panic!("expected success")
    };
    assert_eq!(out["already"], true);
    assert_eq!(f.pair(), pair);
    assert_eq!(
        f.rows("SELECT * FROM events WHERE plan='fresh' ORDER BY id"),
        events
    );
    assert!(
        std::fs::read_to_string(f.message())
            .unwrap()
            .contains("Phase shaping:")
    );
}

#[test]
fn shaping_by_user_mode_is_stored_and_conflicts_are_refused() {
    let f = Fixture::new("durable");
    f.plan(&["--by-user"]).unwrap();
    let pair = f.pair();
    assert_eq!(pair[0][2], Value::Integer(1));
    assert!(!f.message().exists());
    let events = f.rows("SELECT * FROM events WHERE plan='fresh' ORDER BY id");
    f.plan(&["--by-user"]).unwrap();
    assert!(f.plan(&[]).is_err());
    assert_eq!(f.pair(), pair);
    assert_eq!(
        f.rows("SELECT * FROM events WHERE plan='fresh' ORDER BY id"),
        events
    );
    assert!(!f.message().exists());
}

#[test]
fn concurrent_shaping_provisioning_serializes_same_id() {
    use std::sync::{Arc, Barrier};

    fn run(ports: &Ports, id: &str) -> Result<Output, CpdError> {
        let command = Cli::parse_from([
            "cpd",
            "new",
            id,
            "--context",
            "ctx",
            "--title",
            "Fresh",
            "--shaping",
        ])
        .command;
        Ports::scope(ports, || cpd_core::plan::run(command))
    }
    let f = Fixture::new("durable");
    // Simultaneous release exercises concurrency, but does not force an internal interleaving.
    let barrier = Arc::new(Barrier::new(3));
    std::thread::scope(|scope| {
        let first_ports = f.fakes.ports.clone();
        let first_barrier = barrier.clone();
        let first = scope.spawn(move || {
            first_barrier.wait();
            run(&first_ports, "fresh")
        });
        let waiter_ports = f.fakes.ports.clone();
        let waiter_barrier = barrier.clone();
        let waiter = scope.spawn(move || {
            waiter_barrier.wait();
            run(&waiter_ports, "fresh")
        });
        barrier.wait();
        let other = run(&f.fakes.ports, "other");
        first.join().unwrap().unwrap();
        waiter.join().unwrap().unwrap();
        other.unwrap();
    });
    assert_eq!(f.pair().len(), 1);
    assert_eq!(
        f.rows("SELECT id FROM sessions WHERE plan='fresh' AND role='lead'")
            .len(),
        1
    );
    for kind in ["plan-created", "session-registered"] {
        assert_eq!(
            f.rows(&format!(
                "SELECT id FROM events WHERE plan='fresh' AND kind='{kind}'"
            ))
            .len(),
            1
        );
    }
    assert_eq!(
        f.rows("SELECT id FROM context_records WHERE kind='plan' AND plan_id='fresh'")
            .len(),
        1
    );
    assert_eq!(
        f.rows(
            "SELECT p.id FROM plans p JOIN sessions s ON s.id=p.lead_session WHERE p.id='other'"
        )
        .len(),
        1
    );
}
