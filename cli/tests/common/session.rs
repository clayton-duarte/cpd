//! What the session tests share (`cli/tests/session/`): the Sessions fixtures and the readers of a Session's row and
//! Events, one definition each. A fixture takes the [`TestEnv`] (`fx.env` for an `Fx`); a reader takes either, through
//! [`Cpd`], so a file that runs the command tree in this process (`Fx`) and one that runs the binary read the same way.
//! SQL a test writes itself is `env.sql(..)`, the harness's one fixture writer.

use std::fs;
use std::sync::{Arc, Mutex, PoisonError};

use cpd_core::events::{Kind, NewEvent, append};
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::ids::{PlanId, SessionId};
use serde_json::{Value, json};

use super::fx::Fx;
use super::{Run, TestEnv};

/// Something that runs `cpd <args>`: a [`TestEnv`] (the built binary) or an [`Fx`] (the command tree in this process).
pub trait Cpd {
    fn cpd(&self, args: &[&str]) -> Run;
}

impl Cpd for TestEnv {
    fn cpd(&self, args: &[&str]) -> Run {
        TestEnv::cpd(self, args)
    }
}

impl Cpd for Fx {
    fn cpd(&self, args: &[&str]) -> Run {
        Fx::cpd(self, args)
    }
}

/// The `pi` of a test that runs the command tree in this process: a route of the `FakeRunner` that answers every call
/// of `pi` with the one [`Reply`] it holds now ([`FakePi::say`]), so a test changes the answer between two runs. What
/// pi was given (argv, stdin, environment, cwd) is the `FakeRunner`'s record: `fx.calls("pi")`.
pub struct FakePi {
    reply: Mutex<Reply>,
}

impl FakePi {
    /// Routes the `pi` of `fx`'s `FakeRunner` to a new `FakePi` that exits 0 with an empty answer.
    pub fn install(fx: &Fx) -> Arc<FakePi> {
        let pi = Arc::new(FakePi { reply: Mutex::new(Reply::ok("")) });
        let _ = (*fx.fakes.runner).clone().route("pi", pi.clone());
        pi
    }

    /// pi's answer from now on: its exit, stdout and stderr, or `Reply::io(..)` when it cannot start.
    pub fn say(&self, reply: Reply) {
        *self.reply.lock().unwrap_or_else(PoisonError::into_inner) = reply;
    }
}

