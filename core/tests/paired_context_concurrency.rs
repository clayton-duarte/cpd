//! Concurrent Context creation with fake host and clock.

use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard};

use clap::Parser;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::host::runner::Reply;
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
        // This binary serializes environment access and starts no threads.
        unsafe {
            std::env::set_var("CPD_HOME", home.root());
            std::env::set_var("HOME", dir.path());
            std::env::remove_var("CPD_SESSION");
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
    use cpd_core::clock::Clock;
    use cpd_core::host::runner::{Call, Handler};
    use std::sync::{Arc, mpsc};
    use std::time::{Duration, Instant};

    struct Gate {
        entered: mpsc::SyncSender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl Handler for Gate {
        fn handle(&self, call: &Call) -> Reply {
            if call.args.iter().any(|a| a == "new-window")
                && call.args.iter().any(|a| a == "ctx-ctx")
            {
                self.entered.send(()).unwrap();
                self.release.lock().unwrap().recv().unwrap();
                Reply::ok("@1 %1\n")
            } else if call.args.iter().any(|a| a == "new-window") {
                Reply::ok("@2 %2\n")
            } else {
                Reply::ok("")
            }
        }
    }
    struct WaitClock {
        inner: Arc<cpd_core::clock::ManualClock>,
        entered: mpsc::SyncSender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl Clock for WaitClock {
        fn now(&self) -> Instant {
            self.inner.now()
        }
        fn utc(&self) -> time::OffsetDateTime {
            self.inner.utc()
        }
        fn sleep(&self, d: Duration) {
            self.entered.send(()).unwrap();
            self.release.lock().unwrap().recv().unwrap();
            self.inner.advance(d);
        }
    }
    fn run(ports: &Ports, id: &str) -> Result<Output, CpdError> {
        let command =
            Cli::parse_from(["cpd", "new", id, "--title", "Context", "--owner", "acme"]).command;
        Ports::scope(ports, || cpd_core::context::run(command))
    }
    let f = Fixture::new("pi");
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    (*f.fakes.runner).clone().route(
        "tmux",
        Arc::new(Gate {
            entered: entered_tx,
            release: Mutex::new(release_rx),
        }),
    );
    let (waiting_tx, waiting_rx) = mpsc::sync_channel(1);
    let (resume_tx, resume_rx) = mpsc::sync_channel(1);
    let mut waiter_ports = f.fakes.ports.clone();
    waiter_ports.clock = Arc::new(WaitClock {
        inner: f.fakes.clock.clone(),
        entered: waiting_tx,
        release: Mutex::new(resume_rx),
    });
    std::thread::scope(|scope| {
        let first_ports = f.fakes.ports.clone();
        let first = scope.spawn(move || run(&first_ports, "ctx"));
        entered_rx.recv().unwrap();
        let waiter = scope.spawn(move || run(&waiter_ports, "ctx"));
        let waiting = waiting_rx.recv_timeout(Duration::from_secs(2));
        let other = run(&f.fakes.ports, "other");
        release_tx.send(()).unwrap();
        let first_result = first.join().unwrap();
        let _ = resume_tx.send(());
        let second_result = waiter.join().unwrap();
        assert!(
            waiting.is_ok(),
            "same-ID creation did not wait for its in-flight peer"
        );
        first_result.unwrap();
        second_result.unwrap();
        other.unwrap();
    });
    assert_eq!(f.rows("SELECT id FROM contexts WHERE id='ctx'").len(), 1);
    assert_eq!(
        f.rows("SELECT id FROM sessions WHERE context='ctx' AND role='manager'")
            .len(),
        1
    );
    assert_eq!(
        f.rows("SELECT id FROM events WHERE context='ctx' AND kind='session-moved'")
            .len(),
        1
    );
}

#[test]
fn context_creation_timeout_is_retryable() {
    use std::fs::OpenOptions;
    let f = Fixture::new("durable");
    let path = f.home.locks_dir().join("context-new-ctx.lock");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
        .unwrap();
    assert!(cpd_core::lock::try_lock(&lock).unwrap());
    let command =
        || Cli::parse_from(["cpd", "new", "ctx", "--title", "Context", "--owner", "acme"]).command;
    let error = Ports::scope(&f.fakes.ports, || cpd_core::context::run(command()))
        .expect_err("lock timeout must be reported");
    assert_eq!(error.code(), cpd_core::error::ErrorCode::LockTimeout);
    assert!(error.message().to_lowercase().contains("retry"));
    assert!(f.rows("SELECT id FROM contexts WHERE id='ctx'").is_empty());
    drop(lock);
    Ports::scope(&f.fakes.ports, || cpd_core::context::run(command())).unwrap();
    assert_eq!(f.rows("SELECT id FROM contexts WHERE id='ctx'").len(), 1);
}
