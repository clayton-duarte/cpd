//! The starter and the resources of a Job (contract §3.4 "Resources of a Job", §1.4; commands.md §5.5, §5.7): in-process on the
//! fakes. The test holds a resource with its own open file description of the lock file, as another process would. The driver is
//! the test's [`Driverish`] `Spawn`: at the spawn it keeps a copy of every descriptor of a resource lock the starter has open, as
//! the child of a real spawn inherits them, so "the driver holds them until it exits" is a copy the test drops.

use crate::common;

use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::fd::{BorrowedFd, OwnedFd};
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;
use std::process::ExitStatus;
use std::sync::{Arc, Mutex};

use common::fx::{Fx, exit_code};
use common::{Run, refused};
use cpd_core::error::CpdError;
use cpd_core::host::spawn::{FakeSpawn, Launch, Pid, Signal, Spawn};
use cpd_core::ids::PlanId;
use cpd_core::lock;
use cpd_core::plan::{jobs, locked_fingerprint};
use cpd_core::ports::Ports;
use nix::unistd::dup;
use serde_json::{Value, json};

use super::driver::{JOB, PLAN, fixture, seed_home, seed_rows, store};

const COMMAND_RECORD: &str =
    r#"{"by":"command","pid":4242,"command":"npm run dev","session":"s9","attempt":null,"at":"2026-10-26T21:40:00.000Z"}"#;
const OTHER_RECORD: &str = r#"{"by":"attempt","plan":"other","job":"db","attempt":"db-a1","at":"2026-10-26T21:41:00.000Z"}"#;

/// A `Spawn` over the `FakeSpawn` whose `spawn` keeps a copy (a `dup`, the same open file description) of each descriptor of a
/// resource lock this process has open: what the driver inherits.
struct Driverish {
    inner: Arc<FakeSpawn>,
    locks: PathBuf,
    kept: Mutex<Vec<OwnedFd>>,
}

impl Driverish {
    /// The driver exits: its descriptors close.
    fn exits(&self) {
        self.kept.lock().unwrap().clear();
    }

    fn holds(&self) -> usize {
        self.kept.lock().unwrap().len()
    }

    /// The (device, inode) of each resource lock file that exists.
    fn targets(&self) -> Vec<(u64, u64)> {
        let Ok(entries) = fs::read_dir(&self.locks) else { return vec![] };
        entries
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("resource."))
            .filter_map(|e| e.metadata().ok())
            .map(|m| (m.dev(), m.ino()))
            .collect()
    }
}

impl Spawn for Driverish {
    fn spawn(&self, l: &Launch) -> io::Result<Pid> {
        let targets = self.targets();
        let numbers: Vec<i32> = fs::read_dir("/dev/fd")?.flatten().filter_map(|e| e.file_name().to_string_lossy().parse().ok()).collect();
        for n in numbers {
            // SAFETY: only `dup`s the number, which fails when it is not open.
            let Ok(copy) = dup(unsafe { BorrowedFd::borrow_raw(n) }) else { continue };
            let file = File::from(copy);
            if file.metadata().is_ok_and(|m| targets.contains(&(m.dev(), m.ino()))) {
                self.kept.lock().unwrap().push(OwnedFd::from(file));
            }
        }
        self.inner.spawn(l)
    }
    fn try_wait(&self, p: Pid) -> io::Result<Option<ExitStatus>> {
        self.inner.try_wait(p)
    }
    fn signal(&self, p: Pid, s: Signal) -> io::Result<()> {
        self.inner.signal(p, s)
    }
    fn alive(&self, p: Pid) -> bool {
        self.inner.alive(p)
    }
    fn exe_of(&self, p: Pid) -> Option<PathBuf> {
        self.inner.exe_of(p)
    }
    fn cmdline(&self, p: Pid) -> Option<String> {
        self.inner.cmdline(p)
    }
    fn with_env(&self, key: &str, value: &str) -> Vec<Pid> {
        self.inner.with_env(key, value)
    }
    fn ppid(&self, p: Pid) -> Option<Pid> {
        self.inner.ppid(p)
    }
    fn env_of(&self, p: Pid, key: &str) -> Option<String> {
        self.inner.env_of(p, key)
    }
    fn descendants(&self, root: Pid) -> Vec<Pid> {
        self.inner.descendants(root)
    }
    fn exec(&self, l: &Launch) -> io::Error {
        self.inner.exec(l)
    }
    fn stream(&self, l: &Launch) -> io::Result<(Pid, Box<dyn io::BufRead + Send>)> {
        self.inner.stream(l)
    }
    fn self_exe(&self) -> Result<PathBuf, CpdError> {
        self.inner.self_exe()
    }
}

struct World {
    fx: Fx,
    spawn: Arc<Driverish>,
    ports: Ports,
}

