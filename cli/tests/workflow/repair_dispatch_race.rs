//! While a repair (`pr rebase`, `pr push`) holds a Workflow, update Job dispatch of that Workflow is excluded by the repair
//! marker (contract §7.6 "The repair marker", pro-d465; §3.7 E3). Driven through public behaviour only, in-process on the
//! fixture of the rebase tests: the Gate's `sh` is a mock that, when the Gate runs (the repair is then in progress), runs
//! `cpd job dispatch` of a queued `update` Job and reads the store. No sleeps, no thread, no real external program.

use std::fs;
use std::sync::{Arc, Mutex};

use crate::workflow::{Poll, Rows, code, rebase_fx};

use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::ids::PlanId;
use cpd_core::plan::{jobs, locked_fingerprint};
use cpd_core::ports::Ports;
use cpd_core::store::Store;
use serde_json::Value;

const UPDATE: &str = "up-repo";

/// What the mock Gate saw while the repair was in progress.
#[derive(Clone, Debug)]
struct Seen {
    /// `workflows.repair` as the store held it at that moment.
    marker: Result<Option<Value>, String>,
    /// The stdout of `cpd job dispatch up-repo` run at that moment.
    dispatch: String,
    /// `jobs.status` of the update Job and the count of its Attempts afterwards.
    job: (String, i64),
}

/// The Gate's `sh`: it runs `during` once, and answers `reply`.
struct Gate {
    during: Box<dyn Fn() + Send + Sync>,
    reply: Reply,
}

impl Handler for Gate {
    fn handle(&self, _call: &Call) -> Reply {
        (self.during)();
        self.reply.clone()
    }
}

/// `workflows.repair`: the error text when the store has no such column yet, so a test of dispatch alone is not masked by it.
fn marker_of(store: &mut Store) -> Result<Option<Value>, String> {
    store
        .read(|c| {
            Ok(
                c.query_row("SELECT repair FROM workflows WHERE plan = 'pay'", [], |r| {
                    r.get::<_, Option<String>>(0)
                })?,
            )
        })
        .map(|text| text.map(|t| serde_json::from_str(&t).unwrap()))
        .map_err(|e| e.message().to_owned())
}

fn job_state(store: &mut Store) -> (String, i64) {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT j.status, (SELECT count(*) FROM attempts a WHERE a.plan = j.plan AND a.job = j.key)
                   FROM jobs j WHERE j.plan = 'pay' AND j.key = ?1",
                [UPDATE],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap()
}

impl Poll {
    /// A queued `update` Job of PR `repo`, with its Locked-Job fingerprint.
    fn queue_update(&self) {
        self.store()
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, kind, created_txn, created_at)
                     VALUES ('pay', 'repo', ?1, 'Update', 'S2', 'Merge the base.', 'Because.', '[\"**\"]', '[\"it works\"]', '[]',
                             'draft', 1, 'update', ?2, 't')",
                    rusqlite::params![UPDATE, tx.id()],
                )?;
                Ok(())
            })
            .unwrap();
        let plan = PlanId::parse("pay").unwrap();
        let all = self.store().read(|c| jobs::load(c, &plan)).unwrap();
        let job = all.iter().find(|j| j.key == UPDATE).unwrap();
        let fingerprint = locked_fingerprint(job);
        self.store()
            .write("fixture", |tx| {
                tx.execute(
                    "UPDATE jobs SET status = 'queued', fingerprint = ?1 WHERE plan = 'pay' AND key = ?2",
                    rusqlite::params![fingerprint, UPDATE],
                )?;
                Ok(())
            })
            .unwrap();
        let exe = self.env.root().join("cpd-installed");
        fs::write(&exe, "").unwrap();
        self.fakes.spawn.set_self_exe(&exe);
    }

    fn store_at(&self) -> Store {
        let home = CpdHome::resolve(Some(self.env.cpd_home().into()), None).unwrap();
        Store::open(&home, 5000).unwrap()
    }

    /// `cpd job dispatch up-repo --plan pay` now.
    fn dispatch(&self) -> String {
        dispatch_on(&self.fakes.ports)
    }

    /// The Gate (`pre-merge`) answers `reply` and, once, tries to dispatch the update Job and reads the store: what a repair in
    /// progress looks like from outside. Returns what it saw.
    fn watch_gate(&self, reply: Reply) -> Arc<Mutex<Option<Seen>>> {
        self.hooks("pre-merge = \"gate\"\n");
        let seen = Arc::new(Mutex::new(None));
        let ports = self.fakes.ports.clone();
        let root = self.env.cpd_home().to_owned();
        let into = seen.clone();
        let during = move || {
            let home = CpdHome::resolve(Some(root.clone().into()), None).unwrap();
            let mut store = Store::open(&home, 5000).unwrap();
            let marker = marker_of(&mut store);
            let dispatch = dispatch_on(&ports);
            *into.lock().unwrap() = Some(Seen {
                marker,
                dispatch,
                job: job_state(&mut store),
            });
        };
        let _ = (*self.fakes.runner).clone().route(
            "sh",
            Arc::new(Gate {
                during: Box::new(during),
                reply,
            }),
        );
        seen
    }
}

