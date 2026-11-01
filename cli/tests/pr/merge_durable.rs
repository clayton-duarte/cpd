//! R (contract §7.4) for a `durable` Worker: the merge recovery of an Attempt in `merging` whose driver died ends the Attempt
//! and stops its Worker by the Attempt's `runner`, which for `durable` is a SIGKILL of the stored `worker_pid` at once
//! (§3.4 "The Worker runner and its seam"), guarded: the pid's `cmdline` must hold the runner file and this Attempt's spec path.
//! In-process on the fake ports: the git route, the `FakeSpawn`, no sleep.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::fx::Fx as Kit;
use cpd_core::config::{Catalog, Config, TierModel};
use cpd_core::git::{Setup, create_attempt_worktree, create_lead_worktree};
use cpd_core::home::CpdHome;
use cpd_core::host::spawn::{Pid, Signal, Spawn};
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::merge::{self, Ctx, End, Failure};
use cpd_core::store::Store;
use rusqlite::params;
use serde_json::Value;

fn plan() -> PlanId {
    PlanId::parse("pay").unwrap()
}

fn catalog() -> Catalog {
    let model = |m: &str| TierModel { model: m.to_owned(), efforts: vec![0, 1, 2, 3, 4, 5] };
    Catalog { pi_version: "1.2.0".to_owned(), o: model("p/opus"), s: model("p/sonnet"), h: model("p/haiku"), g: TierModel { model: "p/gemini".to_owned(), efforts: vec![] } }
}

fn metrics(_: &merge::Attempt) -> Option<Value> {
    None
}

struct Fx {
    kit: Kit,
    home: CpdHome,
    config: Config,
    catalog: Catalog,
    repo: PathBuf,
    lead: PathBuf,
}

/// A repo with a Lead worktree for Plan `pay` (PR `repo`, Lead Branch `cpd/pay`), `pi_adapter` named in the config.
fn fx() -> Fx {
    let kit = Kit::with_ports();
    let repo = kit.git_repo("pay").canonicalize().unwrap();
    let home = CpdHome::resolve(Some(kit.env.cpd_home().into()), None).unwrap();
    let adapter = kit.env.root().join("pi");
    fs::create_dir_all(adapter.join("durable-worker")).unwrap();
    fs::write(adapter.join("package.json"), "{}").unwrap();
    fs::write(adapter.join("durable-worker/main.ts"), "").unwrap();
    fs::write(
        home.config(),
        format!("version = 1\nmachine = \"t\"\npi_adapter = \"{}\"\n[tmux]\nsocket = \"{}\"\n", adapter.display(), kit.env.tmux_socket()),
    )
    .unwrap();
    let config = Config::load(&home).unwrap();
    let setup = Setup { timeout: Duration::from_secs(30), log: kit.env.root().join("lead.log") };
    let lead = kit.scoped(|| create_lead_worktree(&repo, &plan(), "main", &setup)).unwrap().path;
    kit.env.sql(&format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
         INSERT INTO prs (plan, key, repo, name, base, status, lead_branch, lead_worktree, title, description, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '{repo}', 'repo', 'main', 'dispatch', 'cpd/pay', '{lead}', 'Charge the card', 'A description', cpd_txn(), 't', 't');",
        repo = repo.display(),
        lead = lead.display()
    ));
    kit.env.repo_policy("acme", "repo", "");
    Fx { kit, home, config, catalog: catalog(), repo, lead }
}

impl Fx {
    fn store(&self) -> Store {
        Store::open(&self.home, 5000).unwrap()
    }

    /// Job `key` `running` with Attempt `<key>-a1` in `merging`: its branch holds a commit the Lead Branch does not (the driver
    /// died before step 8), runner `durable` and `worker_pid` `pid`.
    fn merging_durable(&self, key: &str, pid: Pid) -> (AttemptId, PathBuf) {
        let attempt = AttemptId::parse(&format!("{key}-a1")).unwrap();
        let setup = Setup { timeout: Duration::from_secs(30), log: self.kit.env.root().join("attempt.log") };
        let made = self.kit.scoped(|| create_attempt_worktree(&self.repo, &self.lead, &plan(), &attempt, &setup)).unwrap();
        fs::write(made.path.join("api.txt"), "api\n").unwrap();
        self.kit.git(&made.path, &["add", "-A"]);
        self.kit.git(&made.path, &["commit", "-q", "-m", "edit api.txt"]);
        let sha = self.kit.git(&made.path, &["rev-parse", "HEAD"]);
        self.store()
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, kind, fingerprint, created_txn, created_at)
                     VALUES ('pay', 'repo', ?1, 'T', 'S2', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', 'running', 0, 'work', ?2, ?3, 't')",
                    params![key, "f".repeat(64), tx.id()],
                )?;
                tx.execute(
                    "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha,
                                           submitted_sha, submitted_at, started_at, driver_pid, runner, worker_pid)
                     VALUES ('pay', ?1, ?2, 1, 'S2', 'ladder', 'merging', ?3, ?4, ?5, ?6, 't', 't', 4242, 'durable', ?7)",
                    params![attempt.to_string(), key, made.branch, made.path.to_str().unwrap(), made.base_sha, sha, pid.0],
                )?;
                Ok(())
            })
            .unwrap();
        (attempt, made.path)
    }

    /// R as E2 runs it, for `attempt`: no driver holds its lock.
    fn recover(&self, attempt: &AttemptId) -> Option<merge::Recovered> {
        let ctx = Ctx { home: &self.home, config: &self.config, catalog: &self.catalog, source: "watch", metrics: &metrics };
        self.kit.scoped(|| merge::recover_attempt(&mut self.store(), &ctx, &plan(), attempt)).unwrap()
    }

    fn attempt_row(&self, attempt: &AttemptId) -> (String, Option<String>, Option<String>) {
        self.store()
            .read(|c| {
                Ok(c.query_row("SELECT status, outcome, cause FROM attempts WHERE plan = 'pay' AND id = ?1", [attempt.to_string()], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })?)
            })
            .unwrap()
    }
}

#[test]
fn r_kills_the_durable_runner_of_a_merging_attempt_whose_driver_died() {
    let fx = fx();
    let spec = fx.home.worker_spec(&plan(), &AttemptId::parse("api-a1").unwrap());
    let runner = fx.kit.fakes.spawn.add_process(Pid(1), &format!("/opt/node/bin/node {} --model p/sonnet --spec {}", fx.kit.env.root().join("pi/durable-worker/main.ts").display(), spec.display()));
    let (attempt, worktree) = fx.merging_durable("api", runner);

    let recovered = fx.recover(&attempt).expect("the driver lock is free: R runs");

    // R3: the merge did not happen; the Attempt ends `crashed` and its worktree is kept.
    assert!(matches!(recovered.end, Some(End::Failed { cause: Failure::Crashed, .. })), "{recovered:?}");
    assert_eq!(fx.attempt_row(&attempt), ("ended".to_owned(), Some("failed".to_owned()), Some("crashed".to_owned())));
    assert!(Path::new(&worktree).exists());
    // R8: SIGKILL at once, by the stored pid: no SIGTERM, no wait.
    assert_eq!(fx.kit.fakes.spawn.signals(), [(runner, Signal::Kill)]);
    assert!(!fx.kit.fakes.spawn.alive(runner));
}