/// Plan `pay`, PR `repo` of repo `acme/pay` whose policy file is `policy`, and Job `api` queued, listing `resources`.
fn world(policy: &str, resources: &[&str]) -> World {
    let fx = Fx::with_ports();
    let repo = fx.env.root().join("repos/pay");
    seed_home(&fx.env, "");
    fx.env.repo_policy("acme", "pay", policy);
    seed_rows(&fx.env, &repo, &fx.env.root().join("repos/pay.pay"));
    add_job(&fx, JOB, resources);
    let exe = fx.env.root().join("cpd-installed");
    fs::write(&exe, "").unwrap();
    fx.fakes.spawn.set_self_exe(&exe);
    let spawn = Arc::new(Driverish { inner: fx.fakes.spawn.clone(), locks: fx.env.cpd_home().join("locks"), kept: Mutex::default() });
    let ports = Ports { clock: fx.fakes.ports.clock.clone(), runner: fx.fakes.ports.runner.clone(), spawn: spawn.clone() };
    World { fx, spawn, ports }
}

/// Job `key` of PR `repo` of Plan `pay`, queued with its Locked-Job fingerprint, listing `resources` in its Spec (null when none:
/// a Locked Job's Spec cannot change, so they are written while it is a draft).
fn add_job(fx: &Fx, key: &str, resources: &[&str]) {
    let resources = if resources.is_empty() { "NULL".to_owned() } else { format!("'{}'", json!(resources)) };
    fixture(
        &fx.env,
        &format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at, resources)
             VALUES ('pay', 'repo', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 0, cpd_txn(), 't', {resources});"
        ),
    );
    let plan = PlanId::parse(PLAN).unwrap();
    let all = store(&fx.env).read(|conn| jobs::load(conn, &plan)).unwrap();
    let job = all.iter().find(|j| j.key == key).unwrap();
    fixture(&fx.env, &format!("UPDATE jobs SET status = 'queued', fingerprint = '{}' WHERE plan = 'pay' AND key = '{key}';", locked_fingerprint(job)));
}

impl World {
    /// `job dispatch <key>` on the ports of the `Driverish`.
    fn dispatch(&self, key: &str) -> Run {
        let argv = ["cpd", "job", "dispatch", key, "--plan", PLAN].map(std::ffi::OsString::from);
        let mut stdout = Vec::new();
        let code = cpd::run_with(&self.ports, argv, &mut stdout);
        Run { exit: exit_code(code), stdout: String::from_utf8(stdout).unwrap(), stderr: String::new() }
    }

    fn lock(&self, name: &str) -> PathBuf {
        self.fx.env.cpd_home().join(format!("locks/resource.repo.acme.pay.{name}.1.lock"))
    }

