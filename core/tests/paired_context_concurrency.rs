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
    use cpd_core::host::runner::{Call, Handler};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, mpsc};
    use std::time::Duration;

    struct Gate {
        entered: mpsc::SyncSender<()>,
        release: Mutex<mpsc::Receiver<()>>,
        first: AtomicBool,
    }
    impl Handler for Gate {
        fn handle(&self, call: &Call) -> Reply {
            if call.args.iter().any(|a| a == "new-window")
                && call.args.iter().any(|a| a == "ctx-ctx")
                && self.first.swap(false, Ordering::SeqCst)
            {
                let _ = self.entered.try_send(());
                let _ = self
                    .release
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(5));
                Reply::ok("@1 %1\n")
            } else if call.args.iter().any(|a| a == "new-window") {
                Reply::ok("@2 %2\n")
            } else {
                Reply::ok("")
            }
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
            first: AtomicBool::new(true),
        }),
    );
    std::thread::scope(|scope| {
        let first_ports = f.fakes.ports.clone();
        let first = scope.spawn(move || run(&first_ports, "ctx"));
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("first provisioning reached tmux");
        let waiter_ports = f.fakes.ports.clone();
        let waiter = scope.spawn(move || run(&waiter_ports, "ctx"));
        let other = run(&f.fakes.ports, "other");
        release_tx.send(()).unwrap();
        let first_result = first.join().unwrap();
        let second_result = waiter.join().unwrap();
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
