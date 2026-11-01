//! `cpd janitor` (commands.md §29, contract §7.2), in process on `Fakes`: the git is the in-process `Universe`
//! behind a `GitRoute`, tmux and `gh` are scripted on the runner, the store is a migrated database in a temp dir.
//! No test starts a process, runs real git, sleeps or reads the wall clock. The [`Rig`] is also the one of
//! `pr/poll_tests.rs`.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tempfile::TempDir;

use crate::env::Env;
use crate::fakes::git::{GitRoute, Head, Repo, Universe};
use crate::git::{attempt_worktree_path, workflow_lead_worktree_path};
use crate::home::CpdHome;
use crate::host::runner::Reply;
use crate::ids::{AttemptId, PlanId, WorkflowKey};
use crate::janitor;
use crate::output::Output;
use crate::ports::{Fakes, Ports};
use crate::store::Store;

/// A Workflow the rig stores, with its two worktrees: the Lead worktree on the Lead Branch and one parallel Attempt's.
pub(crate) struct Workflow {
    pub(crate) key: &'static str,
    pub(crate) status: &'static str,
    pub(crate) lead: PathBuf,
    pub(crate) lead_branch: String,
    pub(crate) attempt: PathBuf,
    pub(crate) attempt_branch: String,
}

impl Workflow {
    /// The aside directory of a failed removal of the Lead worktree (contract §7.2).
    pub(crate) fn aside(&self) -> PathBuf {
        let mut name = self.lead.clone().into_os_string();
        name.push(".removing");
        PathBuf::from(name)
    }

    /// The aside directory of a failed removal of the Attempt worktree (contract §7.2).
    pub(crate) fn attempt_aside(&self) -> PathBuf {
        let mut name = self.attempt.clone().into_os_string();
        name.push(".removing");
        PathBuf::from(name)
    }
}

/// Plan `pay` in one repo `pay` whose `origin/main` is its only commit, with the Workflows added by [`Rig::pr`].
pub(crate) struct Rig {
    pub(crate) tmp: TempDir,
    pub(crate) fakes: Fakes,
    pub(crate) universe: Arc<Mutex<Universe>>,
    pub(crate) repo: Repo,
    pub(crate) home: CpdHome,
    pub(crate) workflows: Vec<Workflow>,
    number: i64,
    /// The Attempts [`Rig::pr`] stores next are `running`, not `ended`.
    pub(crate) running: bool,
}

impl Rig {
    pub(crate) fn new() -> Rig {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let fakes = Fakes::new();
        let mut universe = Universe::new(fakes.clock.clone());
        universe.set_global("user.name", "cpd test");
        universe.set_global("user.email", "test@cpd.invalid");
        let universe = Arc::new(Mutex::new(universe));
        let route = Arc::new(GitRoute::new(universe.clone()));
        let _ = (*fakes.runner).clone().with_git(&route);
        // No tmux server: `list-panes` and every other call fail as tmux does without one.
        let _ = (*fakes.runner).clone().on(
            "tmux",
            |_| true,
            Reply::exit(1, "", "no server running on /tmp/tmux-0/s\n"),
        );
        let repo = Repo::init(&universe, &root.join("pay"));
        repo.write("f.txt", "base\n");
        repo.commit("base");
        repo.keyed("pay");
        let home = CpdHome::resolve(Some(root.join("cpd").into_os_string()), None).unwrap();
        std::fs::create_dir_all(home.config().parent().unwrap()).unwrap();
        std::fs::write(
            home.config(),
            "version = 1\nmachine = \"t\"\n[tmux]\nsocket = \"s\"\n",
        )
        .unwrap();
        let rig = Rig {
            tmp,
            fakes,
            universe,
            repo,
            home,
            workflows: Vec::new(),
            number: 800,
            running: false,
        };
        rig.sql(
            "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
             INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
               VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
             INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
               VALUES ('pay', 'ctx', 'Pay the bills', 'deploy', 'lead1', 't', 't');",
        );
        rig
    }

