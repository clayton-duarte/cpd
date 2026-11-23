//! Concurrent Context creation with fake host and clock.

use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard};

use clap::Parser;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::output::Output;
use cpd_core::ports::{Fakes, Ports};
use cpd_core::store::Store;
use rusqlite::types::Value;

static ENV_LOCK: Mutex<()> = Mutex::new(());

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: cpd_core::context::Command,
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
            format!("version = 1\nmachine = \"m1\"\nmanager_runner = \"{runner}\"\n"),
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
        // Environment changes are serialized; spawned threads are joined before restoration.
        unsafe {
            std::env::set_var("CPD_HOME", home.root());
            std::env::set_var("HOME", dir.path());
            std::env::remove_var("CPD_SESSION");
            for key in ["PI_SESSION_ID", "CPD_PLAN", "CPD_ATTEMPT"] {
                std::env::remove_var(key);
            }
        }
        Self {
            home,
            fakes: Fakes::new(),
            saved,
            _dir: dir,
            _lock: lock,
        }
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
fn concurrent_context_provisioning_serializes_same_id() {
    use std::sync::{Arc, Barrier};

    fn run(ports: &Ports, id: &str) -> Result<Output, CpdError> {
        let command =
            Cli::parse_from(["cpd", "new", id, "--title", "Context", "--owner", "acme"]).command;
        Ports::scope(ports, || cpd_core::context::run(command))
    }
    let f = Fixture::new("durable");
    // Initialize the store before either caller starts. Both callers cross the
    // authored command boundary together, rather than waiting for tmux (which
    // durable Managers never invoke).
    drop(f.store());
    let barrier = Arc::new(Barrier::new(3));
    std::thread::scope(|scope| {
        let mut callers = Vec::new();
        for _ in 0..2 {
            let ports = f.fakes.ports.clone();
            let barrier = barrier.clone();
            callers.push(scope.spawn(move || {
                barrier.wait();
                run(&ports, "ctx")
            }));
        }
        barrier.wait();
        for caller in callers {
            caller.join().unwrap().unwrap();
        }
        run(&f.fakes.ports, "other").unwrap();
    });
    assert_eq!(f.rows("SELECT id FROM contexts WHERE id='ctx'").len(), 1);
    assert_eq!(
        f.rows(
            "SELECT id FROM sessions WHERE context='ctx' AND role='manager' AND runner='durable'"
        )
        .len(),
        1
    );
    assert_eq!(
        f.rows("SELECT id FROM events WHERE context='ctx' AND kind='session-registered'")
            .len(),
        1
    );
    assert_eq!(
        f.rows("SELECT a.id FROM events a JOIN events b ON a.txn=b.txn AND a.context=b.context JOIN sessions s ON s.id=b.session WHERE a.context='ctx' AND a.kind='context-created' AND b.kind='session-registered' AND s.role='manager'").len(),
        1
    );
}
