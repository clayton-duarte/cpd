//! Shaping pair provisioning recovers without real processes or external services.

use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard};

use clap::Parser;
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::host::runner::Reply;
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
fn shaping_tmux_failure_retries_same_pair() {
    let f = Fixture::new("pi");
    (*f.fakes.runner)
        .clone()
        .on(
            "tmux",
            |c| c.args.iter().any(|a| a == "has-session"),
            Reply::ok(""),
        )
        .once(
            "tmux",
            |c| c.args.iter().any(|a| a == "new-window"),
            Reply::exit(1, "", "injected failure"),
        )
        .on(
            "tmux",
            |c| c.args.iter().any(|a| a == "new-window"),
            Reply::ok("@1 %1\n"),
        )
        .on("tmux", |_| true, Reply::ok(""));
    let error = f.plan(&[]).expect_err("tmux failure must be reported");
    assert!(error.message().to_lowercase().contains("retry"));
    let pair = f.pair();
    assert_eq!(pair.len(), 1);
    let registered = f.rows("SELECT * FROM events WHERE plan='fresh' AND kind IN ('plan-created','session-registered') ORDER BY id");
    let Output::Success(out) = f.plan(&[]).unwrap() else {
        panic!("expected success")
    };
    assert_eq!(out["already"], true);
    assert_eq!(f.pair(), pair);
    assert_eq!(f.rows("SELECT * FROM events WHERE plan='fresh' AND kind IN ('plan-created','session-registered') ORDER BY id"), registered);
    assert_eq!(
        f.rows("SELECT id FROM events WHERE plan='fresh' AND kind='session-moved'")
            .len(),
        1
    );
    assert_eq!(
        f.fakes
            .runner
            .calls()
            .iter()
            .filter(|c| c.args.iter().any(|a| a == "new-window"))
            .count(),
        2
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
    use cpd_core::clock::Clock;
    use cpd_core::host::runner::{Call, Handler};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, mpsc};
    use std::time::{Duration, Instant};

    struct Gate {
        entered: mpsc::SyncSender<()>,
        release: Mutex<mpsc::Receiver<()>>,
        first: AtomicBool,
    }
    impl Handler for Gate {
        fn handle(&self, call: &Call) -> Reply {
            if call.args.iter().any(|a| a == "new-window")
                && call.args.iter().any(|a| a == "park-fresh")
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
            let _ = self.entered.try_send(());
            let _ = self
                .release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(5));
            self.inner.advance(d);
        }
    }
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
        let first = scope.spawn(move || run(&first_ports, "fresh"));
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("first provisioning reached tmux");
        let waiter = scope.spawn(move || run(&waiter_ports, "fresh"));
        // A correct lock sends this before either same-ID caller can create a second window.
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
    assert_eq!(f.pair().len(), 1);
    assert_eq!(
        f.rows("SELECT id FROM events WHERE plan='fresh' AND kind='plan-created'")
            .len(),
        1
    );
    assert_eq!(
        f.rows("SELECT id FROM events WHERE plan='fresh' AND kind='session-moved'")
            .len(),
        1
    );
    assert_eq!(
        f.rows("SELECT id FROM context_records WHERE kind='plan' AND plan_id='fresh'")
            .len(),
        1
    );
}