    /// Runs `sql` on the store, in one transaction.
    pub(crate) fn sql(&self, sql: &str) {
        let mut store = Store::open(&self.home, 5000).unwrap();
        store
            .write("fixture", |tx| {
                tx.execute_batch(sql)?;
                Ok(())
            })
            .unwrap();
    }

    /// Workflow `key` with status `open`, `merged` or `cancelled`: its Lead worktree (clean, pushed) on its Lead Branch and one
    /// ended parallel Attempt `<key>-a1` with its worktree and branch. A cancelled Workflow's row has no `lead_worktree`,
    /// as `workflow cancel` leaves it; its Lead worktree is the path contract §7.1 names.
    pub(crate) fn workflow(&mut self, key: &'static str, status: &'static str) -> &Workflow {
        self.workflow_with(key, status, "cancelled", "failed")
    }

    /// [`Rig::pr`] with the Job's status `job` (`merged`, `running`, `failed` or `cancelled`) and the Attempt's outcome
    /// `attempt` (`failed` or `merged`). `dispatch` is a Workflow status too: a Lead worktree and no pull request yet.
    pub(crate) fn workflow_with(
        &mut self,
        key: &'static str,
        status: &'static str,
        job: &str,
        outcome: &str,
    ) -> &Workflow {
        let plan = PlanId::parse("pay").unwrap();
        let lead_branch = format!("cpd/pay/{key}");
        let lead = workflow_lead_worktree_path(
            self.repo.dir(),
            &plan,
            &WorkflowKey::parse(key).unwrap(),
            &lead_branch,
        )
        .unwrap();
        let attempt_id = format!("{key}-a1");
        let attempt_branch = format!("cpd/pay--{attempt_id}");
        let attempt = attempt_worktree_path(
            self.repo.dir(),
            &plan,
            &AttemptId::parse(&attempt_id).unwrap(),
        )
        .unwrap();
        self.repo.branch(&lead_branch);
        self.repo.branch(&attempt_branch);
        {
            let mut u = self.universe.lock().unwrap();
            u.add_worktree(self.repo.id(), &lead, Head::Branch(lead_branch.clone()));
            u.add_worktree(
                self.repo.id(),
                &attempt,
                Head::Branch(attempt_branch.clone()),
            );
        }
        // `add_worktree` makes an empty directory: write the tracked file so that both worktrees are clean.
        for dir in [&lead, &attempt] {
            std::fs::write(dir.join("f.txt"), "base\n").unwrap();
        }
        self.number += 1;
        let n = self.number;
        let repo = self.repo.dir().display();
        let (pull, lead_column, stamps) = match status {
            "cancelled" => (
                String::new(),
                "NULL".to_owned(),
                "NULL, NULL, NULL, NULL, NULL".to_owned(),
            ),
            "dispatch" => (
                String::new(),
                format!("'{}'", lead.display()),
                "'t', NULL, NULL, NULL, NULL".to_owned(),
            ),
            _ => {
                let merged = if status == "merged" { "'t'" } else { "NULL" };
                let state = if status == "merged" { "merged" } else { "open" };
                (
                    format!(
                        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                           VALUES ('acme/pay', {n}, 'https://github.com/acme/pay/pull/{n}', 'Workflow {n}', 0, '{state}', 'none', 'none', 'opened', {merged}, 'opened');"
                    ),
                    format!("'{}'", lead.display()),
                    format!("'t', {n}, 'https://github.com/acme/pay/pull/{n}', 'opened', (SELECT id FROM pull_requests WHERE number = {n})"),
                )
            }
        };
        let merged_at = if status == "merged" { "'t'" } else { "NULL" };
        let attempt_state = match (self.running, outcome) {
            (true, _) => "'running', NULL, NULL, NULL",
            (false, "merged") => "'ended', 'merged', 'merged', 'm'",
            (false, _) => "'ended', 'failed', 'crashed', NULL",
        };
        let job_columns = match job {
            "merged" => "NULL, NULL, NULL, 'm'",
            "failed" => "NULL, NULL, 'crashed', NULL",
            "cancelled" => "'no', 't', NULL, NULL",
            _ => "NULL, NULL, NULL, NULL",
        };
        let ended_at = if self.running { "NULL" } else { "'t'" };
        self.sql(&format!(
            "{pull}
             INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at,
                              pr_number, pr_url, pr_opened_at, pull_request, merged_at)
               VALUES ('pay', '{key}', '{repo}', 'pay', 'main', '{status}', '{lead_branch}', {lead_column}, cpd_txn(), 't', {stamps}, {merged_at});
             INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                               status, appended, fingerprint, cancel_reason, cancel_requested_at, failed_cause, merged_sha, created_txn, created_at)
               VALUES ('pay', '{key}', '{key}', 'B', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '{job}', 0, '{fp}', {job_columns}, cpd_txn(), 't');
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, merged_sha, branch, worktree, base_sha, started_at, ended_at)
               VALUES ('pay', '{attempt_id}', '{key}', 1, 'S2', 'ladder', {attempt_state}, '{attempt_branch}', '{aw}', 'sha', 't', {ended_at});",
            fp = "f".repeat(64),
            aw = attempt.display(),
        ));
        self.workflows.push(Workflow {
            key,
            status: if status == "dispatch" {
                "dispatch"
            } else if status == "merged" {
                "merged"
            } else if status == "cancelled" {
                "cancelled"
            } else {
                "open"
            },
            lead,
            lead_branch,
            attempt,
            attempt_branch,
            attempt_id,
        });
        self.workflows.last().unwrap()
    }

    /// Whether the local branch exists in the repo.
    pub(crate) fn branch_exists(&self, branch: &str) -> bool {
        self.universe
            .lock()
            .unwrap()
            .ref_oid(self.repo.id(), &format!("refs/heads/{branch}"))
            .is_some()
    }

    /// Whether the repo still registers a worktree at `path`.
    pub(crate) fn registered(&self, path: &Path) -> bool {
        self.universe
            .lock()
            .unwrap()
            .worktrees(self.repo.id())
            .iter()
            .any(|w| w.path == path)
    }

    /// Everything of the Workflow still on disk or in git: `(lead directory, lead registration, lead branch, attempt
    /// directory, attempt registration, attempt branch)`.
    pub(crate) fn left(&self, workflow: &Workflow) -> [bool; 6] {
        [
            workflow.lead.exists(),
            self.registered(&workflow.lead),
            self.branch_exists(&workflow.lead_branch),
            workflow.attempt.exists(),
            self.registered(&workflow.attempt),
            self.branch_exists(&workflow.attempt_branch),
        ]
    }

    /// The `error` Events of the store as `(text, data)`, in order.
    pub(crate) fn errors(&self) -> Vec<(String, Value)> {
        let mut store = Store::open(&self.home, 5000).unwrap();
        store
            .read(|c| {
                let mut stmt =
                    c.prepare("SELECT text, data FROM events WHERE kind = 'error' ORDER BY id")?;
                let rows = stmt
                    .query_map([], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            serde_json::from_str(&r.get::<_, String>(1)?).unwrap(),
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap()
    }

    /// One janitor pass on the fake ports.
    pub(crate) fn pass(&self) -> Output {
        let mut env = Env::open_home_with(&self.fakes.ports, self.home.clone()).unwrap();
        Ports::scope(&self.fakes.ports, || janitor::pass(&mut env)).unwrap()
    }
}

fn fields(output: Output) -> serde_json::Map<String, Value> {
    match output {
        Output::Success(map) => map,
        Output::Owned => panic!("the janitor prints its own envelope"),
    }
}

/// The `outcomes` of a pass, as `(pr, outcome)`.
fn outcomes(output: Output) -> Vec<(String, String)> {
    let map = fields(output);
    map["outcomes"]
        .as_array()
        .unwrap_or_else(|| panic!("a pass that ran has outcomes: {map:?}"))
        .iter()
        .map(|o| {
            (
                o["pr"].as_str().unwrap().to_owned(),
                o["outcome"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[test]
fn janitor_pass_removes_cancelled_and_merged_pr_worktrees_and_branches() {
    let mut rig = Rig::new();
    rig.workflow("api", "merged");
    rig.workflow("web", "cancelled");
    let outcomes = outcomes(rig.pass());
    for workflow in &rig.workflows {
        assert_eq!(
            rig.left(workflow),
            [false; 6],
            "{} Workflow {}: every worktree, registration and local branch is gone",
            workflow.status,
            workflow.key
        );
        assert!(
            outcomes.iter().any(|(key, _)| key == workflow.key),
            "an outcome for Workflow {}: {outcomes:?}",
            workflow.key
        );
    }
    assert!(
        outcomes.iter().all(|(_, outcome)| outcome == "removed"),
        "{outcomes:?}"
    );
    assert_eq!(rig.errors(), Vec::new(), "no Event");
}

#[test]
fn janitor_pass_skips_a_pr_whose_attempt_is_still_running() {
    let mut rig = Rig::new();
    rig.running = true;
    rig.workflow("api", "merged");
    rig.running = false;
    rig.workflow("web", "cancelled");
    rig.workflow("lock", "cancelled");
    // The driver of `web-a1` is alive: the lock file of the Attempt is held.
    std::fs::create_dir_all(rig.home.locks_dir()).unwrap();
    let held = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(rig.home.attempt_lock(
            &PlanId::parse("pay").unwrap(),
            &AttemptId::parse("web-a1").unwrap(),
        ))
        .unwrap();
    assert!(crate::lock::try_lock(&held).unwrap());
    let outcomes = outcomes(rig.pass());
    for key in ["api", "web"] {
        let workflow = rig.workflows.iter().find(|p| p.key == key).unwrap();
        assert_eq!(rig.left(workflow), [true; 6], "Workflow {key} is untouched");
        assert!(
            outcomes.contains(&(key.to_owned(), "pending".to_owned())),
            "Workflow {key} is pending: {outcomes:?}"
        );
    }
    let idle = rig.workflows.iter().find(|p| p.key == "lock").unwrap();
    assert_eq!(
        rig.left(idle),
        [false; 6],
        "an idle Workflow is cleaned in the same pass"
    );
    assert_eq!(rig.errors(), Vec::new(), "no Event");
}

fn dirty_lead_is_kept_untouched(status: &'static str, other: &str) {
    let mut rig = Rig::new();
    rig.workflow("api", status);
    let workflow = &rig.workflows[0];
    std::fs::write(workflow.lead.join("dirty.txt"), "the Lead's own change\n").unwrap();
    let outcomes = outcomes(rig.pass());
    let workflow = &rig.workflows[0];
    assert!(
        workflow.lead.join("dirty.txt").exists(),
        "the dirty worktree is unchanged"
    );
    assert_eq!(
        (
            rig.registered(&workflow.lead),
            rig.branch_exists(&workflow.lead_branch),
            workflow.aside().exists()
        ),
        (true, true, false)
    );
    assert!(
        outcomes.contains(&("api".to_owned(), "kept".to_owned())),
        "{outcomes:?}"
    );
    let errors = rig.errors();
    assert_eq!(errors.len(), 1, "one Event: {errors:?}");
    let (text, data) = &errors[0];
    assert_eq!(data["code"], "cleanup_failed");
    assert!(
        text.contains(status) && !text.contains(other),
        "the text names the status {status}, never {other}: {text}"
    );
    assert!(text.contains("untouched"), "{text}");
    assert!(
        text.contains(&workflow.lead.display().to_string()),
        "{text}"
    );
    assert_eq!(data["details"]["status"], status);
    assert_eq!(data["details"]["outcome"], "kept");
    assert_eq!(data["details"]["path"], workflow.lead.display().to_string());
}

#[test]
fn janitor_pass_says_kept_untouched_for_a_dirty_lead_worktree_of_a_cancelled_pr() {
    dirty_lead_is_kept_untouched("cancelled", "merged");
}

#[test]
fn janitor_pass_says_kept_untouched_for_a_dirty_lead_worktree_of_a_merged_pr() {
    dirty_lead_is_kept_untouched("merged", "cancelled");
}

#[test]
fn janitor_pass_reports_a_kept_worktree_once() {
    let mut rig = Rig::new();
    rig.workflow("api", "cancelled");
    std::fs::write(rig.workflows[0].lead.join("dirty.txt"), "wip\n").unwrap();
    rig.pass();
    assert_eq!(rig.errors().len(), 1, "the first pass reports it");
    let second = outcomes(rig.pass());
    assert!(
        second.contains(&("api".to_owned(), "kept".to_owned())),
        "the second pass still sees it: {second:?}"
    );
    assert_eq!(rig.errors().len(), 1, "a later pass retries silently");
}

#[test]
fn janitor_pass_finishes_what_a_killed_pass_left() {
    let mut rig = Rig::new();
    rig.workflow("api", "merged");
    let workflow = &rig.workflows[0];
    // A pass killed after the rename: the path is gone, the aside is the marker, the registration and branch remain.
    std::fs::rename(&workflow.lead, workflow.aside()).unwrap();
    assert!(rig.registered(&workflow.lead) && rig.branch_exists(&workflow.lead_branch));
    let outcomes = outcomes(rig.pass());
    let workflow = &rig.workflows[0];
    assert_eq!(
        (
            workflow.lead.exists(),
            workflow.aside().exists(),
            rig.registered(&workflow.lead),
            rig.branch_exists(&workflow.lead_branch)
        ),
        (false, false, false, false)
    );
    assert!(
        outcomes.iter().all(|(_, outcome)| outcome == "removed") && !outcomes.is_empty(),
        "{outcomes:?}"
    );
    assert_eq!(rig.errors(), Vec::new(), "no Event");
}

#[test]
fn janitor_pass_finishes_an_attempt_worktree_left_as_removing() {
    let mut rig = Rig::new();
    rig.workflow("api", "merged");
    let workflow = &rig.workflows[0];
    // A pass or A9 killed after the rename: the path is gone, the aside is the marker, the registration and branch remain.
    std::fs::rename(&workflow.attempt, workflow.attempt_aside()).unwrap();
    let outcomes = outcomes(rig.pass());
    let workflow = &rig.workflows[0];
    assert_eq!(rig.left(workflow), [false; 6]);
    assert!(!workflow.attempt_aside().exists());
    assert!(
        outcomes.iter().all(|(_, outcome)| outcome == "removed") && !outcomes.is_empty(),
        "{outcomes:?}"
    );
    assert_eq!(rig.errors(), Vec::new(), "no Event");
}

#[test]
fn janitor_pass_while_another_holds_the_lock_does_nothing() {
    let mut rig = Rig::new();
    rig.workflow("api", "merged");
    rig.workflow("web", "cancelled");
    std::fs::create_dir_all(rig.home.locks_dir()).unwrap();
    let held = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(rig.home.locks_dir().join("janitor.lock"))
        .unwrap();
    assert!(crate::lock::try_lock(&held).unwrap());
    assert!(
        janitor::running(&rig.home).unwrap(),
        "the probe sees the held lock"
    );
    let map = fields(rig.pass());
    assert_eq!(map.get("skipped"), Some(&Value::from("running")), "{map:?}");
    assert!(!map.contains_key("outcomes"), "{map:?}");
    for workflow in &rig.workflows {
        assert_eq!(
            rig.left(workflow),
            [true; 6],
            "Workflow {} is untouched",
            workflow.key
        );
    }
    assert_eq!(rig.errors(), Vec::new(), "no Event");
}

/// Contract §7.2 "Ended Attempts of merged Jobs on a live Workflow": the Workflow `key` in `status` with its Job in status `job` and
/// its Attempt ended `attempt`, after one pass: `(left, outcomes, errors)`.
fn live_pass(
    status: &'static str,
    job: &str,
    attempt: &str,
) -> (Rig, [bool; 6], Vec<(String, String)>) {
    let mut rig = Rig::new();
    rig.workflow_with("api", status, job, attempt);
    let outcomes = outcomes(rig.pass());
    let left = rig.left(&rig.workflows[0]);
    (rig, left, outcomes)
}

fn swept_and_lead_untouched(status: &'static str, job: &str, attempt: &str) {
    let (rig, left, outcomes) = live_pass(status, job, attempt);
    assert_eq!(
        left,
        [true, true, true, false, false, false],
        "the Lead's three stay, the Attempt's three are gone"
    );
    assert!(
        outcomes.contains(&("api".to_owned(), "removed".to_owned())),
        "{outcomes:?}"
    );
    assert!(
        outcomes.iter().all(|(_, outcome)| outcome == "removed"),
        "{outcomes:?}"
    );
    assert_eq!(rig.errors(), Vec::new(), "no Event");
}

#[test]
fn janitor_pass_sweeps_the_attempt_of_a_merged_job_of_an_open_pr() {
    swept_and_lead_untouched("open", "merged", "failed");
}

#[test]
fn janitor_pass_sweeps_the_attempt_of_a_merged_job_of_a_dispatch_pr() {
    swept_and_lead_untouched("dispatch", "merged", "failed");
}

#[test]
fn janitor_pass_sweeps_a_merged_attempt_of_a_live_pr_whatever_its_job_status() {
    swept_and_lead_untouched("open", "running", "merged");
}

#[test]
fn janitor_pass_keeps_the_attempts_of_a_live_pr_whose_job_is_not_merged() {
    let (rig, left, outcomes) = live_pass("open", "failed", "failed");
    assert_eq!(
        left, [true; 6],
        "a failed Job's Attempts are kept for debugging"
    );
    assert!(
        outcomes.iter().all(|(key, _)| key != "api"),
        "no outcome for the Workflow: {outcomes:?}"
    );
    assert_eq!(rig.errors(), Vec::new(), "no Event");
}

#[test]
fn janitor_pass_leaves_a_live_prs_running_attempt_alone_and_sweeps_the_merged_jobs() {
    let mut rig = Rig::new();
    rig.workflow_with("a", "open", "merged", "failed");
    let plan = PlanId::parse("pay").unwrap();
    let branch = "cpd/pay--b-a1".to_owned();
    let dir =
        attempt_worktree_path(rig.repo.dir(), &plan, &AttemptId::parse("b-a1").unwrap()).unwrap();
    rig.repo.branch(&branch);
    rig.universe
        .lock()
        .unwrap()
        .add_worktree(rig.repo.id(), &dir, Head::Branch(branch.clone()));
    std::fs::write(dir.join("f.txt"), "base\n").unwrap();
    rig.sql(&format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                           status, appended, fingerprint, created_txn, created_at)
           VALUES ('pay', 'a', 'b', 'B', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{fp}', cpd_txn(), 't');
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha, started_at)
           VALUES ('pay', 'b-a1', 'b', 1, 'S2', 'ladder', 'running', '{branch}', '{dir}', 'sha', 't');",
        fp = "f".repeat(64),
        dir = dir.display(),
    ));
    let outcomes = outcomes(rig.pass());
    assert_eq!(
        rig.left(&rig.workflows[0]),
        [true, true, true, false, false, false],
        "a-a1 is swept, the Lead stays"
    );
    assert_eq!(
        (
            dir.exists(),
            rig.registered(&dir),
            rig.branch_exists(&branch)
        ),
        (true, true, true),
        "b-a1 is untouched"
    );
    assert!(
        outcomes.contains(&("a".to_owned(), "removed".to_owned())),
        "{outcomes:?}"
    );
    assert_eq!(rig.errors(), Vec::new(), "no Event");
}

#[test]
fn janitor_pass_skips_a_swept_attempt_whose_driver_lock_is_held() {
    let mut rig = Rig::new();
    rig.workflow_with("api", "open", "merged", "failed");
    std::fs::create_dir_all(rig.home.locks_dir()).unwrap();
    let held = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(rig.home.attempt_lock(
            &PlanId::parse("pay").unwrap(),
            &AttemptId::parse("api-a1").unwrap(),
        ))
        .unwrap();
    assert!(crate::lock::try_lock(&held).unwrap());
    let outcomes = outcomes(rig.pass());
    assert_eq!(rig.left(&rig.workflows[0]), [true; 6]);
    assert!(
        outcomes.contains(&("api".to_owned(), "pending".to_owned())),
        "{outcomes:?}"
    );
    assert_eq!(rig.errors(), Vec::new(), "no Event");
}
