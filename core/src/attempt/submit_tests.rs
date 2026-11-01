//! Red tests for `attempt submit` from a linear Attempt's workflow worktree (contract §7.3, commands.md §6.1,
//! hs-d680/hs-d682/hs-d683): the Attempt's worktree is the PR's workflow worktree and its branch the Lead
//! Branch, so `wrong_worktree` accepts a cwd inside the workflow worktree. Red until `wlinear-impl` gives the
//! starter the workflow worktree and Lead Branch.
//!
//! The Attempt is the one `driver::start_with` records; the caller's cwd is a real temp directory that is the
//! fake git's workflow worktree, so no test changes the process's environment and none starts a process.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tempfile::TempDir;

use super::row;
use crate::config::{Catalog, Config, TierModel};
use crate::driver::{StartCtx, start_with};
use crate::fakes::git::{GitRoute, Head, Repo, Universe};
use crate::home::CpdHome;
use crate::ids::{AttemptId, JobKey, PlanId};
use crate::ports::{Fakes, Ports};
use crate::store::Store;

fn config() -> Config {
    Config::parse("version = 1\nmachine = \"test\"\n", Path::new("config.toml")).unwrap().0
}

fn catalog() -> Catalog {
    let model = |m: &str| TierModel { model: m.to_owned(), efforts: vec![1, 2, 3] };
    Catalog { pi_version: "1.0.0".to_owned(), o: model("o/m"), s: model("s/m"), h: model("h/m"), g: TierModel { model: "g/m".to_owned(), efforts: vec![] } }
}

/// Plan `pay`, its PR `api` and Job `a`, and the fake git of the workflow worktree on `cpd/pay/api`.
struct Rig {
    _tmp: TempDir,
    fakes: Fakes,
    home: CpdHome,
    store: Store,
    lead_worktree: PathBuf,
}

fn rig() -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let fakes = Fakes::new();
    let mut universe = Universe::new(fakes.clock.clone());
    universe.set_global("user.name", "cpd test");
    universe.set_global("user.email", "test@cpd.invalid");
    let universe = Arc::new(Mutex::new(universe));
    let route = Arc::new(GitRoute::new(universe.clone()));
    let _ = (*fakes.runner).clone().with_git(&route);
    let repo = Repo::init(&universe, &root.join("r"));
    repo.write("f.txt", "base\n");
    repo.commit("base");
    repo.branch("cpd/pay/api");
    let lead_worktree = root.join("r.lead");
    universe.lock().unwrap().add_worktree(repo.id(), &lead_worktree, Head::Branch("cpd/pay/api".to_owned()));

    let home = CpdHome::resolve(Some(root.join("cpd").into_os_string()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let placeholder = "f".repeat(64);
    store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '{}', 'O3', 'working', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, locked_at, created_at)
                   VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
                 INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, started_at, created_txn, created_at)
                   VALUES ('pay', 'api', '{}', 'main', 'dispatch', 'cpd/pay/api', '{}', 't', cpd_txn(), 't');
                 INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
                   VALUES ('pay', 'api', 'a', 'A', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'queued', 0, '{placeholder}', cpd_txn(), 't');",
                lead_worktree.display(),
                repo.dir().display(),
                lead_worktree.display(),
            ))?;
            Ok(())
        })
        .unwrap();
    // The Fingerprint of §2.3, computed from the stored row.
    let plan = PlanId::parse("pay").unwrap();
    let jobs = store.read(|conn| crate::plan::jobs::load(conn, &plan)).unwrap();
    let fingerprints: Vec<(String, String)> = jobs.iter().map(|j| (j.key.clone(), crate::plan::locked_fingerprint(j))).collect();
    store
        .write("fixture", |tx| {
            for (key, fingerprint) in &fingerprints {
                tx.execute("UPDATE jobs SET fingerprint = ?2 WHERE plan = 'pay' AND key = ?1", rusqlite::params![key, fingerprint])?;
            }
            Ok(())
        })
        .unwrap();
    Rig { _tmp: tmp, fakes, home, store, lead_worktree }
}

#[test]
fn a_linear_attempt_submits_from_the_workflow_worktree() {
    let mut rig = rig();
    rig.fakes.spawn.set_self_exe(std::env::current_exe().unwrap());
    let config = config();
    let catalog = catalog();
    let ctx = StartCtx { home: &rig.home, config: &config, catalog: &catalog, command: "test" };
    let plan = PlanId::parse("pay").unwrap();
    start_with(&rig.fakes.ports, &mut rig.store, &ctx, &plan, &JobKey::parse("a").unwrap()).unwrap();
    let id = AttemptId::parse("a-a1").unwrap();
    let attempt = rig.store.read(|conn| row::load(conn, &plan, &id)).unwrap();
    assert_eq!(attempt.branch, "cpd/pay/api", "the linear Attempt's branch is the Lead Branch");
    // The caller's cwd is the workflow worktree; submit checks the Attempt's own worktree and branch.
    let previous = std::env::current_dir().unwrap();
    std::env::set_current_dir(&rig.lead_worktree).unwrap();
    let worktree = PathBuf::from(&attempt.worktree);
    let ports = Ports { clock: rig.fakes.ports.clock.clone(), runner: rig.fakes.ports.runner.clone(), spawn: rig.fakes.ports.spawn.clone(), ..Ports::real() };
    let result = Ports::scope(&ports, || super::submit::require_inside(&attempt, &worktree));
    std::env::set_current_dir(previous).unwrap();
    assert!(result.is_ok(), "submit accepts a cwd in the workflow worktree on the Lead Branch: {result:?}");
}
