//! Red tests for `pr poll`'s clean-up of a merged PR (contract §7.2, hs-d680/hs-d682/hs-d683): it removes
//! the PR's workflow worktree (and its Lead Branch) once, and each parallel Attempt worktree and branch,
//! under the every-Job-idle check. A linear Attempt is the workflow worktree and is never removed as a
//! separate Attempt worktree: the dirty/unpushed keep applies to it. Red until `wlinear-impl` teaches the
//! clean-up the linear case.
//!
//! No test runs git, reads the wall clock or sleeps: the git is the in-process `Universe` behind a
//! `GitRoute`, and the call is scoped on a `Ports`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tempfile::TempDir;

use super::poll::{Attempt, Merged, clean};
use crate::fakes::git::{GitRoute, Head, Repo, Universe};
use crate::ids::{AttemptId, PlanId, PrKey};
use crate::ports::{Fakes, Ports};

/// The repo and the two worktrees of a merged PR: the workflow worktree `L` (dirty) and a parallel `P`.
struct Rig {
    _tmp: TempDir,
    fakes: Fakes,
    repo: Repo,
    lead_worktree: PathBuf,
    parallel_worktree: PathBuf,
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
    repo.branch("cpd/pay--b-a1");
    let lead_worktree = root.join("r.lead");
    let parallel_worktree = root.join("r.parallel");
    {
        let mut u = universe.lock().unwrap();
        u.add_worktree(repo.id(), &lead_worktree, Head::Branch("cpd/pay/api".to_owned()));
        u.add_worktree(repo.id(), &parallel_worktree, Head::Branch("cpd/pay--b-a1".to_owned()));
    }
    // The workflow worktree has uncommitted work: the clean-up must keep it (and its Lead Branch).
    std::fs::write(lead_worktree.join("dirty.txt"), "the Lead's own change\n").unwrap();
    Rig { _tmp: tmp, fakes, repo, lead_worktree, parallel_worktree }
}

fn merged(rig: &Rig) -> Merged {
    let attempt = |id: &str, branch: &str, worktree: &Path| Attempt {
        id: AttemptId::parse(id).unwrap(),
        branch: branch.to_owned(),
        worktree: worktree.display().to_string(),
        ended: true,
    };
    Merged {
        plan: PlanId::parse("pay").unwrap(),
        pr: PrKey::parse("api").unwrap(),
        repo: rig.repo.dir().display().to_string(),
        lead_branch: "cpd/pay/api".to_owned(),
        lead_worktree: Some(rig.lead_worktree.display().to_string()),
        pull_request: None,
        attempts: vec![
            // The linear Attempt runs in the workflow worktree, on the Lead Branch.
            attempt("a-a1", "cpd/pay/api", &rig.lead_worktree),
            attempt("b-a1", "cpd/pay--b-a1", &rig.parallel_worktree),
        ],
        fresh: false,
    }
}

#[test]
fn a_merged_pr_removes_the_workflow_worktree_once() {
    let rig = rig();
    let merged = merged(&rig);
    let ports = Ports { clock: rig.fakes.ports.clock.clone(), runner: rig.fakes.ports.runner.clone(), spawn: rig.fakes.ports.spawn.clone() };
    let failures = Ports::scope(&ports, || clean(&merged));
    assert!(!rig.parallel_worktree.exists(), "the parallel Attempt worktree is removed");
    assert!(rig.lead_worktree.exists(), "the workflow worktree with local changes is kept, not force-removed: {failures:?}");
    assert!(
        failures.iter().any(|(path, reason)| Path::new(path) == rig.lead_worktree && reason == "dirty"),
        "the kept workflow worktree is reported `dirty`: {failures:?}"
    );
}