    /// Holds `name` as another process would: its own open file description, with `record` in the file.
    fn hold(&self, name: &str, record: &str) -> File {
        let path = self.lock(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(&path).unwrap();
        assert!(lock::try_lock(&file).unwrap(), "the test could not take {name}");
        fs::write(&path, record).unwrap();
        file
    }

    /// Whether some open file description holds the resource (a shared try-lock is refused while one does).
    fn held(&self, name: &str) -> bool {
        match File::open(self.lock(name)) {
            Ok(file) => !lock::shared(&file).unwrap(),
            Err(_) => false,
        }
    }

    fn record(&self, name: &str) -> Value {
        serde_json::from_str(&fs::read_to_string(self.lock(name)).unwrap()).unwrap()
    }

    fn txns(&self) -> i64 {
        store(&self.fx.env).read(|c| Ok(c.query_row("SELECT count(*) FROM txns", [], |r| r.get(0))?)).unwrap()
    }

    fn attempts(&self) -> i64 {
        self.fx.env.query("SELECT count(*) FROM attempts")
    }

    fn status(&self, key: &str) -> String {
        self.fx.env.query(&format!("SELECT status FROM jobs WHERE plan = 'pay' AND key = '{key}'"))
    }

    fn shown(&self, key: &str) -> Value {
        self.fx.cpd(&["job", "show", key, "--plan", PLAN]).ok()["job"].clone()
    }
}

const BOTH: &str = "[resources.e2e-port]\n[resources.db]\n";

#[test]
fn resource_hold_the_starter_takes_every_resource_of_the_job_and_writes_the_attempt_record() {
    let w = world(BOTH, &["db", "e2e-port"]);

    let started = w.dispatch(JOB).ok();

    assert_eq!(started["attempt"], "api-a1");
    // The starter handed both to the driver: they are held, each with the Attempt's holder record.
    assert_eq!(w.spawn.holds(), 2);
    for name in ["db", "e2e-port"] {
        assert!(w.held(name), "{name} is not held");
        let record = w.record(name);
        assert_eq!(
            (record["by"].as_str(), record["plan"].as_str(), record["job"].as_str(), record["attempt"].as_str()),
            (Some("attempt"), Some("pay"), Some("api"), Some("api-a1")),
            "{record}"
        );
        let at = record["at"].as_str().unwrap_or_default();
        assert!(time::OffsetDateTime::parse(at, &time::format_description::well_known::Rfc3339).is_ok(), "at {at:?}");
        assert_eq!(record.as_object().unwrap().len(), 5, "{record}");
    }
    assert_eq!(w.attempts(), 1);
}

#[test]
fn resource_hold_a_held_resource_refuses_the_start_with_resource_busy_and_takes_nothing() {
    let w = world(BOTH, &["e2e-port"]);
    let _other = w.hold("e2e-port", COMMAND_RECORD);

    let run = w.dispatch(JOB);

    let error = refused(&run, "resource_busy", 3);
    assert_eq!(error["details"]["job"], "api");
    assert_eq!(error["details"]["resource"], "e2e-port");
    // No Attempt row, no driver, and the holder's record is left as it was.
    assert_eq!(w.attempts(), 0);
    assert_eq!(w.status(JOB), "queued");
    assert_eq!(w.fx.fakes.spawn.launches_but_janitor(), []);
    assert_eq!(fs::read_to_string(w.lock("e2e-port")).unwrap(), COMMAND_RECORD);
}

#[test]
fn resource_hold_taking_is_all_or_none_across_two_resources() {
    let w = world(BOTH, &["db", "e2e-port"]);
    // `db` sorts first and is free: the starter takes it, finds `e2e-port` held, and must not keep `db`.
    let _other = w.hold("e2e-port", COMMAND_RECORD);

    let error = refused(&w.dispatch(JOB), "resource_busy", 3);

    assert_eq!(error["details"]["resource"], "e2e-port");
    assert!(!w.held("db"), "db was left taken");
    assert_eq!(w.spawn.holds(), 0);
    assert_eq!(w.attempts(), 0);

    // The other way round: `db` held, `e2e-port` free; the one held is named and the free one is not taken.
    drop(_other);
    let _db = w.hold("db", OTHER_RECORD);
    let error = refused(&w.dispatch(JOB), "resource_busy", 3);
    assert_eq!(error["details"]["resource"], "db");
    assert!(!w.held("e2e-port"), "e2e-port was left taken");
    assert_eq!(w.attempts(), 0);
}

#[test]
fn resource_hold_the_driver_holds_the_resources_until_it_exits() {
    let w = world(BOTH, &["e2e-port"]);
    w.dispatch(JOB).ok();
    // The starter has returned and closed its own copies: the driver's descriptors hold the lock, with the record intact.
    assert!(w.held("e2e-port"));
    assert_eq!(w.record("e2e-port")["attempt"], "api-a1");
    // A second taker is refused while the driver lives.
    let refused_while = File::open(w.lock("e2e-port")).unwrap();
    assert!(!lock::shared(&refused_while).unwrap());
    drop(refused_while);

    // The driver exits (the Attempt ended, or it died): the kernel releases the lock, with no call that releases it by name.
    w.spawn.exits();

    assert!(!w.held("e2e-port"));
    let file = OpenOptions::new().read(true).write(true).open(w.lock("e2e-port")).unwrap();
    assert!(lock::try_lock(&file).unwrap(), "the resource is taken again once the driver is gone");
}

#[test]
fn resource_hold_job_dispatch_refuses_resource_busy_and_changes_nothing() {
    let w = world(BOTH, &["db", "e2e-port"]);
    let _other = w.hold("db", OTHER_RECORD);
    let before = w.txns();
    let events: i64 = w.fx.env.query("SELECT count(*) FROM events");

    refused(&w.dispatch(JOB), "resource_busy", 3);

    assert_eq!(w.txns(), before);
    assert_eq!(w.fx.env.query::<i64>("SELECT count(*) FROM events"), events);
    assert_eq!(w.status(JOB), "queued");
    assert_eq!(w.attempts(), 0);
    assert_eq!(w.fx.fakes.spawn.launches_but_janitor(), []);
    // Free once the holder lets go: the same command now starts the Job.
    drop(_other);
    assert_eq!(w.dispatch(JOB).ok()["attempt"], "api-a1");
}

#[test]
fn resource_hold_job_show_has_no_waiting_for_on_other_jobs() {
    let w = world(BOTH, &["db", "e2e-port"]);
    let _held = w.hold("e2e-port", COMMAND_RECORD);
    // A queued Job that lists none, a running one and a cancelled one that list some.
    add_job(&w.fx, "plain", &[]);
    add_job(&w.fx, "running", &["e2e-port"]);
    add_job(&w.fx, "cancelled", &["e2e-port"]);
    fixture(
        &w.fx.env,
        "UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = 'running';
         UPDATE jobs SET status = 'cancelled', cancel_reason = 'no longer needed', cancel_requested_at = 't' WHERE plan = 'pay' AND key = 'cancelled';",
    );

    // Control: the queued Job that lists resources has the key.
    assert!(w.shown(JOB).as_object().unwrap().contains_key("waiting_for"));
    for key in ["plain", "running", "cancelled"] {
        let job = w.shown(key);
        assert!(!job.as_object().unwrap().contains_key("waiting_for"), "{key}: {job}");
    }
}
