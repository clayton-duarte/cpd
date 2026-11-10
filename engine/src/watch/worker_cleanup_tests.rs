//! Crash recovery must not discard an Attempt worktree after the driver dies (§3.4).
//! Git, tmux, and worker processes are fakes; only the fixture files and store are real.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use cpd_core::driver::lock::Starter;
use cpd_core::fakes::git::{GitRoute, Head, Repo, Universe};
use cpd_core::git::run::{ok_with, run_with};
use cpd_core::host::spawn::{Pid, Signal};
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::ports::{Fakes, Ports};

use super::liveness;
use super::testkit::{Fx, ago};

struct Case {
    fx: Fx,
    fakes: Fakes,
    route: Arc<GitRoute>,
    universe: Arc<Mutex<Universe>>,
    repo: Repo,
    tree: PathBuf,
    branch: String,
    pid: Pid,
    tmux: cpd_core::host::runner::FakeRunner,
}

impl Case {
    fn new(status: &str, linear: bool) -> Self {
        let mut fx = Fx::with_plan("");
        fx.add_running("work", "running", &ago(1));
        let tmux = fx.on_fake_tmux();
        let fakes = Fakes::new();
        let root = fx
            .engine
            .home
            .root()
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap();
        let mut universe = Universe::new(fakes.clock.clone());
        universe.set_global("user.name", "cpd test");
        universe.set_global("user.email", "test@cpd.invalid");
        let universe = Arc::new(Mutex::new(universe));
        let route = Arc::new(GitRoute::new(universe.clone()));
        let _ = (*fakes.runner).clone().with_git(&route);
        let repo = Repo::init(&universe, &root.join("repo"));
        repo.write("tracked.txt", "base\n");
        let submitted = repo.commit("submitted");
        let branch = if linear {
            "cpd/pay"
        } else {
            "cpd/pay--work-a1"
        }
        .to_owned();
        repo.branch("cpd/pay");
        if !linear {
            repo.branch(&branch);
        }
        let tree = if linear {
            root.join("lead")
        } else {
            root.join("attempt")
        };
        {
            let mut u = universe.lock().unwrap();
            u.add_worktree(
                repo.id(),
                &root.join("lead"),
                Head::Branch("cpd/pay".into()),
            );
            if !linear {
                u.add_worktree(repo.id(), &tree, Head::Branch(branch.clone()));
            }
        }
        std::fs::write(root.join("lead/tracked.txt"), "base\n").unwrap();
        std::fs::write(tree.join("tracked.txt"), "base\n").unwrap();
        let runner_file = fx
            .engine
            .config
            .pi_adapter_dir()
            .unwrap()
            .join("durable-worker/main.ts");
        let plan = PlanId::parse("pay").unwrap();
        let id = AttemptId::parse("work-a1").unwrap();
        let spec = fx.engine.home.worker_spec(&plan, &id);
        let pid = fakes.spawn.add_process(
            Pid(1),
            &format!("{} --spec {}", runner_file.display(), spec.display()),
        );
        fx.fixture(&format!(
            "UPDATE attempts SET status = '{status}', branch = '{branch}', worktree = '{}', base_sha = '{submitted}', submitted_sha = '{submitted}', submitted_at = 't', runner = 'durable', worker_pid = {} WHERE id = 'work-a1';",
            tree.display(), pid.0
        ));
        fx.engine.ports = fakes.ports.clone();
        Self {
            fx,
            fakes,
            route,
            universe,
            repo,
            tree,
            branch,
            pid,
            tmux,
        }
    }

    fn run(&mut self) {
        Ports::scope(&self.fakes.ports, || liveness::run(&mut self.fx.engine));
        self.route.assert_supported();
        assert!(self.fakes.runner.unscripted().is_empty());
        assert!(self.tmux.unscripted().is_empty());
        assert_eq!(self.fx.count("error"), 0);
    }