impl Handler for FakePi {
    fn handle(&self, _: &Call) -> Reply {
        self.reply.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}

/// The `$CPD_HOME` of `env`.
pub fn home(env: &TestEnv) -> CpdHome {
    CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap()
}

/// The Events of `kind`, in order.
pub fn events(cpd: &impl Cpd, kind: &str) -> Vec<Value> {
    cpd.cpd(&["events", "list", "--kind", kind]).ok()["events"].as_array().unwrap().clone()
}

/// The `session` of `cpd session show <id>`.
pub fn show(cpd: &impl Cpd, id: &str) -> Value {
    cpd.cpd(&["session", "show", id]).ok()["session"].clone()
}

/// Context `ctx` with its Manager `manager1`; Plans `pay` (Lead `lead1`, idle;
/// Job `api` with Attempt `api-a1`, Worker `worker1` and an open Escalation)
/// and `other` (Lead `lead2`, stopped). Every Session's cwd is the env's root.
pub fn setup(env: &TestEnv) {
    env.write_tmux_config("[timeouts]\nfork_quiet_seconds = 1\n"); // a 1 s quiet time for forks
    let cwd = env.root().display().to_string();
    let mut store = env.store();
    store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at) VALUES
                   ('lead1', 'lead', 'ctx', 'pay', NULL, 'Lead one', '{cwd}', 'O3', 'idle', '2020-01-01T00:00:01Z', 't1'),
                   ('lead2', 'lead', 'ctx', 'other', NULL, 'Lead two', '{cwd}', 'O3', 'stopped', '2020-01-01T00:00:00Z', 't2'),
                   ('manager1', 'manager', 'ctx', NULL, NULL, 'Manager', '{cwd}', 'O5', 'stopped', '2020-01-01T00:00:00Z', 't0'),
                   ('worker1', 'worker', 'ctx', 'pay', 'api-a1', 'pay api a1', '{cwd}', 'S2', 'working', '2020-01-01T00:00:03Z', 't3');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES
                   ('pay', 'ctx', 'Pay', 'grounding', 'lead1', 't'),
                   ('other', 'ctx', 'Other', 'grounding', 'lead2', 't');
                 INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at) VALUES
                   ('pay', 'r', '/r', 'main', 'waiting', 'cpd/pay', {id}, 't'),
                   ('other', 'r', '/r', 'main', 'waiting', 'cpd/other', {id}, 't');
                 INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, created_txn, created_at)
                   VALUES ('pay', 'r', 'api', 'Api', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{fp}', {id}, 't');
                 INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, worker_session, started_at)
                   VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', 'running', 'cpd/pay--api-a1', '/w', 'worker1', 't');
                 INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at)
                   VALUES ('pay', 'api-a1-q1', 'api-a1', 1, 'Which?', 'open', 1, 't');",
                fp = "a".repeat(64),
                id = tx.id()
            ))?;
            append(
                tx,
                NewEvent::new(
                    Kind::SessionRegistered,
                    "Lead one registered.",
                    json!({"role": "lead", "name": "Lead one", "forked_from": null, "tier": "O3"}),
                )
                .plan(PlanId::parse("pay").unwrap())
                .session(SessionId::parse("lead1").unwrap()),
            )?;
            Ok(())
        })
        .unwrap();
    for plan in ["pay", "other"] {
        let path = home(env).lead_message(&PlanId::parse(plan).unwrap());
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, format!("Plan {plan}.")).unwrap();
    }
}

/// Context `ctx`; Plans `pay` (Lead `lead1`, idle, with a side fork `fork1`) and
/// `other` (Lead `lead2`, stopped); Worker `worker1` of `pay`. No Manager, no Escalation, every Session's cwd `/`.
pub fn setup_with_fork(env: &TestEnv) {
    fs::write(env.cpd_home().join("config.toml"), "version = 1\nmachine = \"m1\"\nlead_pi_packages = []\n").unwrap();
    let fp = "a".repeat(64);
    let mut store = env.store();
    store
        .write("fixture", |tx| {
            let txn = tx.id();
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, attempt, name, cwd, tier, state, state_at, created_at, forked_from) VALUES
                   ('lead1', 'lead', 'ctx', 'pay', NULL, 'Lead one', '/', 'O3', 'idle', 't1', 't1', NULL),
                   ('lead2', 'lead', 'ctx', 'other', NULL, 'Lead two', '/', 'O3', 'stopped', 't2', 't2', NULL),
                   ('fork1', 'lead', 'ctx', 'pay', NULL, 'Side fork', '/', 'O3', 'stopped', 't4', 't4', 'lead1'),
                   ('worker1', 'worker', 'ctx', 'pay', 'api-a1', 'pay api a1', '/', 'S2', 'working', 't3', 't3', NULL);
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at) VALUES
                   ('pay', 'ctx', 'Pay', 'grounding', 'lead1', 't'),
                   ('other', 'ctx', 'Other', 'grounding', 'lead2', 't');
                 INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at) VALUES
                   ('pay', 'r', '/r', 'main', 'waiting', 'cpd/pay', {txn}, 't'),
                   ('other', 'r', '/r', 'main', 'waiting', 'cpd/other', {txn}, 't');
                 INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, created_txn, created_at)
                   VALUES ('pay', 'r', 'api', 'Api', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{fp}', {txn}, 't');
                 INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, worker_session, started_at)
                   VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', 'running', 'cpd/pay--api-a1', '/w', 'worker1', 't');"
            ))?;
            Ok(())
        })
        .unwrap();
}
