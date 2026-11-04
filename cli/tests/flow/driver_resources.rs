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

const OTHER_RECORD: &str = r#"{"by":"attempt","plan":"other","job":"db","attempt":"db-a1","at":"2026-10-26T21:41:00.000Z"}"#;

/// A `Spawn` over the `FakeSpawn` whose `spawn` keeps a copy (a `dup`, the same open file description) of each descriptor of a
/// resource lock this process has open: what the driver inherits.
struct Driverish {
    inner: Arc<FakeSpawn>,
    locks: PathBuf,
    kept: Mutex<Vec<OwnedFd>>,
}

impl Driverish {
    fn holds(&self) -> usize {
        self.kept.lock().unwrap().len()
    }

    /// The (device, inode) of each resource lock file that exists.
    fn targets(&self) -> Vec<(u64, u64)> {
        let Ok(entries) = fs::read_dir(&self.locks) else {
            return vec![];
        };
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
        let numbers: Vec<i32> = fs::read_dir("/dev/fd")?
            .flatten()
            .filter_map(|e| e.file_name().to_string_lossy().parse().ok())
            .collect();
        for n in numbers {
            // SAFETY: only `dup`s the number, which fails when it is not open.
            let Ok(copy) = dup(unsafe { BorrowedFd::borrow_raw(n) }) else {
                continue;
            };
            let file = File::from(copy);
            if file
                .metadata()
                .is_ok_and(|m| targets.contains(&(m.dev(), m.ino())))
            {
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
    let spawn = Arc::new(Driverish {
        inner: fx.fakes.spawn.clone(),
        locks: fx.env.cpd_home().join("locks"),
        kept: Mutex::default(),
    });
    let ports = Ports {
        clock: fx.fakes.ports.clock.clone(),
        runner: fx.fakes.ports.runner.clone(),
        spawn: spawn.clone(),
        ..Ports::real()
    };
    World { fx, spawn, ports }
}

/// Job `key` of PR `repo` of Plan `pay`, queued with its Locked-Job fingerprint, listing `resources` in its Spec (null when none:
/// a Locked Job's Spec cannot change, so they are written while it is a draft).
fn add_job(fx: &Fx, key: &str, resources: &[&str]) {
    let resources = if resources.is_empty() {
        "NULL".to_owned()
    } else {
        format!("'{}'", json!(resources))
    };
    fixture(
        &fx.env,
        &format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at, resources)
             VALUES ('pay', 'repo', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 0, cpd_txn(), 't', {resources});"
        ),
    );
    let plan = PlanId::parse(PLAN).unwrap();
    let all = store(&fx.env).read(|conn| jobs::load(conn, &plan)).unwrap();
    let job = all.iter().find(|j| j.key == key).unwrap();
    fixture(
        &fx.env,
        &format!(
            "UPDATE jobs SET status = 'queued', fingerprint = '{}' WHERE plan = 'pay' AND key = '{key}';",
            locked_fingerprint(job)
        ),
    );
}

impl World {
    /// `job dispatch <key>` on the ports of the `Driverish`.
    fn dispatch(&self, key: &str) -> Run {
        let argv = ["cpd", "job", "dispatch", key, "--plan", PLAN].map(std::ffi::OsString::from);
        let mut stdout = Vec::new();
        let code = cpd::run_with(&self.ports, argv, &mut stdout);
        Run {
            exit: exit_code(code),
            stdout: String::from_utf8(stdout).unwrap(),
            stderr: String::new(),
        }
    }

    fn lock(&self, name: &str) -> PathBuf {
        self.fx
            .env
            .cpd_home()
            .join(format!("locks/resource.repo.acme.pay.{name}.1.lock"))
    }