    fn git(&self, args: &[&str]) -> String {
        ok_with(&*self.fakes.runner, &self.tree, args).unwrap()
    }

    fn status(&mut self) -> String {
        self.fx
            .query("SELECT status FROM attempts WHERE id = 'work-a1'")
    }

    fn branch_tip(&self) -> String {
        self.universe
            .lock()
            .unwrap()
            .ref_oid(self.repo.id(), &format!("refs/heads/{}", self.branch))
            .unwrap()
            .to_string()
    }
}

#[test]
fn driver_death_cleanup_preserves_in_progress_work() {
    // A live driver owns its worker and worktree: E2 must leave all of them alone.
    let mut live = Case::new("running", true);
    std::fs::write(live.tree.join("tracked.txt"), "live edit\n").unwrap();
    let plan = PlanId::parse("pay").unwrap();
    let id = AttemptId::parse("work-a1").unwrap();
    let held = Starter::take(&live.fx.engine.home, &plan, &id).unwrap();
    live.run();
    assert_eq!(live.status(), "running");
    assert_eq!(
        std::fs::read(live.tree.join("tracked.txt")).unwrap(),
        b"live edit\n"
    );
    assert!(live.fakes.spawn.signals().is_empty());
    drop(held);

    // A liveness probe error is not proof of death either.
    let mut uncertain = Case::new("running", true);
    let lock_path = uncertain.fx.engine.home.attempt_lock(&plan, &id);
    std::fs::create_dir_all(&lock_path).unwrap();
    std::fs::write(uncertain.tree.join("tracked.txt"), "uncertain edit\n").unwrap();
    Ports::scope(&uncertain.fakes.ports, || {
        liveness::run(&mut uncertain.fx.engine)
    });
    uncertain.route.assert_supported();
    assert_eq!(uncertain.status(), "running");
    assert!(uncertain.fakes.spawn.signals().is_empty());
    assert_eq!(
        std::fs::read(uncertain.tree.join("tracked.txt")).unwrap(),
        b"uncertain edit\n"
    );

    // For a dead driver, uncertain edits and operation markers survive, even though its worker is stopped.
    for marker in ["MERGE_HEAD", "rebase-merge/orig-head"] {
        let mut dead = Case::new("running", true);
        let admin = dead.universe.lock().unwrap().admin_dir(&dead.tree);
        let marker_path = admin.join(marker);
        std::fs::create_dir_all(marker_path.parent().unwrap()).unwrap();
        std::fs::write(&marker_path, "operation state\n").unwrap();
        std::fs::write(dead.tree.join("tracked.txt"), "staged edit\n").unwrap();
        dead.git(&["add", "tracked.txt"]);
        std::fs::write(dead.tree.join("tracked.txt"), "in progress\n").unwrap();
        std::fs::write(dead.tree.join("untracked.txt"), "keep me\n").unwrap();
        let tip = dead.branch_tip();
        let head = dead.git(&["rev-parse", "--verify", "HEAD"]);
        let before = dead.git(&["status", "--porcelain"]);
        let index = dead.git(&["write-tree"]);
        assert!(
            before.contains("MM tracked.txt"),
            "fixture must have staged and unstaged changes: {before}"
        );
        assert!(before.contains("?? untracked.txt"));
        dead.run();
        assert_eq!(dead.status(), "ended");
        assert_eq!(dead.fakes.spawn.signals(), vec![(dead.pid, Signal::Kill)]);
        assert_eq!(dead.branch_tip(), tip);
        assert_eq!(dead.git(&["rev-parse", "--verify", "HEAD"]), head);
        assert_eq!(dead.git(&["status", "--porcelain"]), before);
        assert_eq!(dead.git(&["write-tree"]), index);
        assert_eq!(
            std::fs::read(dead.tree.join("tracked.txt")).unwrap(),
            b"in progress\n"
        );
        assert_eq!(
            std::fs::read(dead.tree.join("untracked.txt")).unwrap(),
            b"keep me\n"
        );
        assert_eq!(std::fs::read(marker_path).unwrap(), b"operation state\n");
    }

    // Fake git models conflict stages for both operations; MERGE_HEAD is supplied as a fixture
    // because the fake does not model that marker. A stopped rebase models its own metadata.
    for operation in ["merge", "rebase"] {
        let mut rebasing = Case::new("running", true);
        let base = rebasing
            .universe
            .lock()
            .unwrap()
            .ref_oid(rebasing.repo.id(), "refs/heads/main")
            .unwrap();
        {
            let mut u = rebasing.universe.lock().unwrap();
            let main_blob = u.blob(rebasing.repo.id(), b"main change\n");
            let main_tree = u.tree(rebasing.repo.id(), &[("tracked.txt", &main_blob)]);
            let main = u.commit(
                rebasing.repo.id(),
                &main_tree,
                std::slice::from_ref(&base),
                "main",
            );
            u.set_ref(rebasing.repo.id(), "refs/heads/main", &main, "commit: main");
            let topic_blob = u.blob(rebasing.repo.id(), b"topic change\n");
            let topic_tree = u.tree(rebasing.repo.id(), &[("tracked.txt", &topic_blob)]);
            let topic = u.commit(
                rebasing.repo.id(),
                &topic_tree,
                std::slice::from_ref(&base),
                "topic",
            );
            u.set_ref(
                rebasing.repo.id(),
                "refs/heads/cpd/pay",
                &topic,
                "commit: topic",
            );
        }
        std::fs::write(rebasing.tree.join("tracked.txt"), "topic change\n").unwrap();
        let out = run_with(
            &*rebasing.fakes.runner,
            &rebasing.tree,
            &[operation, "main"],
        )
        .unwrap();
        assert_eq!(
            out.code, 1,
            "fixture must stop at a conflict: {}",
            out.stderr
        );
        let admin = rebasing.universe.lock().unwrap().admin_dir(&rebasing.tree);
        let marker = if operation == "merge" {
            let marker = admin.join("MERGE_HEAD");
            std::fs::write(&marker, "merge operation\n").unwrap();
            marker
        } else {
            admin.join("rebase-merge/orig-head")
        };
        let marker_before = std::fs::read(&marker).unwrap();
        let conflicted = std::fs::read(rebasing.tree.join("tracked.txt")).unwrap();
        let status = rebasing.git(&["status", "--porcelain"]);
        assert!(
            status.contains("UU tracked.txt"),
            "fixture has no unmerged stages: {status}"
        );
        let head = rebasing.git(&["rev-parse", "--verify", "HEAD"]);
        let tip = rebasing.branch_tip();
        rebasing.run();
        assert_eq!(rebasing.status(), "ended");
        assert_eq!(
            rebasing.fakes.spawn.signals(),
            vec![(rebasing.pid, Signal::Kill)]
        );
        assert_eq!(rebasing.git(&["status", "--porcelain"]), status);
        assert_eq!(rebasing.git(&["rev-parse", "--verify", "HEAD"]), head);
        assert_eq!(rebasing.branch_tip(), tip);
        assert_eq!(
            std::fs::read(rebasing.tree.join("tracked.txt")).unwrap(),
            conflicted
        );
        assert_eq!(std::fs::read(marker).unwrap(), marker_before);
    }

    // R2 currently removes a clean parallel worktree and its branch after the merge landed.
    // Driver-death recovery must retain them as found, even when A9 would remove them.
    let mut merged = Case::new("merging", false);
    let tip = merged.branch_tip();
    merged.run();
    assert_eq!(merged.status(), "ended");
    assert_eq!(
        merged.fakes.spawn.signals(),
        vec![(merged.pid, Signal::Kill)]
    );
    assert!(
        merged.tree.exists(),
        "R2 removed the dead driver's worktree"
    );
    assert_eq!(merged.branch_tip(), tip, "R2 removed or moved the branch");
}
