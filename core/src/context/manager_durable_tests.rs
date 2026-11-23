use std::{fs, sync::Arc};

use crate::{
    clock::ManualClock,
    error::{CpdError, ErrorCode},
    home::CpdHome,
    host::{host_socket::FakeHostSocket, runner::FakeRunner, spawn::FakeSpawn},
    output::Output,
    ports::Ports,
    store::Store,
};
use clap::Parser;

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: super::Command,
}

struct World {
    dir: tempfile::TempDir,
    home: CpdHome,
    store: Store,
    ports: Ports,
    runner: FakeRunner,
}

impl World {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
        fs::create_dir_all(home.config().parent().unwrap()).unwrap();
        fs::write(
            home.config(),
            "version = 1\nmachine = \"m1\"\nmanager_runner = \"pi\"\n[tmux]\nsocket = \"t\"\n",
        )
        .unwrap();
        let store = Store::open(&home, 5000).unwrap();
        let runner = FakeRunner::with_tmux();
        let ports = Ports {
            clock: Arc::new(ManualClock::new()),
            runner: Arc::new(runner.clone()),
            spawn: Arc::new(FakeSpawn::new()),
            host_socket: Arc::new(FakeHostSocket::new()),
            host_conn: Arc::new(crate::session::tail::FakeHostConn::new()),
        };
        Self {
            dir,
            home,
            store,
            ports,
            runner,
        }
    }
    fn command(&self, words: &[&str]) -> Result<Output, CpdError> {
        let command = Cli::try_parse_from(["context"].into_iter().chain(words.iter().copied()))
            .unwrap()
            .command;
        crate::env::with_fixture_environment(&self.home, self.dir.path(), || {
            Ports::scope(&self.ports, || super::run(command))
        })
    }
    fn sql(&mut self, sql: &str) {
        self.store
            .write("fixture", |tx| {
                tx.execute_batch(sql)?;
                Ok(())
            })
            .unwrap();
    }
    fn count(&mut self, sql: &str) -> i64 {
        self.store
            .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }
    fn owner(&self) {
        let path = self.home.root().join("policy/acme/owner.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }
}

#[test]
fn explicit_pi_refused_before_effects() {
    let mut w = World::new();
    let before = w.count("SELECT count(*) FROM txns");
    let err = w
        .command(&["manager", "--context", "host", "--runner", "pi"])
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::Usage);
    assert_eq!(err.exit().code(), 2);
    assert_eq!(w.count("SELECT count(*) FROM contexts"), 0);
    assert_eq!(w.count("SELECT count(*) FROM sessions"), 0);
    assert_eq!(w.count("SELECT count(*) FROM events"), 0);
    assert_eq!(w.count("SELECT count(*) FROM txns"), before);
    assert!(
        !w.home
            .manager_message(&crate::ids::ContextId::parse("host").unwrap())
            .exists()
    );
    assert!(w.runner.calls().is_empty());
}

#[test]
fn fresh_manager_is_durable_across_creation_routes() {
    for override_args in [&[][..], &["--runner", "durable"][..]] {
        let mut w = World::new();
        w.sql("INSERT INTO contexts (id,title,created_at,created_on) VALUES ('ctx','Ctx','t','m')");
        let mut args = vec!["manager", "--context", "ctx"];
        args.extend_from_slice(override_args);
        w.command(&args).unwrap();
        assert_eq!(w.count("SELECT count(*) FROM sessions WHERE context='ctx' AND role='manager' AND runner='durable' AND tmux_pane IS NULL"), 1);
        assert_eq!(
            w.count(
                "SELECT count(*) FROM events WHERE context='ctx' AND kind='session-registered'"
            ),
            1
        );
        assert!(w.runner.calls().is_empty());
    }
    let mut w = World::new();
    w.owner();
    w.command(&["new", "newctx", "--title", "New", "--owner", "acme"])
        .unwrap();
    assert_eq!(w.count("SELECT count(*) FROM sessions WHERE context='newctx' AND role='manager' AND runner='durable'"), 1);
    let mut p = World::new();
    p.command(&["manager", "--context", "host"]).unwrap();
    assert_eq!(p.count("SELECT count(*) FROM sessions WHERE context='host' AND role='manager' AND runner='durable'"), 1);
}

#[test]
fn existing_manager_and_atomic_pairing_are_preserved() {
    let mut w = World::new();
    w.owner();
    let err = w
        .command(&["new", "denied", "--title", "Denied", "--owner", "unknown"])
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::OwnerUnconfirmed);
    assert_eq!(w.count("SELECT count(*) FROM contexts"), 0);
    assert_eq!(w.count("SELECT count(*) FROM sessions"), 0);
    assert_eq!(w.count("SELECT count(*) FROM events"), 0);
    w.sql("CREATE TRIGGER reject_manager BEFORE INSERT ON events WHEN NEW.kind = 'session-registered' BEGIN SELECT RAISE(ABORT, 'reject'); END;");
    assert!(
        w.command(&["new", "rollback", "--title", "Rollback", "--owner", "acme"])
            .is_err()
    );
    assert_eq!(
        w.count("SELECT count(*) FROM contexts WHERE id='rollback'"),
        0
    );
    assert_eq!(
        w.count("SELECT count(*) FROM sessions WHERE context='rollback'"),
        0
    );
    assert_eq!(
        w.count("SELECT count(*) FROM events WHERE context='rollback'"),
        0
    );
    w.sql("DROP TRIGGER reject_manager");
    w.command(&["new", "paired", "--title", "Paired", "--owner", "acme"])
        .unwrap();
    assert_eq!(w.count("SELECT count(*) FROM events a JOIN events b ON a.txn=b.txn AND a.context=b.context JOIN sessions s ON s.id=b.session WHERE a.context='paired' AND a.kind='context-created' AND b.kind='session-registered' AND s.runner='durable'"), 1);
    w.sql("INSERT INTO contexts (id,title,created_at,created_on) VALUES ('old','Old','t','m'); INSERT INTO sessions (id,role,context,name,cwd,tier,state,state_at,created_at,runner,tmux_pane) VALUES ('oldmgr','manager','old','old','/','O4','idle','t','t','pi','%1');");
    w.command(&["manager", "--context", "old"]).unwrap();
    assert_eq!(
        w.count("SELECT count(*) FROM sessions WHERE id='oldmgr' AND runner='pi'"),
        1
    );
}