    /// Holds `name` as another process would: its own open file description, with `record` in the file.
    fn hold(&self, name: &str, record: &str) -> File {
        let path = self.lock(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .unwrap();
        assert!(
            lock::try_lock(&file).unwrap(),
            "the test could not take {name}"
        );
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

    fn txns(&self) -> i64 {
        store(&self.fx.env)
            .read(|c| Ok(c.query_row("SELECT count(*) FROM txns", [], |r| r.get(0))?))
            .unwrap()
    }

    fn attempts(&self) -> i64 {
        self.fx.env.query("SELECT count(*) FROM attempts")
    }

    fn status(&self, key: &str) -> String {
        self.fx.env.query(&format!(
            "SELECT status FROM jobs WHERE plan = 'pay' AND key = '{key}'"
        ))
    }

    fn shown(&self, key: &str) -> Value {
        self.fx.cpd(&["job", "show", key, "--plan", PLAN]).ok()["job"].clone()
    }
}

const BOTH: &str = "[resources.e2e-port]\n[resources.db]\n";

#[test]
fn resource_hold_a_held_resource_refuses_the_start_with_resource_busy_and_takes_nothing() {
    let w = world(BOTH, &["e2e-port"]);
    let _other = w.hold("e2e-port", &common::command_record());

    let run = w.dispatch(JOB);

    let error = refused(&run, "resource_busy", 3);
    assert_eq!(error["details"]["job"], "api");
    assert_eq!(error["details"]["resource"], "e2e-port");
    // No Attempt row, no driver, and the holder's record is left as it was.
    assert_eq!(w.attempts(), 0);
    assert_eq!(w.status(JOB), "queued");
    assert_eq!(w.fx.fakes.spawn.launches_but_janitor(), []);
    assert_eq!(
        fs::read_to_string(w.lock("e2e-port")).unwrap(),
        common::command_record()
    );
}

#[test]
fn resource_hold_taking_is_all_or_none_across_two_resources() {
    let w = world(BOTH, &["db", "e2e-port"]);
    // `db` sorts first and is free: the starter takes it, finds `e2e-port` held, and must not keep `db`.
    let _other = w.hold("e2e-port", &common::command_record());

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
    let _held = w.hold("e2e-port", &common::command_record());
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
    assert!(
        w.shown(JOB)
            .as_object()
            .unwrap()
            .contains_key("waiting_for")
    );
    for key in ["plain", "running", "cancelled"] {
        let job = w.shown(key);
        assert!(
            !job.as_object().unwrap().contains_key("waiting_for"),
            "{key}: {job}"
        );
    }
}

#[test]
fn resource_run_and_the_starter_cannot_both_take_the_last_slot() {
    // One slot of `e2e-port`. The starter takes it for `api`: a tenant in the database (its `job_pools` row commits with the
    // Attempt), with no lock file. A `resource run` after it finds no free slot, and is refused with the Job as the holder.
    let w = world("[resources.e2e-port]\n", &["e2e-port"]);
    assert_eq!(w.dispatch(JOB).ok()["attempt"], "api-a1");
    let rows = w.fx.env.query::<i64>("SELECT count(*) FROM job_pools WHERE plan = 'pay' AND job = 'api' AND pool = 'repo:acme/pay:e2e-port'");
    assert_eq!(
        rows, 1,
        "the starter's row, in the transaction of the Attempt"
    );

    let repo = w.fx.keyed_repo("pay");
    let run =
        w.fx.cpd_in(&repo, &["resource", "run", "e2e-port", "--", "npm", "test"]);

    let error = refused(&run, "resource_busy", 3);
    let holders = error["details"]["holders"]
        .as_array()
        .unwrap_or_else(|| panic!("{error}"));
    assert_eq!(holders.len(), 1, "{error}");
    assert_eq!(
        (
            &holders[0]["slot"],
            &holders[0]["holder"]["by"],
            &holders[0]["holder"]["job"]
        ),
        (&Value::Null, &json!("attempt"), &json!("api"))
    );
    assert!(
        w.fx.fakes.spawn.execs().is_empty(),
        "the command never started"
    );
}

// ─── E3's `job-blocked` Event (contract §3.7 E3): the engine in-process on the fakes, on the `ManualClock` ───
//
// The engine/schedule harness, reused from [`common::engine`]'s `Fx` helpers: `engine_config`, `engine_routes`, `engine_plan`,
// `add_job`, `add_running`, `live_driver`, `watch_ticks`. The drivers E3 starts are launches of the `FakeSpawn`.

use cpd_core::host::runner::Reply;

/// The engine in-process for E3: Plan `pay` (PR `repo` of the repo `acme/pay`, whose policy file is `policy`), `concurrency`
/// Worker slots (`retry_cap = 0`). The drivers E3 starts are launches of the `FakeSpawn`, whose `self_exe` is a file that exists.
fn engine(concurrency: u32, policy: &str) -> Fx {
    let fx = Fx::with_ports();
    fx.engine_config(&format!("concurrency = {concurrency}\nretry_cap = 0\n"));
    fx.engine_routes();
    fx.env.sql("INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');");
    fx.engine_plan();
    fx.env
        .sql("UPDATE workflows SET name = 'pay' WHERE plan = 'pay';");
    fx.env.repo_policy("acme", "pay", policy);
    let exe = fx.env.root().join("cpd-installed");
    fs::write(&exe, "").unwrap();
    fx.fakes.spawn.set_self_exe(&exe);
    let poll = json!({"ok": true, "polled": 0, "rate_limit": {"remaining": 5000, "reset_at": "2030-01-01T00:00:00.000Z"}});
    fx.fake("cpd-installed", Reply::ok(&poll.to_string()));
    fx
}

/// The `job-blocked` Events of Plan `pay`: their `job` and `data`, in order.
fn blocked(fx: &Fx) -> Vec<(String, Value)> {
    fx.env.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT job, data FROM events WHERE kind = 'job-blocked' AND plan = 'pay' ORDER BY id",
        )?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows
            .into_iter()
            .map(|(job, data)| (job, serde_json::from_str(&data).unwrap()))
            .collect())
    })
}

