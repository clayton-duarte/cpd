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