fn dispatch_on(ports: &Ports) -> String {
    let argv = ["cpd", "job", "dispatch", UPDATE, "--plan", "pay"].map(std::ffi::OsString::from);
    let mut stdout = Vec::new();
    cpd::run_with(ports, argv, &mut stdout);
    String::from_utf8(stdout).unwrap()
}

/// A Workflow whose base moved (so `pr rebase` has work and runs the Gate) with a queued update Job.
fn fx() -> Poll {
    let fx = rebase_fx("deploy");
    fx.advance_base("base.txt", "base\n");
    fx.queue_update();
    fx
}

fn seen(cell: &Arc<Mutex<Option<Seen>>>) -> Seen {
    cell.lock().unwrap().clone().expect("the Gate ran")
}

#[test]
fn update_dispatch_skipped_during_repair() {
    let fx = fx();
    let gate = fx.watch_gate(Reply::ok(""));

    let run = fx.pr("rebase");

    assert_eq!(run.ok()["ok"], true);
    let seen = seen(&gate);
    assert_eq!(
        seen.job,
        ("queued".to_owned(), 0),
        "the update Job did not start while the repair held the Workflow: {seen:?}"
    );
    assert!(
        !seen.dispatch.contains("\"attempt\""),
        "dispatch started no Attempt: {}",
        seen.dispatch
    );
    assert!(
        fx.fakes.spawn.launches().is_empty(),
        "no driver was spawned during the repair"
    );
}

#[test]
fn repair_marker_cleared_on_every_exit() {
    // Success: the marker is set while the Gate runs, and null once the command returned.
    let fx = fx();
    let gate = fx.watch_gate(Reply::ok(""));
    assert_eq!(fx.pr("rebase").ok()["ok"], true);
    let marker = seen(&gate)
        .marker
        .unwrap()
        .expect("set while the repair runs");
    assert_eq!(marker["command"], "pr rebase");
    assert_eq!(
        marker_of(&mut fx.store_at()),
        Ok(None),
        "cleared on success"
    );

    drop(fx);

    // `gate_failed`.
    let fx = self::fx();
    let _ = fx.watch_gate(Reply::exit(1, "", "nope"));
    assert_eq!(code(&fx.pr("rebase")).0, "gate_failed");
    assert_eq!(
        marker_of(&mut fx.store_at()),
        Ok(None),
        "cleared on gate_failed"
    );

    drop(fx);

    // The conflict stop, then `--abort`.
    let fx = rebase_fx("deploy");
    fx.hooks("pre-merge = \"gate\"\n");
    fx.conflicting(&["lead"]);
    assert_eq!(code(&fx.pr("rebase")).0, "rebase_conflict");
    assert_eq!(
        marker_of(&mut fx.store_at()),
        Ok(None),
        "cleared at the conflict stop"
    );
    assert_eq!(fx.rebase("--abort").ok()["aborted"], true);
    assert_eq!(marker_of(&mut fx.store_at()), Ok(None), "cleared on abort");
}

#[test]
fn update_dispatch_resumes_after_repair() {
    // After a success and after a refusal (`gate_failed`) the update Job starts.
    for reply in [Reply::ok(""), Reply::exit(1, "", "nope")] {
        let fx = fx();
        let gate = fx.watch_gate(reply);
        let _ = fx.pr("rebase");
        assert_eq!(
            seen(&gate).job,
            ("queued".to_owned(), 0),
            "held during the repair"
        );

        let out = fx.dispatch();

        let json: Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out}"));
        assert_eq!(json["ok"], true, "{out}");
        assert_eq!(job_state(&mut fx.store_at()).1, 1, "an Attempt started");
    }
}