/// The `skipped_n` of Job `key` (NULL: never skipped).
fn skipped_n(fx: &Fx, key: &str) -> Option<i64> {
    fx.env.query(&format!(
        "SELECT skipped_n FROM jobs WHERE plan = 'pay' AND key = '{key}'"
    ))
}

#[test]
fn job_blocked_is_appended_once_per_waiting_episode() {
    // One Worker slot, taken by a running Attempt: `api` is Ready but cannot start — no pool slot is free. The first tick of
    // the waiting episode appends one `job-blocked` Event; later ticks append no second one until the Job starts.
    let fx = engine(1, "");
    let busy = fx.live_driver("busy-a1");
    fx.add_running("busy", "running", &fx.stamp_ago(60_000));
    fx.add_job("api", &[], "");

    fx.watch_first_tick().unwrap();

    let first = blocked(&fx);
    assert_eq!(
        first,
        vec![("api".to_owned(), json!({"waiting": 1}))],
        "one Event on the first tick of the waiting episode: {first:?}"
    );

    // The waiting goes on: no second Event.
    fx.watch_ticks(3).unwrap();
    assert_eq!(
        blocked(&fx).len(),
        1,
        "the waiting went on: no second Event"
    );

    // `busy` merges, the slot frees: `api` starts, and no Event is appended for it.
    fx.env.sql("UPDATE attempts SET status = 'ended', outcome = 'merged', cause = 'merged', merged_sha = 'abc', ended_at = 't' WHERE id = 'busy-a1';
         UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE plan = 'pay' AND key = 'busy';");
    drop(busy);
    fx.watch_first_tick().unwrap();

    assert_eq!(
        fx.env
            .query::<String>("SELECT status FROM jobs WHERE plan = 'pay' AND key = 'api'"),
        "running"
    );
    assert_eq!(
        blocked(&fx).len(),
        1,
        "the Job started: the episode is over, no second Event"
    );
}

#[test]
fn no_job_blocked_while_a_dependency_waits() {
    // One Worker slot, taken by a running Attempt: `dep` is Ready but cannot start, `api` waits for it — a Job whose Dependency
    // waits is never Ready, so it produces no `job-blocked` Event and is not counted in `dep`'s `waiting`.
    let fx = engine(1, "");
    let _busy = fx.live_driver("busy-a1");
    fx.add_running("busy", "running", &fx.stamp_ago(60_000));
    fx.add_job("dep", &[], "");
    fx.add_job("api", &["dep"], "");

    fx.watch_first_tick().unwrap();

    assert_eq!(
        blocked(&fx),
        vec![("dep".to_owned(), json!({"waiting": 1}))],
        "only `dep`, Ready and blocked"
    );
    assert_eq!(
        skipped_n(&fx, "api"),
        None,
        "`api` is never Ready: no skip, no Event"
    );

    // The waiting goes on: `dep` still has its one Event, `api` never gets one.
    fx.watch_ticks(3).unwrap();
    assert_eq!(
        blocked(&fx),
        vec![("dep".to_owned(), json!({"waiting": 1}))]
    );
    assert_eq!(skipped_n(&fx, "api"), None);
}

#[test]
fn no_job_blocked_for_an_archived_plan() {
    // An archived Plan is refused while a Job is queued, so the fixture is the direct insert: `api` is queued in the archived
    // Plan `pay`. Its Jobs are never Ready, so it produces no `job-blocked` Event and is never skipped.
    let fx = engine(1, "");
    let busy = fx.live_driver("busy-a1");
    fx.add_running("busy", "running", &fx.stamp_ago(60_000));
    fx.add_job("api", &[], "");
    fx.env.sql("UPDATE plans SET archived_at = 't';");

    fx.watch_first_tick().unwrap();

    assert_eq!(
        blocked(&fx),
        vec![],
        "the archived Plan's Job produced no Event"
    );
    assert_eq!(
        skipped_n(&fx, "api"),
        None,
        "its Jobs are never Ready: no skip"
    );

    fx.watch_ticks(3).unwrap();
    assert_eq!(blocked(&fx), vec![]);
    assert_eq!(skipped_n(&fx, "api"), None);
    drop(busy);
}
