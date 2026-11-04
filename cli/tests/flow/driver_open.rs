//! The Attempt driver of an `open` or `update` Job (contract §3.4, §7.4, §7.7), and the stall rule (§3.4 A4, hs-d195), in this process:
//! `job dispatch` records the driver's `Launch` on the `FakeSpawn`, and the test runs it through `run_with` ([`Fx::run_launch_with`]) on
//! the fake git (a bare `origin` that takes the push), the fake tmux, a scripted `gh` and a Gate hook that is a reply. The Worker is
//! the test's own closure, called after the driver's first poll: it does what pi would do in its pane (commit, `cpd attempt submit`),
//! on the same thread, between two polls of the driver, with no process and no wait. Time is the `ManualClock`.

use crate::common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::fx::Fx;
use cpd_core::clock::Clock;
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, FakeRunner, Handler, Reply, Runner};
use cpd_core::host::spawn::{Pid, Signal, Spawn};
use cpd_core::ids::{AttemptId, PlanId};
use serde_json::{Value, json};

use super::driver::{
    JOB, PLAN, PR_TITLE, PR_URL, add_job_in, fixture, seed_home, seed_rows, store,
};

/// One run of a Gate hook, as the fake `sh` saw it.
#[derive(Clone, Debug)]
struct GateRun {
    /// The rendered command (`sh -c <command>`).
    command: String,
    /// The directory it ran in.
    cwd: PathBuf,
    /// The commit checked out there, read when the hook runs.
    head: String,
    /// Whether the hook's environment names a test slot (`CPD_TEST_SLOT`): there are no slots since #425.
    slotted: bool,
}

/// The Gate's hooks, replies of the fake `sh`: each notes its command, its directory and the commit the hook runs on (the head of its
/// worktree, read when it runs) and answers `reply`.
struct Hooks {
    runner: Arc<dyn Runner>,
    runs: Mutex<Vec<GateRun>>,
    reply: Mutex<Reply>,
}

impl Handler for Hooks {
    fn handle(&self, call: &Call) -> Reply {
        let cwd = call.cwd.clone().expect("a hook has a cwd");
        let mut head = Command::new("git");
        head.args(["rev-parse", "HEAD"]).current_dir(&cwd);
        let head = self
            .runner
            .output(&mut head, "git", Duration::from_secs(5))
            .expect("git rev-parse");
        self.runs.lock().unwrap().push(GateRun {
            command: call.args[1].clone(),
            cwd,
            head: String::from_utf8_lossy(&head.stdout).trim().to_owned(),
            slotted: call.envs.iter().any(|(k, _)| k == "CPD_TEST_SLOT"),
        });
        self.reply.lock().unwrap().clone()
    }
}

/// Plan `pay` as `pr start` leaves it, in this process: a repo `pay` whose `origin` is the bare repo `repos/origin.git` (a repo of the
/// fake git, which holds `main` and the Lead Branch), its Lead worktree, the rows (Job `api` queued) and the work session of the fake tmux.
pub(super) struct Live {
    pub(super) fx: Fx,
    pub(super) repo: PathBuf,
    pub(super) lead: PathBuf,
    origin: PathBuf,
    hooks: Arc<Hooks>,
}

pub(super) fn live(config_extra: &str) -> Live {
    let fx = Fx::with_ports();
    let repo = fx.keyed_repo("pay");
    let origin = fx.env.root().join("repos/origin.git");
    fx.git(
        fx.env.root(),
        &[
            "init",
            "--bare",
            "-q",
            "-b",
            "main",
            origin.to_str().unwrap(),
        ],
    );
    fx.local_origin(&repo, &origin);
    // The Lead worktree as `pr start` makes it, and both branches pushed (R35).
    let lead = repo.parent().unwrap().join("pay.pay");
    fx.git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "cpd/pay/repo",
            lead.to_str().unwrap(),
            "main",
        ],
    );
    fx.git(&repo, &["push", "-q", "origin", "main", "cpd/pay/repo"]);
    // These tests pin the pre-triage Gate retry (pro-d280): no classifier call.
    let config = if config_extra.contains("failure_triage") {
        config_extra.to_string()
    } else if config_extra.contains("[experimental]") {
        config_extra.replacen(
            "[experimental]",
            "[experimental]\nfailure_triage = false",
            1,
        )
    } else {
        format!("{config_extra}\n[experimental]\nfailure_triage = false\n")
    };
    seed_home(&fx.env, &config);
    // The work session as the engine leaves it: its `watch` window, and remain-on-exit (§6.1).
    for args in [
        &[
            "new-session",
            "-d",
            "-s",
            "cpd-work",
            "-n",
            "watch",
            "sleep",
            "600",
        ][..],
        &["set", "-g", "remain-on-exit", "on"],
    ] {
        assert!(fx.tmux(args).status.success(), "tmux {args:?}");
    }
    seed_rows(&fx.env, &repo, &lead);
    add_job_in(&fx.env, "repo", JOB, &[]);
    // The test binary stands for cpd's own executable: a launched driver is run by the test, never started.
    fx.fakes
        .spawn
        .set_self_exe(std::env::current_exe().unwrap());
    let hooks = Arc::new(Hooks {
        runner: fx.fakes.runner.weak(),
        runs: Mutex::new(Vec::new()),
        reply: Mutex::new(Reply::ok("")),
    });
    let _ = (*fx.fakes.runner).clone().route("sh", hooks.clone());
    Live {
        fx,
        repo,
        lead,
        origin,
        hooks,
    }
}

impl Live {
    pub(super) fn runner(&self) -> FakeRunner {
        (*self.fx.fakes.runner).clone()
    }

    pub(super) fn fixture(&self, sql: &str) {
        fixture(&self.fx.env, sql);
    }

    /// One value of the store, read through a store opened and closed here: `env.query` keeps a connection, and with it a
    /// descriptor, open, which could be the fd 3 a driver is handed.
    pub(super) fn value<T: rusqlite::types::FromSql>(&self, sql: &str) -> T {
        store(&self.fx.env)
            .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }

    pub(super) fn job_status(&self, key: &str) -> String {
        self.value(&format!(
            "SELECT status FROM jobs WHERE plan = 'pay' AND key = '{key}'"
        ))
    }

    pub(super) fn phase(&self) -> String {
        self.value("SELECT phase FROM plans WHERE id = 'pay'")
    }

    /// The worktree Attempt `id` runs in, as the starter recorded it: the PR's Lead worktree for a linear Attempt (the only
    /// live Attempt of its PR, hs-d680), its own `pay.pay--<id>` for a parallel one. Before the Attempt exists, the parallel path.
    pub(super) fn worktree(&self, id: &str) -> PathBuf {
        let stored: Option<String> = store(&self.fx.env)
            .read(|c| {
                Ok(c.query_row(
                    "SELECT worktree FROM attempts WHERE plan = 'pay' AND id = ?1",
                    [id],
                    |r| r.get(0),
                )
                .ok())
            })
            .unwrap();
        stored.map_or_else(
            || self.repo.parent().unwrap().join(format!("pay.pay--{id}")),
            PathBuf::from,
        )
    }

    /// [`Live::worktree`] as a hook sees it as its cwd (the hook runner canonicalizes it): the Attempt's worktree is gone once it ends.
    fn canonical_worktree(&self, id: &str) -> PathBuf {
        self.worktree(id).canonicalize().unwrap_or_else(|_| {
            self.repo
                .canonicalize()
                .unwrap()
                .parent()
                .unwrap()
                .join(format!("pay.pay--{id}"))
        })
    }

    pub(super) fn lead_sha(&self) -> String {
        self.fx
            .git(&self.repo, &["rev-parse", "refs/heads/cpd/pay/repo"])
    }

    pub(super) fn remote(&self, branch: &str) -> String {
        self.fx.git(
            &self.origin,
            &["rev-parse", &format!("refs/heads/{branch}")],
        )
    }

    /// Commits `path` with `content` in `dir` and returns the commit.
    pub(super) fn commit(&self, dir: &Path, path: &str, content: &str, message: &str) -> String {
        fs::write(dir.join(path), content).unwrap();
        self.fx.git(dir, &["add", "-A"]);
        self.fx.git(dir, &["commit", "-q", "-m", message]);
        self.fx.git(dir, &["rev-parse", "HEAD"])
    }

    /// Moves `main` on `origin` (through the main checkout) with a commit writing `path`.
    fn advance_base(&self, path: &str, content: &str) -> String {
        let at = self.commit(&self.repo, path, content, &format!("base {path}"));
        self.fx.git(&self.repo, &["push", "-q", "origin", "main"]);
        at
    }

    /// Sets the `[hooks]` of the repo `acme/pay`'s policy file: `text` is `[pre-merge]` and its `gate`, and an empty commit is made
    /// on the Lead Branch, where the Attempt worktrees are cut from.
    fn hooks(&self, text: &str) {
        let file = format!("[hooks]\n{text}").replace("\n[", "\n[hooks.");
        self.fx.env.repo_policy("acme", "pay", &file);
        self.fx.git(
            &self.lead,
            &["commit", "-q", "--allow-empty", "-m", "hooks"],
        );
    }

    /// What the Gates noted: `<command> head=<sha>`, one per run of a hook, in order.
    pub(super) fn gate_runs(&self) -> Vec<String> {
        self.hooks
            .runs
            .lock()
            .unwrap()
            .iter()
            .map(|r| format!("{} head={}", r.command, r.head))
            .collect()
    }

    /// The directory each run of a hook was in, in order.
    fn gate_dirs(&self) -> Vec<PathBuf> {
        self.hooks
            .runs
            .lock()
            .unwrap()
            .iter()
            .map(|r| r.cwd.clone())
            .collect()
    }

    /// There is no test slot (#425): no hook was given one, and no slot lock file exists in `$CPD_HOME/run`, and no gate worktree is left
    /// beside the repo.
    fn assert_no_slot_and_no_gate_worktree(&self) {
        assert!(
            self.hooks.runs.lock().unwrap().iter().all(|r| !r.slotted),
            "a hook was given a test slot"
        );
        let names = |dir: &Path| -> Vec<String> {
            fs::read_dir(dir)
                .map(|d| {
                    d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default()
        };
        let slots: Vec<String> = names(&self.fx.env.cpd_home().join("run"))
            .into_iter()
            .filter(|n| n.starts_with("test-"))
            .collect();
        assert!(slots.is_empty(), "{slots:?}");
        let gates: Vec<String> = names(self.repo.parent().unwrap())
            .into_iter()
            .filter(|n| n.contains(".gate-"))
            .collect();
        assert!(gates.is_empty(), "{gates:?}");
    }

    /// The fake `gh`: no pull request of the branch yet, `pr create` answers [`PR_URL`], and the lookup of the pull request answers it.
    pub(super) fn gh_creates(&self) {
        self.runner()
            .on("gh", |c| c.args[..2] == ["pr", "list"], Reply::ok("[]"))
            .on("gh", |c| c.args[..2] == ["pr", "create"], Reply::ok(PR_URL))
            .on(
                "gh",
                |c| c.args[..2] == ["api", "graphql"],
                Reply::ok(&common::pay_lookup()),
            );
    }

    /// `gh`'s calls that are not the poll's or a lookup's: `pr list`, `pr create`.
    pub(super) fn gh_pr_calls(&self) -> Vec<Vec<String>> {
        self.fx
            .calls("gh")
            .into_iter()
            .map(|c| c.args)
            .filter(|a| a[0] == "pr")
            .collect()
    }

    /// The Events of `kind` of Plan `pay`: `(txn, data)`, in order.
    pub(super) fn plan_events(&self, kind: &str) -> Vec<(i64, Value)> {
        store(&self.fx.env)
            .read(|c| {
                let mut stmt = c.prepare(
                    "SELECT txn, data FROM events WHERE plan = 'pay' AND kind = ?1 ORDER BY id",
                )?;
                let rows = stmt
                    .query_map([kind], |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            serde_json::from_str(&r.get::<_, String>(1)?).unwrap(),
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap()
    }

    /// `pr` of the open PR or the PR that is `dispatch`: `(status, pr_number, pr_opened_at)`.
    pub(super) fn pr_row(&self) -> (String, Option<i64>, Option<String>) {
        store(&self.fx.env)
            .read(|c| Ok(c.query_row("SELECT status, pr_number, pr_opened_at FROM workflows WHERE plan = 'pay' AND key = 'repo'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?))
            .unwrap()
    }

    pub(super) fn attempt(&self, id: &str) -> Value {
        self.fx.cpd(&["attempt", "show", id, "--plan", PLAN]).ok()["attempt"].clone()
    }

    /// The Events about Attempt `id`, in order.
    pub(super) fn events(&self, id: &str) -> Vec<Value> {
        let all = self.fx.cpd(&["events", "list", "--plan", PLAN]).ok()["events"]
            .as_array()
            .unwrap()
            .clone();
        all.into_iter().filter(|e| e["attempt"] == id).collect()
    }

    fn event(&self, id: &str, kind: &str) -> Value {
        self.events(id)
            .into_iter()
            .find(|e| e["kind"] == kind)
            .unwrap_or_else(|| panic!("no {kind} for {id}"))
    }

    /// Job `key` of PR `repo` as cpd adds an `open` or `update` Job: `appended`, no Dependencies, `queued` with its fingerprint.
    pub(super) fn add_cpd_job(&self, key: &str, kind: &str) {
        self.fixture(&format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, created_txn, created_at)
             VALUES ('pay', 'repo', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 1, '{kind}', cpd_txn(), 't');"
        ));
        let plan = PlanId::parse(PLAN).unwrap();
        let all = store(&self.fx.env)
            .read(|conn| cpd_core::plan::jobs::load(conn, &plan))
            .unwrap();
        let job = all.iter().find(|j| j.key == key).unwrap();
        self.fixture(&format!(
            "UPDATE jobs SET status = 'queued', fingerprint = '{}' WHERE plan = 'pay' AND key = '{key}';",
            cpd_core::plan::locked_fingerprint(job)
        ));
    }

    /// Dispatches Job `key` and runs its driver on this thread: `hook(self, id, n)` is called after the driver's `n`th nap (from 1),
    /// where the Worker's pi acts. Returns the id of the Attempt and what the driver printed.
    pub(super) fn drive(
        &self,
        key: &str,
        hook: impl FnMut(&Live, &str, usize),
    ) -> (String, common::Run) {
        self.drive_as(key, false, hook)
    }

    /// [`Live::drive`] for an Attempt that is PARALLEL (hs-d683: its PR has a live sibling): `job dispatch` always asks for the linear
    /// one, so the Attempt it records, still `starting`, is given its own `cpd/<plan>--<attempt>` branch and worktree before its
    /// driver runs, as the starter records a parallel one. The driver then creates the worktree and runs `pre-start` (A2), rebases
    /// and fast-forwards with the compare-and-swap (§7.4) and removes the worktree at its end (A9).
    pub(super) fn drive_parallel(
        &self,
        key: &str,
        hook: impl FnMut(&Live, &str, usize),
    ) -> (String, common::Run) {
        self.drive_as(key, true, hook)
    }

    /// Gives the `starting` Attempt `id` the branch and worktree the starter records for a parallel one.
    pub(super) fn make_parallel(&self, id: &str) {
        let worktree = self.repo.parent().unwrap().join(format!("pay.pay--{id}"));
        self.fixture(&format!(
            "UPDATE attempts SET branch = 'cpd/pay--{id}', worktree = '{}' WHERE plan = 'pay' AND id = '{id}' AND status = 'starting';",
            worktree.display()
        ));
    }

    fn drive_as(
        &self,
        key: &str,
        parallel: bool,
        mut hook: impl FnMut(&Live, &str, usize),
    ) -> (String, common::Run) {
        let out = self.dispatch(key).ok();
        let id = out["attempt"]
            .as_str()
            .unwrap_or_else(|| panic!("no attempt in {out}"))
            .to_owned();
        if parallel {
            self.make_parallel(&id);
        }
        let launch = self
            .fx
            .take_launch(self.fx.fakes.spawn.launches().len() - 1);
        let home = CpdHome::resolve(Some(self.fx.env.cpd_home().into()), None).unwrap();
        let lock = home.attempt_lock(
            &PlanId::parse(PLAN).unwrap(),
            &AttemptId::parse(&id).unwrap(),
        );
        // The starter holds the driver lock (`flock`) and hands it to the driver as fd 3 (§3.4): take it here, or an engine tick
        // run while this driver lives (E2's `lock::probe`) sees a free lock and ends the Attempt `crashed` for a dead driver.
        let lock_file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&lock)
            .unwrap();
        assert!(
            cpd_core::lock::try_lock(&lock_file).unwrap(),
            "the driver lock of {id} is free"
        );
        let _fd3 = self.fx.hand_over(lock_file);
        let ran = self
            .fx
            .run_launch_with(&launch, |_, nap| hook(self, &id, nap));
        assert_eq!(ran.exit, 0, "{}", ran.stdout);
        (id, ran)
    }

    /// [`Live::drive`] with a Worker that acts once, in the driver's first poll: returns the Attempt it ended and what `worker`
    /// returned. `worker` gets the id of the Attempt.
    pub(super) fn run_job<W>(
        &self,
        key: &str,
        worker: impl FnOnce(&Live, &str) -> W,
    ) -> (Value, W) {
        let (mut worker, mut did) = (Some(worker), None);
        let (id, _) = self.drive(key, |live, id, nap| {
            if nap == 1 {
                did = Some((worker.take().expect("the first nap is once"))(live, id));
            }
        });
        (
            self.attempt(&id),
            did.expect("the driver napped: the Worker ran"),
        )
    }

    /// `job dispatch <key>`: the driver's `Launch` is recorded, nothing is started.
    pub(super) fn dispatch(&self, key: &str) -> common::Run {
        self.fx.cpd(&["job", "dispatch", key, "--plan", PLAN])
    }
}

/// What `attempt submit` answered the Worker.
pub(super) fn submit(live: &Live, id: &str) -> Value {
    // The environment of the Worker's window: its Plan and its Attempt.

    live.fx
        .cpd_with(
            &live.worktree(id),
            &["attempt", "submit", id],
            &[("CPD_PLAN", PLAN), ("CPD_ATTEMPT", id)],
        )
        .json()
}

/// A Worker that did nothing: it submits the Lead Branch as it is.
pub(super) fn submit_only(live: &Live, id: &str) -> Value {
    submit(live, id)
}

/// A Worker that merges the base into its branch, resolving the conflict on its own side, and submits. (`FakeGit` has no `MERGE_HEAD`:
/// a conflicted merge cannot be concluded by a `commit`, so the Worker resolves in the merge, with `-X ours`.)
fn merge_the_base_and_submit(live: &Live, id: &str) -> Value {
    let wt = live.worktree(id);
    live.fx.git(&wt, &["fetch", "-q", "origin", "main"]);
    live.fx.git(
        &wt,
        &["merge", "-q", "--no-edit", "-X", "ours", "origin/main"],
    );
    submit(live, id)
}

/// PR `repo` as `pr start` leaves it, with a bare `origin` that holds `main` and the Lead Branch as the lock pushed it, the policy
/// both Gates `local` (`[gate] merge` and `pr`, contract §8.3) with a `pre-commit` and a `pre-merge` hook that note the commit they run on, Job `api` merged (its commit `api.txt` is on
/// the Lead Branch, **not pushed**), and the PR's `open` Job `open-repo` queued. `gh` is the test's to script.
pub(super) fn open_live() -> Live {
    open_live_with("")
}

/// [`open_live`] with `config_extra` appended to the home config (`retry_cap = 0`, ...).
fn open_live_with(config_extra: &str) -> Live {
    let live = live(config_extra);
    live.fx
        .env
        .owner_policy("acme", "[gate]\nmerge = \"local\"\npr = \"local\"\n");
    let merged = live.commit(&live.lead, "api.txt", "api\n", "api");
    live.fixture(&format!(
        "UPDATE jobs SET status = 'running' WHERE key = 'api'; UPDATE jobs SET status = 'merged', merged_sha = '{merged}' WHERE key = 'api';"
    ));
    live.hooks("[pre-commit]\ngate = \"make quick\"\n\n[pre-merge]\ngate = \"make test\"\n");
    live.add_cpd_job("open-repo", "open");
    live.fixture(
        "UPDATE workflows SET started_at = '2026-10-27T09:00:00.000Z' WHERE key = 'repo';",
    );
    live
}

/// The same with the pull request already open (812, as `pr open` leaves it), the Plan in `deploy`, and no `open` Job.
pub(super) fn open_pr_live() -> Live {
    let live = open_live();
    live.fixture(&format!(
        "UPDATE jobs SET status = 'cancelled', cancel_reason = 'opened', cancel_requested_at = 't' WHERE key = 'open-repo';
         INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, mergeable, merge_state, created_at, merged_at, changed_at)
           VALUES ('acme/pay', 812, '{PR_URL}', '{PR_TITLE}', 0, 'open', 'none', 'none', 'mergeable', 'clean', 'opened', NULL, 'opened');
         UPDATE workflows SET status = 'open', pr_number = 812, pr_url = '{PR_URL}', pr_opened_at = 'opened', pull_request = (SELECT id FROM pull_requests) WHERE key = 'repo';
         UPDATE plans SET phase = 'deploy' WHERE id = 'pay';"
    ));
    live
}

/// A Worker that did nothing, with the base unmoved or moved without a conflict, submits the Lead Branch as it is: its submit runs no
/// Gate, and its merge merges nothing (the Lead Branch does not move), runs both Gates (`gate.merge`, then `gate.pr`) in the Attempt
/// worktree at its HEAD, which is the Lead tip, with no test slot, pushes it, and runs `gh pr create` (never with `--draft`), writing
/// `pr open`'s rows and Events in the transaction that ends the Job `merged` (contract §7.4 step 6, hs-d679).
#[test]
fn an_open_job_with_no_conflict_merges_nothing_runs_both_gates_at_its_merge_pushes_and_opens_the_pull_request()
 {
    let live = open_live();
    live.gh_creates();
    let tip = live.lead_sha();
    // The base moved, without a conflict: that is no reason to merge it.
    let base = live.advance_base("base.txt", "base\n");
    assert_ne!(
        live.remote("cpd/pay/repo"),
        tip,
        "the Lead's commit is not pushed yet"
    );

    // The submit runs no Gate: the Worker's closure ends right after it, before the driver merges.
    let (attempt, submitted) = live.run_job("open-repo", |live, id| {
        let submitted = submit(live, id);
        assert_eq!(
            live.gate_runs(),
            Vec::<String>::new(),
            "the submit ran a Gate"
        );
        submitted
    });

    assert_eq!(
        (&attempt["outcome"], &attempt["cause"]),
        (&json!("merged"), &json!("merged")),
        "{attempt}"
    );
    assert_eq!(submitted["ok"], true, "{submitted}");
    assert_eq!(live.job_status("open-repo"), "merged");
    // Nothing merged: the Lead Branch is where it was, and does not hold the base.
    assert_eq!(live.lead_sha(), tip);
    assert_eq!(attempt["merged_sha"], tip);
    assert!(!live.lead.join("base.txt").exists());
    assert_ne!(
        live.fx.git(&live.repo, &["merge-base", &tip, &base]),
        base,
        "the base is not an ancestor of the Lead Branch"
    );
    // Both Gates ran at the merge, once each and in this order, at the Lead tip, in the Attempt's own worktree (not a gate worktree, no slot).
    assert_eq!(
        live.gate_runs(),
        [
            format!("make quick head={tip}"),
            format!("make test head={tip}")
        ]
    );
    let worktree = live.canonical_worktree("open-repo-a1");
    assert_eq!(live.gate_dirs(), [worktree.clone(), worktree]);
    live.assert_no_slot_and_no_gate_worktree();
    let logs = live.fx.env.cpd_home().join("run/pay/logs");
    let log = fs::read_to_string(logs.join("open-repo-a1.pre-merge.log")).unwrap();
    assert!(!log.contains("slot"), "{log}");
    assert!(
        logs.join("open-repo-a1.pre-commit.log").exists(),
        "the gate.merge log is the Attempt's"
    );
    // Pushed, and the pull request created: ready, never a draft.
    assert_eq!(live.remote("cpd/pay/repo"), tip);
    let calls = live.gh_pr_calls();
    assert_eq!(
        calls[0],
        [
            "pr",
            "list",
            "--head",
            "cpd/pay/repo",
            "--state",
            "all",
            "--json",
            "number,url,state,createdAt"
        ]
    );
    let created: Vec<&Vec<String>> = calls.iter().filter(|c| c[1] == "create").collect();
    assert_eq!(created.len(), 1, "{calls:?}");
    assert_eq!(
        created[0][..8],
        [
            "pr",
            "create",
            "--base",
            "main",
            "--head",
            "cpd/pay/repo",
            "--title",
            PR_TITLE
        ]
    );
    assert!(
        live.fx.calls("gh").iter().all(|c| !c
            .args
            .iter()
            .any(|a| a == "--draft" || a == "ready" || a == "convert-to-draft")),
        "never a draft"
    );
    // `pr open`'s rows and Events, with `job-merged`, in one transaction.
    let (status, number, opened_at) = live.pr_row();
    assert_eq!(
        (status.as_str(), number, opened_at.is_some()),
        ("open", Some(812), true)
    );
    assert_eq!(live.phase(), "deploy");
    assert_eq!(
        live.value::<String>("SELECT pr_url FROM workflows WHERE plan = 'pay'"),
        PR_URL
    );
    assert_eq!(live.value::<i64>("SELECT count(*) FROM pull_requests WHERE repo = 'acme/pay' AND number = 812 AND state = 'open' AND draft = 0"), 1);
    assert!(
        live.value::<Option<i64>>("SELECT pull_request FROM workflows WHERE plan = 'pay'")
            .is_some()
    );
    let (merged, opened, phase) = (
        live.plan_events("job-merged"),
        live.plan_events("pr-opened"),
        live.plan_events("plan-phase"),
    );
    let ours: Vec<&(i64, Value)> = merged
        .iter()
        .filter(|e| e.1["sha"] == tip.as_str() && e.1["kind"] == "open")
        .collect();
    assert_eq!(ours.len(), 1, "{merged:?}");
    assert_eq!(
        (
            &ours[0].1["workflow"],
            &ours[0].1["number"],
            &ours[0].1["url"]
        ),
        (&json!("repo"), &json!(812), &json!(PR_URL))
    );
    assert_eq!(
        opened.iter().map(|e| e.1.clone()).collect::<Vec<_>>(),
        [json!({"workflow": "repo", "number": 812, "url": PR_URL})]
    );
    assert_eq!(
        (phase.len(), &phase[0].1["from"], &phase[0].1["to"]),
        (1, &json!("dispatch"), &json!("deploy"))
    );
    assert_eq!(
        (opened[0].0, phase[0].0),
        (ours[0].0, ours[0].0),
        "one transaction"
    );
}

/// An `open` Job's checks (base check, Gate) are one `checks` span, closed in the transaction that makes the Attempt `merging`.
#[test]
fn an_open_jobs_checks_are_one_span_that_ends_in_the_transaction_that_makes_the_attempt_merging() {
    let live = open_live();
    live.gh_creates();
    live.run_job("open-repo", submit_only);
    let span =
        |edge: &str| json!({"from": "running", "to": "running", "check": edge, "what": "checks"});
    let statuses: Vec<Value> = live
        .events("open-repo-a1")
        .iter()
        .filter(|e| e["kind"] == "attempt-status")
        .map(|e| e["data"].clone())
        .collect();
    assert_eq!(
        statuses[1..4],
        [
            span("start"),
            span("end"),
            json!({"from": "running", "to": "merging"})
        ],
        "{statuses:?}"
    );
    let txns: String = live.value(
        "SELECT group_concat(txn) FROM (SELECT txn FROM events WHERE kind = 'attempt-status' AND attempt = 'open-repo-a1' ORDER BY rowid)",
    );
    let txns: Vec<&str> = txns.split(',').collect();
    assert_eq!(
        txns[2], txns[3],
        "the end and running -> merging share a transaction"
    );
}

/// With a base that conflicts with the Lead Branch, the submit is accepted (it runs no base check, no Gate: #425) and the base check is
/// the first step of the Attempt's merge (§7.4 step 6), before both Gates: the Attempt ends `failed` / `merge_conflict` (retried at the same Tier),
/// the conflicted files and the command that merges the base in its detail, and nothing is gated, pushed or opened. The Job goes back to
/// `queued`; the next Attempt's Worker merges the base and resolves it, its merge commit lands as it is (no rebase), both Gates run on
/// it, and the pull request opens.
#[test]
fn an_open_job_with_a_conflicting_base_fails_its_merge_merge_conflict_until_the_next_attempt_merges_the_base()
 {
    let live = open_live();
    live.gh_creates();
    // The Lead Branch changed README.md, and so did the base.
    live.commit(&live.lead, "README.md", "mine\n", "mine");
    let tip = live.lead_sha();
    let base = live.advance_base("README.md", "theirs\n");
    let remote = live.remote("cpd/pay/repo");

    // The first Worker does not merge the base: its submit is accepted, and the merge is what finds the conflict.
    let (first, submitted) = live.run_job("open-repo", submit_only);

    assert_eq!(submitted["ok"], true, "{submitted}");
    assert_eq!(
        (&first["outcome"], &first["cause"]),
        (&json!("failed"), &json!("merge_conflict")),
        "{first}"
    );
    // The detail of the failure is the feedback the next Attempt's Spec carries.
    let retry = live.event("open-repo-a1", "attempt-retry");
    let detail = retry["data"]["feedback"].as_str().unwrap();
    assert!(
        detail.contains("README.md") && detail.contains(&base[..7]),
        "the conflicted file and the base are named: {retry}"
    );
    assert!(
        detail.contains("merge it into your branch")
            && detail.contains("cpd attempt submit open-repo-a1"),
        "what the Worker does is named: {retry}"
    );
    assert!(
        detail.contains("Nothing was gated, pushed or opened"),
        "{retry}"
    );
    assert_eq!(
        live.job_status("open-repo"),
        "queued",
        "retried at the same Tier"
    );
    // Before the Gates: nothing gated, pushed, created or written.
    assert_eq!(
        live.gate_runs(),
        Vec::<String>::new(),
        "no Gate ran on a head the base refuses"
    );
    assert_eq!(
        (live.lead_sha(), live.remote("cpd/pay/repo")),
        (tip.clone(), remote)
    );
    assert!(
        live.gh_pr_calls().iter().all(|c| c[1] != "create"),
        "{:?}",
        live.gh_pr_calls()
    );
    assert_eq!(
        (live.pr_row().0.as_str(), live.phase().as_str()),
        ("dispatch", "dispatch")
    );

    // The next Attempt's Worker merges the base and resolves the conflict: its merge commit lands as it is, and the PR opens.
    let (second, submitted) = live.run_job("open-repo", merge_the_base_and_submit);

    assert_eq!(
        (&second["outcome"], &second["cause"]),
        (&json!("merged"), &json!("merged")),
        "{second}"
    );
    assert_eq!(second["id"], "open-repo-a2");
    assert_eq!(submitted["ok"], true, "{submitted}");
    let new = live.lead_sha();
    assert_ne!(new, tip);
    assert_eq!(
        live.fx.git(&live.repo, &["rev-parse", &format!("{new}~1")]),
        tip
    );
    assert!(
        live.fx
            .git_try(&live.repo, &["merge-base", "--is-ancestor", &base, &new])
            .status
            .success(),
        "the merge commit holds the base"
    );
    assert_eq!(
        fs::read_to_string(live.lead.join("README.md")).unwrap(),
        "mine\n"
    );
    assert_eq!(
        live.gate_runs(),
        [
            format!("make quick head={new}"),
            format!("make test head={new}")
        ],
        "both Gates ran once, on the resolved head"
    );
    live.assert_no_slot_and_no_gate_worktree();
    assert_eq!(live.remote("cpd/pay/repo"), new);
    assert_eq!(
        live.gh_pr_calls()
            .iter()
            .filter(|c| c[1] == "create")
            .count(),
        1
    );
    assert_eq!(live.pr_row().0, "open");
    assert_eq!(live.job_status("open-repo"), "merged");
}

/// A Gate failure is the merge's, not the submit's (#425): the submit is accepted (it runs no Gate), the merge runs `gate.pr` in the Attempt
/// worktree, and its failure ends the Attempt `failed` / `gate_failed` with the hook's tail and log in the detail, nothing pushed, created or
/// written. `gate_failed` is no ladder cause: the Job fails and the Lead decides.
#[test]
fn an_open_jobs_gate_failure_ends_its_merge_gate_failed_and_pushes_nothing_creates_nothing_and_fails_the_attempt()
 {
    // pro-d280: a gate_failed Attempt is retried once by default; retry_cap = 0 keeps this test on the Lead-decides path.
    let live = open_live_with("retry_cap = 0");
    live.gh_creates();
    live.hooks("[pre-merge]\ngate = \"echo the gate says no; exit 1\"\n");
    *live.hooks.reply.lock().unwrap() = Reply::exit(1, "the gate says no\n", "");
    let before = live.remote("cpd/pay/repo");

    let (attempt, submitted) = live.run_job("open-repo", submit_only);

    assert_eq!(
        submitted["ok"], true,
        "the submit runs no Gate: {submitted}"
    );
    assert_eq!(
        (&attempt["outcome"], &attempt["cause"]),
        (&json!("failed"), &json!("gate_failed")),
        "{attempt}"
    );
    let failed = live.plan_events("job-failed");
    assert_eq!(failed.len(), 1, "{failed:?}");
    let detail = failed[0].1["detail"].as_str().unwrap();
    assert!(detail.contains("pre-merge failed (exit 1)"), "{detail}");
    assert!(
        detail.contains("the gate says no"),
        "the hook's tail is in the detail: {detail}"
    );
    assert!(
        detail.contains("open-repo-a1.pre-merge.log"),
        "the hook's log is in the detail: {detail}"
    );
    assert_eq!(
        live.job_status("open-repo"),
        "failed",
        "gate_failed is no ladder cause: the Lead decides"
    );
    assert_eq!(
        live.gate_dirs(),
        [live.canonical_worktree("open-repo-a1")],
        "the Gate ran once, in the Attempt worktree"
    );
    live.assert_no_slot_and_no_gate_worktree();
    assert_eq!(live.remote("cpd/pay/repo"), before, "nothing was pushed");
    assert!(
        live.gh_pr_calls().iter().all(|c| c[1] != "create"),
        "{:?}",
        live.gh_pr_calls()
    );
    assert_eq!(
        (live.pr_row().0.as_str(), live.phase().as_str()),
        ("dispatch", "dispatch")
    );
    assert!(live.plan_events("pr-opened").is_empty());
}

/// The scripted Gate output: 80 numbered lines of 4,000 characters, a blank line between `line 78` and `line 79`.
fn long_gate_output() -> String {
    let mut out = String::new();
    for n in 1..=80 {
        if n == 79 {
            out.push('\n');
        }
        out.push_str(&format!("line {n:02} {}\n", "x".repeat(4000 - 8)));
    }
    out
}

/// The `text` of the `job-failed` Events of Plan `pay`, in order.
fn job_failed_texts(live: &Live) -> Vec<String> {
    store(&live.fx.env)
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT text FROM events WHERE plan = 'pay' AND kind = 'job-failed' ORDER BY id",
            )?;
            Ok(stmt
                .query_map([], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

/// Runs `open-repo` against a hook that answers `reply`; returns the one `job-failed` Event's `(text, data.detail)`.
fn gate_failure_text(reply: Reply) -> (String, String, Live) {
    // pro-d280: retry_cap = 0, so the failure is the Lead's (`job-failed`), not retried.
    let live = open_live_with("retry_cap = 0");
    live.gh_creates();
    live.hooks("[pre-merge]\ngate = \"make gate\"\n");
    *live.hooks.reply.lock().unwrap() = reply;
    live.run_job("open-repo", submit_only);
    let texts = job_failed_texts(&live);
    assert_eq!(texts.len(), 1, "{texts:?}");
    let failed = live.plan_events("job-failed");
    (
        texts[0].clone(),
        failed[0].1["detail"].as_str().unwrap().to_string(),
        live,
    )
}

/// The last five non-blank lines of the scripted output, cut as the Event cuts them.
fn last_five_cut() -> Vec<String> {
    let out = long_gate_output();
    let lines: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len() - 5..]
        .iter()
        .map(|l| format!("{}…", l.trim_end().chars().take(159).collect::<String>()))
        .collect()
}

fn assert_five_lines_see_decide(text: &str) {
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 8, "header, five lines, See, Decide: {text}");
    assert!(lines[0].ends_with("Its last 5 lines:"), "{}", lines[0]);
    assert_eq!(
        lines[1..6],
        last_five_cut()[..],
        "the last five non-blank lines, cut at 160"
    );
    assert!(
        lines[6].starts_with("See: ") && lines[6].ends_with("open-repo-a1.pre-merge.log"),
        "{}",
        lines[6]
    );
    assert_eq!(
        lines[7],
        "Decide: cpd job retry open-repo --tier <T> --plan pay, or cpd job cancel open-repo --reason <text> --plan pay"
    );
}

#[test]
fn a_gate_failed_events_text_stays_within_1500_characters_whatever_the_tail() {
    let (text, _, _live) = gate_failure_text(Reply::exit(1, &long_gate_output(), ""));

    assert!(
        text.chars().count() <= 1500,
        "{} characters",
        text.chars().count()
    );
}

#[test]
fn a_gate_failed_events_text_ends_with_see_the_log_which_holds_the_whole_output_and_the_detail_keeps_the_tail()
 {
    let (text, detail, _live) = gate_failure_text(Reply::exit(1, &long_gate_output(), ""));

    let see = text
        .lines()
        .find(|l| l.starts_with("See: "))
        .unwrap_or_else(|| panic!("no See: line in {} characters", text.chars().count()));
    let path = see.trim_start_matches("See: ");
    assert!(path.ends_with("open-repo-a1.pre-merge.log"), "{path}");
    let log = fs::read_to_string(path).unwrap();
    for n in 1..=80 {
        assert!(
            log.contains(&format!("line {n:02} ")),
            "line {n:02} is in the log"
        );
    }
    for n in 22..=80 {
        assert!(
            detail.contains(&format!("line {n:02} ")),
            "line {n:02} stays in data.detail"
        );
    }
    assert!(
        !detail.contains("line 21 "),
        "the detail holds the 60-line tail only"
    );
    assert!(detail.contains("\n\n"), "the blank line stays in the tail");
    assert!(
        detail.contains(&format!("log: {path}")),
        "the same log path: {detail}"
    );
}

#[test]
fn a_gate_failed_events_text_carries_the_last_five_non_blank_lines_cut_at_160_characters_and_none_before_them()
 {
    let (text, _, _live) = gate_failure_text(Reply::exit(1, &long_gate_output(), ""));

    assert!(
        text.starts_with("Job open-repo failed (gate_failed): pre-merge failed (exit 1)."),
        "{}",
        text.lines().next().unwrap_or("")
    );
    assert_five_lines_see_decide(&text);
    for n in 1..=75 {
        assert!(
            !text.contains(&format!("line {n:02} ")),
            "line {n:02} is not in the text"
        );
    }
}

#[test]
fn a_gate_errors_events_text_is_short_the_same_way() {
    let (text, _, _live) = gate_failure_text(Reply::signal(9, &long_gate_output(), ""));

    assert!(
        text.chars().count() <= 1500,
        "{} characters",
        text.chars().count()
    );
    assert!(
        text.starts_with("Job open-repo failed (gate_error): pre-merge could not be judged ("),
        "{}",
        text.lines().next().unwrap_or("")
    );
    assert_five_lines_see_decide(&text);
}

/// A `gh` failure after the fast-forward ends the Attempt `failed` / `crashed` with the detail `gh_failed`: the push stands, the PR stays
/// `dispatch`, and the Job is retried at the same Tier (the next Attempt finds the pull request, or creates it).
#[test]
fn an_open_jobs_gh_failure_after_the_push_ends_the_attempt_crashed_with_gh_failed_and_the_pr_stays_dispatch()
 {
    let live = open_live();
    live.runner()
        .on("gh", |c| c.args[..2] == ["pr", "list"], Reply::ok("[]"))
        .on("gh", |_| true, Reply::exit(1, "", "gh: boom\n"));
    let tip = live.lead_sha();

    let (attempt, _) = live.run_job("open-repo", submit_only);

    assert_eq!(
        (&attempt["outcome"], &attempt["cause"]),
        (&json!("failed"), &json!("crashed")),
        "{attempt}"
    );
    let retry = live.event("open-repo-a1", "attempt-retry");
    assert!(
        retry["data"]["feedback"]
            .as_str()
            .unwrap()
            .contains("gh_failed"),
        "{retry}"
    );
    assert_eq!(
        live.job_status("open-repo"),
        "queued",
        "retried at the same Tier"
    );
    assert_eq!(
        live.remote("cpd/pay/repo"),
        tip,
        "the push happened before gh"
    );
    assert_eq!(
        (live.pr_row().0.as_str(), live.phase().as_str()),
        ("dispatch", "dispatch")
    );
    assert!(live.plan_events("pr-opened").is_empty());
}

/// The Attempt of an open Job ended `failed` / `crashed` (never left `merging`), its detail starting `prefix`, the Job retried at the same
/// Tier, and the Plan still holds the one `open` Job (pro-d375, pro-d379).
fn assert_open_attempt_crashed(live: &Live, attempt: &Value, prefix: &str) {
    assert_eq!(
        (&attempt["outcome"], &attempt["cause"]),
        (&json!("failed"), &json!("crashed")),
        "{attempt}"
    );
    assert_ne!(attempt["status"], json!("merging"), "{attempt}");
    let retry = live.event("open-repo-a1", "attempt-retry");
    let feedback = retry["data"]["feedback"].as_str().unwrap();
    assert!(
        feedback.starts_with(prefix)
            || feedback.contains(&format!("\n{prefix}"))
            || feedback.contains(&format!(": {prefix}")),
        "the detail starts {prefix}: {retry}"
    );
    assert_eq!(
        live.job_status("open-repo"),
        "queued",
        "retried at the same Tier"
    );
    let opens: i64 = live.value("SELECT count(*) FROM jobs WHERE plan = 'pay' AND kind = 'open'");
    assert_eq!(opens, 1, "no open-repo-2");
}

/// A `gh pr list` that fails at the adoption check (step 6b, the first `gh` call, after the Gates) ends the Attempt `failed` / `crashed` with
/// the detail `gh_failed:`, like step 8b: never left `merging` for R2 to merge (pro-d375).
#[test]
fn an_open_jobs_gh_failure_at_the_adoption_check_ends_the_attempt_crashed_with_gh_failed() {
    let live = open_live();
    live.runner().on(
        "gh",
        |_| true,
        Reply::exit(1, "", "GraphQL: API rate limit already exceeded\n"),
    );
    let before = live.remote("cpd/pay/repo");

    let (attempt, _) = live.run_job("open-repo", submit_only);

    assert_open_attempt_crashed(&live, &attempt, "gh_failed:");
    assert_eq!(live.remote("cpd/pay/repo"), before, "no push happened");
    assert_eq!(
        (live.pr_row().0.as_str(), live.phase().as_str()),
        ("dispatch", "dispatch")
    );
    assert_eq!(live.pr_row().1, None, "no pull request recorded");
    assert!(live.plan_events("pr-opened").is_empty());
}

/// A git failure in `base_check` (step 6: the fetch of the base) ends the Attempt `failed` / `crashed` with the detail `git_failed:` and does
/// not crash the driver, leaving it `merging` (pro-d379).
#[test]
fn an_open_jobs_base_check_git_failure_ends_the_attempt_crashed() {
    let live = open_live();
    live.gh_creates();
    let before = live.remote("cpd/pay/repo");

    let (attempt, _) = live.run_job("open-repo", |live, id| {
        let submitted = submit(live, id);
        live.runner().on(
            "git",
            |c| c.args.iter().any(|a| a == "fetch"),
            Reply::exit(128, "", "fatal: unable to access origin\n"),
        );
        submitted
    });

    assert_open_attempt_crashed(&live, &attempt, "git_failed:");
    assert_eq!(live.remote("cpd/pay/repo"), before, "no push happened");
    assert_eq!(live.pr_row().0, "dispatch");
}

/// The update Job on a conflict: the same path as the open Job without `gh`. Its first Attempt is accepted at the submit and fails at the
/// merge `merge_conflict` (the base check, before the Gates); the next Attempt's Worker merges the base, and cpd runs both Gates on it and
/// pushes the resolution in the merge; the pull request is already open, so nothing is created and no PR row changes.
#[test]
fn an_update_job_with_a_conflicting_base_fails_its_merge_until_the_next_attempt_merges_it_and_its_merge_pushes_the_resolution()
 {
    let live = open_pr_live();
    live.add_cpd_job("up-repo", "update");
    live.commit(&live.lead, "README.md", "mine\n", "mine");
    let tip = live.lead_sha();
    let base = live.advance_base("README.md", "theirs\n");
    let row = live.pr_row();
    let remote = live.remote("cpd/pay/repo");

    let (first, submitted) = live.run_job("up-repo", submit_only);

    assert_eq!(submitted["ok"], true, "{submitted}");
    assert_eq!(
        (&first["outcome"], &first["cause"]),
        (&json!("failed"), &json!("merge_conflict")),
        "{first}"
    );
    let retry = live.event("up-repo-a1", "attempt-retry");
    let detail = retry["data"]["feedback"].as_str().unwrap();
    assert!(
        detail.contains("README.md") && detail.contains(&base[..7]),
        "{retry}"
    );
    assert_eq!(
        live.job_status("up-repo"),
        "queued",
        "retried at the same Tier"
    );
    assert_eq!(
        live.gate_runs(),
        Vec::<String>::new(),
        "the base check is before the Gates"
    );
    assert_eq!(
        (live.lead_sha(), live.remote("cpd/pay/repo")),
        (tip.clone(), remote),
        "nothing merged, nothing pushed"
    );

    let (attempt, submitted) = live.run_job("up-repo", merge_the_base_and_submit);

    assert_eq!(submitted["ok"], true, "{submitted}");
    assert_eq!(
        (&attempt["outcome"], &attempt["cause"]),
        (&json!("merged"), &json!("merged")),
        "{attempt}"
    );
    let new = live.lead_sha();
    assert_eq!(
        live.fx.git(&live.repo, &["rev-parse", &format!("{new}~1")]),
        tip
    );
    assert_eq!(
        live.remote("cpd/pay/repo"),
        new,
        "its merge pushes the resolution"
    );
    assert_eq!(
        live.gate_runs(),
        [
            format!("make quick head={new}"),
            format!("make test head={new}")
        ],
        "both Gates ran, once, on the resolved head"
    );
    live.assert_no_slot_and_no_gate_worktree();
    assert!(
        live.gh_pr_calls().is_empty(),
        "the pull request exists: {:?}",
        live.gh_pr_calls()
    );
    assert_eq!(live.pr_row(), row, "an update Job writes no PR row");
    assert!(live.plan_events("pr-opened").is_empty());
    let merged: Vec<Value> = live
        .plan_events("job-merged")
        .into_iter()
        .map(|e| e.1)
        .filter(|d| d["kind"] == "update")
        .collect();
    assert_eq!(merged.len(), 1, "{merged:?}");
}

/// The newest pull request of the Lead Branch is already `merged` on GitHub and cpd never recorded it (§7.7 submit step 1, §7.4 step 8b): the `open`
/// Job adopts it. Its submit runs no Gate; its merge runs both Gates (step 6, before the adoption: #425 moved the adoption after them,
/// hs-d679) but pushes nothing and creates nothing, and step 10 writes the rows of `pr open` step 0: the PR
/// `merged`, `pr-merged` (no `pr-opened`), and the Plan `done` with the last PR.
#[test]
fn an_open_job_adopts_a_pull_request_merged_on_github_after_its_gates_with_no_push() {
    let live = open_live();
    let merged = common::pay_lookup()
        .replace("\"state\":\"OPEN\"", "\"state\":\"MERGED\"")
        .replace("\"mergedAt\":null", "\"mergedAt\":\"2026-10-20T12:00:00Z\"");
    live.runner()
        .on("gh", |c| c.args[..2] == ["pr", "list"], Reply::ok(&format!("[{{\"number\":812,\"url\":\"{PR_URL}\",\"state\":\"MERGED\",\"createdAt\":\"2026-10-28T10:00:00Z\"}}]")))
        .on("gh", |c| c.args[..2] == ["api", "graphql"], Reply::ok(&merged));
    let before = live.remote("cpd/pay/repo");

    let tip = live.lead_sha();

    let (attempt, submitted) = live.run_job("open-repo", |live, id| {
        let submitted = submit(live, id);
        assert_eq!(
            live.gate_runs(),
            Vec::<String>::new(),
            "the submit ran a Gate"
        );
        submitted
    });

    assert_eq!(
        (&attempt["outcome"], &attempt["cause"]),
        (&json!("merged"), &json!("merged")),
        "{attempt}"
    );
    assert_eq!(submitted["ok"], true, "{submitted}");
    assert_eq!(
        live.gate_runs(),
        [
            format!("make quick head={tip}"),
            format!("make test head={tip}")
        ],
        "both Gates ran at the merge, before the adoption"
    );
    live.assert_no_slot_and_no_gate_worktree();
    assert_eq!(live.remote("cpd/pay/repo"), before, "no push");
    assert!(live.gh_pr_calls().iter().all(|c| c[1] != "create"));
    assert_eq!(live.pr_row().0, "merged");
    assert_eq!(live.phase(), "done");
    assert!(live.plan_events("pr-opened").is_empty());
    assert_eq!(live.plan_events("pr-merged").len(), 1);
}

/// A pull request of the Lead Branch created 2026-10-22, before the PR started (2026-10-27), `MERGED` on GitHub, is not the PR's: the `open`
/// Job's submit and merge ignore it (the Gates run), create the new pull request, and the PR ends `open`, never `merged`.
#[test]
fn an_open_job_ignores_a_merged_pull_request_created_before_the_pr_started() {
    let live = open_live();
    live.runner()
        .on("gh", |c| c.args[..2] == ["pr", "create"], Reply::ok(PR_URL))
        .on("gh", |c| c.args[..2] == ["api", "graphql"], Reply::ok(&common::pay_lookup()))
        .on("gh", |c| c.args[..2] == ["pr", "list"], Reply::ok("[{\"number\":265,\"url\":\"https://github.com/acme/pay/pull/265\",\"state\":\"MERGED\",\"createdAt\":\"2026-10-22T08:00:00Z\"}]"));
    let tip = live.lead_sha();

    let (attempt, submitted) = live.run_job("open-repo", submit_only);

    assert_eq!(
        (&attempt["outcome"], &attempt["cause"]),
        (&json!("merged"), &json!("merged")),
        "{attempt}"
    );
    assert_eq!(submitted["ok"], true, "{submitted}");
    assert_eq!(
        live.gate_runs(),
        [
            format!("make quick head={tip}"),
            format!("make test head={tip}")
        ]
    );
    let created = live
        .gh_pr_calls()
        .into_iter()
        .filter(|c| c[1] == "create")
        .count();
    assert_eq!(created, 1, "{:?}", live.gh_pr_calls());
    let (status, number, _) = live.pr_row();
    assert_eq!((status.as_str(), number), ("open", Some(812)));
    assert!(live.plan_events("pr-merged").is_empty());
    assert_eq!(live.plan_events("pr-opened").len(), 1);
}

// ---------- the stall rule (§3.4 A4, hs-d195) ----------

/// The pid of the process a tool call of the Worker's pi runs as: `sleep 120`, a child of the pane's process.
const TOOL: &str = "sleep 120";

/// A tool call of the Worker's pi whose result is `result` (a JSONL line, or nothing): the session file of the Worker, ending in a
/// `bash` tool call, quiet for an hour of virtual time, and `TOOL` running under the pane's process. Returns the tool's pid.
fn start_tool_call(live: &Live, result: &str) -> Pid {
    let sid: String = live.value("SELECT id FROM sessions WHERE role = 'worker'");
    let dir = live.fx.env.home().join(".pi/agent/sessions/w");
    fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("2026_{sid}.jsonl"));
    let call = json!({"type": "message", "timestamp": "2026-10-14T21:00:05.000Z", "message": {"role": "assistant", "stopReason": "toolUse",
        "content": [{"type": "text", "text": "run it"}, {"type": "toolCall", "id": "call_1", "name": "bash", "arguments": {"command": TOOL}}]}});
    let mut text = format!(
        "{}\n{call}\n",
        json!({"type": "session", "timestamp": "2026-10-14T21:00:00.000Z"})
    );
    if !result.is_empty() {
        text += &format!("{result}\n");
    }
    fs::write(&file, text).unwrap();
    // Quiet for an hour: the file's time is an hour before the virtual now, which is where the driver reads the quiet from.
    let hour_ago =
        std::time::SystemTime::from(live.fx.fakes.clock.utc()) - Duration::from_secs(3600);
    fs::OpenOptions::new()
        .write(true)
        .open(&file)
        .unwrap()
        .set_modified(hour_ago)
        .unwrap();
    let pane = live
        .fx
        .tmux(&["list-panes", "-a", "-F", "#{window_name} #{pane_pid}"]);
    let pane = String::from_utf8(pane.stdout).unwrap();
    let pid: u32 = pane
        .lines()
        .filter(|l| !l.starts_with("watch ") && !l.starts_with("main "))
        .find_map(|l| l.split(' ').nth(1)?.parse().ok())
        .expect("the Worker's pane");
    live.fx.fakes.spawn.add_process(Pid(pid), TOOL)
}

/// The tool call of `start_tool_call` is not killed over `polls` polls of the driver after the one that found the Worker's session
/// file, and the Attempt is stopped then.
fn assert_not_killed(live: &Live, result: &str, polls: usize) {
    let mut tool = None;
    let (id, _) = live.drive("api", |live, _, nap| {
        if nap == 1 {
            tool = Some(start_tool_call(live, result));
        }
        // The driver has recorded the file at the first poll; from then on each poll applies the rule.
        assert!(live.fx.fakes.spawn.alive(tool.unwrap()), "the tool call was killed at poll {nap}");
        assert_eq!(live.fx.fakes.spawn.signals(), []);
        if nap == polls {
            live.fixture("UPDATE attempts SET stop_request = 'timed_out' WHERE plan = 'pay' AND id = 'api-a1';");
        }
    });
    assert_eq!(live.attempt(&id)["cause"], "timed_out");
}

#[test]
fn a_stalled_tool_call_has_its_processes_killed_and_the_worker_keeps_running() {
    let live = live("[timeouts]\nstall_seconds = 2");
    let mut tool = None;
    let mut seen = Vec::new();

    let (id, _) = live.drive("api", |live, _, nap| match nap {
        1 => tool = Some(start_tool_call(live, "")),
        // The tool call is dead and pi lives on: its pane is there, and the Attempt keeps running, through the polls after the kill.
        2..=5 => {
            let tool = tool.unwrap();
            assert!(!live.fx.fakes.spawn.alive(tool), "the tool call is still alive");
            let status: String = live.value("SELECT status FROM attempts WHERE id = 'api-a1'");
            let panes = live.fx.tmux(&["list-panes", "-a", "-F", "#{pane_dead}"]);
            seen.push((status, String::from_utf8(panes.stdout).unwrap()));
            if nap == 5 {
                live.fixture("UPDATE attempts SET stop_request = 'timed_out' WHERE plan = 'pay' AND id = 'api-a1';");
            }
        }
        _ => {}
    });

    let tool = tool.unwrap();
    // The tool call got one SIGTERM, which it did not outlive, and no more however many polls came after: pi itself was never signalled.
    assert_eq!(live.fx.fakes.spawn.signals(), [(tool, Signal::Term)]);
    assert_eq!(seen.len(), 4);
    for (status, panes) in seen {
        assert_eq!(status, "running");
        assert!(panes.lines().all(|l| l == "0"), "{panes:?}");
    }
    // The line the driver logs (the pid, its command line, the idle time, once) is `Watch::check_with`'s, asserted on the fakes in
    // `driver::ports_tests::a_tool_call_quiet_for_the_limit_of_virtual_time_has_its_descendants_terminated`: the driver logs to stderr.
    assert_eq!(live.attempt(&id)["cause"], "timed_out");
    assert!(
        !live.events(&id).iter().any(|e| e["kind"] == "error"),
        "{:?}",
        live.events(&id)
    );
}

#[test]
fn a_tool_call_with_its_result_is_not_a_stall() {
    let live = live("[timeouts]\nstall_seconds = 1");
    // The file is quiet past stall_seconds, but the call has its result.
    assert_not_killed(
        &live,
        r#"{"type":"message","message":{"role":"toolResult","toolCallId":"call_1","content":[{"type":"text","text":"done"}],"isError":false}}"#,
        6,
    );
}

#[test]
fn stall_seconds_0_kills_nothing() {
    let live = live("[timeouts]\nstall_seconds = 0");
    // The file is quiet for an hour, inside a tool call: only stall_seconds = 0 keeps the call alive.
    assert_not_killed(&live, "", 6);
}

// ---------- the Gate retry (contract §3.4 `A Gate retry`, pro-d280) ----------

const JOB_KEY: &str = "open-repo";

/// An open Job whose Gate exits 1 with `output`, with `config_extra` in the config.
fn failing_gate(config_extra: &str, output: &str) -> Live {
    let live = open_live_with(config_extra);
    live.gh_creates();
    live.hooks("[pre-merge]\ngate = \"make gate\"\n");
    *live.hooks.reply.lock().unwrap() = Reply::exit(1, output, "");
    live
}

/// One Attempt of `open-repo`, whose Worker submits at once: the Attempt id.
fn attempt_once(live: &Live) -> String {
    live.drive(JOB_KEY, |live, id, nap| {
        if nap == 1 {
            submit_only(live, id);
        }
    })
    .0
}

fn attempt_count(live: &Live) -> i64 {
    live.value("SELECT count(*) FROM attempts WHERE plan = 'pay' AND job = 'open-repo'")
}

fn text_of(live: &Live, id: &str, kind: &str) -> String {
    live.event(id, kind)["text"].as_str().unwrap().to_owned()
}

fn assert_gate_failed(live: &Live, id: &str) {
    let attempt = live.attempt(id);
    assert_eq!(
        (&attempt["outcome"], &attempt["cause"]),
        (&json!("failed"), &json!("gate_failed")),
        "{attempt}"
    );
}

/// After Attempt 1's Gate failure the Job is queued again (a clear message before anything else is read).
fn assert_queued(live: &Live, after: &str) {
    assert_eq!(
        live.job_status(JOB_KEY),
        "queued",
        "a gate_failed Attempt ({after}) is retried: the Job is queued again, not failed"
    );
}

#[test]
fn a_gate_failed_attempt_is_retried_once_at_the_same_tier_with_no_lead_turn_between() {
    let live = failing_gate("", "the gate says no\n");

    let a1 = attempt_once(&live);

    assert_gate_failed(&live, &a1);
    assert_queued(&live, &a1);
    let a2 = attempt_once(&live);
    assert_eq!(a2, "open-repo-a2");
    let (t1, t2): (String, String) = (
        live.value("SELECT tier FROM attempts WHERE plan = 'pay' AND id = 'open-repo-a1'"),
        live.value("SELECT tier FROM attempts WHERE plan = 'pay' AND id = 'open-repo-a2'"),
    );
    assert_eq!(t2, t1, "the same Tier");
    let origin: String =
        live.value("SELECT origin FROM attempts WHERE plan = 'pay' AND id = 'open-repo-a2'");
    assert_eq!(origin, "ladder");
    assert!(
        live.plan_events("job-retried").is_empty(),
        "no `job retry` happened"
    );
    let retries = live.plan_events("attempt-retry");
    assert!(!retries.is_empty(), "an attempt-retry Event is the trace");
    let first = live.event(&a1, "attempt-retry");
    assert_eq!(first["data"]["cause"], "gate_failed", "{first}");
    assert_eq!(first["data"]["next_tier"], t1.as_str(), "{first}");
    assert!(
        live.events(&a1).iter().all(|e| e["kind"] != "job-failed"),
        "no job-failed for Attempt 1"
    );
}

#[test]
fn a_second_gate_failure_fails_the_job_and_sends_the_one_job_failed_event() {
    let live = failing_gate("", "the gate says no\n");

    let a1 = attempt_once(&live);
    assert_queued(&live, &a1);
    let a2 = attempt_once(&live);

    assert_gate_failed(&live, &a2);
    assert_eq!(live.job_status(JOB_KEY), "failed");
    let cause: Option<String> =
        live.value("SELECT failed_cause FROM jobs WHERE plan = 'pay' AND key = 'open-repo'");
    assert_eq!(cause.as_deref(), Some("gate_failed"));
    let failed = live.plan_events("job-failed");
    assert_eq!(failed.len(), 1, "{failed:?}");
    assert_eq!(failed[0].1["attempts"], 2, "{}", failed[0].1);
    let text = text_of(&live, &a2, "job-failed");
    assert!(
        text.starts_with("Job open-repo failed (gate_failed): pre-merge failed (exit 1)."),
        "{text}"
    );
    let see = text
        .lines()
        .find(|l| l.starts_with("See: "))
        .unwrap_or_else(|| panic!("no See: line in {text}"));
    assert!(see.ends_with("open-repo-a2.pre-merge.log"), "{see}");
    assert_eq!(live.plan_events("attempt-retry").len(), 1);
    assert_eq!(attempt_count(&live), 2);
}

#[test]
fn a_gate_retry_leaves_the_failed_attempt_and_an_attempt_retry_event_with_the_gate_body() {
    let live = failing_gate("", &long_gate_output());

    let a1 = attempt_once(&live);

    assert_queued(&live, &a1);
    assert_gate_failed(&live, &a1);
    let event = live.event(&a1, "attempt-retry");
    let text = event["text"].as_str().unwrap();
    assert!(
        text.chars().count() <= 1500,
        "{} characters",
        text.chars().count()
    );
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 8, "{text}");
    assert!(
        lines[0].starts_with(
            "Attempt open-repo-a1 of Job open-repo failed (gate_failed): pre-merge failed (exit 1)."
        ),
        "{}",
        lines[0]
    );
    assert!(lines[0].ends_with("Its last 5 lines:"), "{}", lines[0]);
    assert_eq!(lines[1..6], last_five_cut()[..]);
    assert!(
        lines[6].starts_with("See: ") && lines[6].ends_with("open-repo-a1.pre-merge.log"),
        "{}",
        lines[6]
    );
    assert!(
        lines[7].starts_with("The Job is queued again at Tier S2."),
        "{}",
        lines[7]
    );
    let path = lines[6].trim_start_matches("See: ");
    let feedback = event["data"]["feedback"].as_str().unwrap();
    assert!(feedback.contains("pre-merge failed (exit 1)"), "{feedback}");
    assert!(feedback.contains(&format!("log: {path}")), "{feedback}");
}

#[test]
fn the_retry_attempts_spec_names_the_failed_gate_and_its_log() {
    let live = failing_gate("", "the gate says no\n");

    let a1 = attempt_once(&live);

    assert_queued(&live, &a1);
    let text = text_of(&live, &a1, "attempt-retry");
    let log = text
        .lines()
        .find_map(|l| l.strip_prefix("See: "))
        .unwrap_or_else(|| panic!("no See: line in {text}"))
        .to_owned();
    attempt_once(&live);
    let spec_path: String =
        live.value("SELECT spec_path FROM attempts WHERE plan = 'pay' AND id = 'open-repo-a2'");
    let spec = fs::read_to_string(&spec_path).unwrap();
    assert!(spec.contains("- Cause: gate_failed\n"), "{spec}");
    assert!(
        spec.contains(&format!("\n  - `{log}`\n")),
        "the Spec does not list the log {log} as an item:\n{spec}"
    );
}

#[test]
fn a_job_retry_opens_a_new_window_for_one_more_gate_retry() {
    let live = failing_gate("", "the gate says no\n");
    let a1 = attempt_once(&live);
    assert_queued(&live, &a1);
    attempt_once(&live);
    assert_eq!(live.job_status(JOB_KEY), "failed");

    live.fx
        .cpd(&["job", "retry", JOB_KEY, "--tier", "S2", "--plan", PLAN])
        .ok();
    let a3 = attempt_once(&live);

    assert_eq!(a3, "open-repo-a3");
    let origin: String =
        live.value("SELECT origin FROM attempts WHERE plan = 'pay' AND id = 'open-repo-a3'");
    assert_eq!(origin, "retry");
    assert_gate_failed(&live, &a3);
    assert_queued(&live, &a3);
    let a4 = attempt_once(&live);
    assert_eq!(a4, "open-repo-a4");
    let (t3, t4): (String, String) = (
        live.value("SELECT tier FROM attempts WHERE plan = 'pay' AND id = 'open-repo-a3'"),
        live.value("SELECT tier FROM attempts WHERE plan = 'pay' AND id = 'open-repo-a4'"),
    );
    assert_eq!(t4, t3);
    assert_eq!(live.job_status(JOB_KEY), "failed");
    assert_eq!(live.plan_events("job-failed").len(), 2);
}

/// Guard: `retry_cap = 0` means no gate retry.
#[test]
fn retry_cap_zero_leaves_a_gate_failure_to_the_lead() {
    let live = failing_gate("retry_cap = 0", "the gate says no\n");

    let a1 = attempt_once(&live);

    assert_gate_failed(&live, &a1);
    assert_eq!(live.job_status(JOB_KEY), "failed");
    assert_eq!(live.plan_events("job-failed").len(), 1);
    assert!(live.plan_events("attempt-retry").is_empty());
}

/// Guard: with `retry_cap = 1` a crash and a Gate failure are 2 automatic Attempts, `1 + retry_cap`: the Job fails at once.
#[test]
fn a_gate_failure_after_a_spent_retry_cap_fails_the_job_at_once() {
    let live = failing_gate("retry_cap = 1", "the gate says no\n");
    let (crashed, _) = live.drive(JOB_KEY, |live, id, nap| {
        if nap == 1 {
            let window = format!("{PLAN}--{id}");
            let pane = live
                .fx
                .panes()
                .into_iter()
                .find(|p| p.window_name == window)
                .expect("the Worker's pane");
            assert!(
                live.fx
                    .tmux(&["__ctl", "pane-exit", &pane.pane, "--status", "4"])
                    .status
                    .success()
            );
        }
    });
    assert_eq!(live.attempt(&crashed)["cause"], "crashed");
    assert_eq!(live.job_status(JOB_KEY), "queued");

    let a2 = attempt_once(&live);

    assert_gate_failed(&live, &a2);
    assert_eq!(live.job_status(JOB_KEY), "failed");
    assert_eq!(live.plan_events("job-failed").len(), 1);
}

const OPEN_ESCALATION: &str = "INSERT INTO escalations (plan, id, attempt, number, question, status, asker_pid, asked_at) VALUES ('pay', 'api-a1-q1', 'api-a1', 1, 'Why?', 'open', 4242, 't');";
const CLOSE_ESCALATION: &str = "UPDATE escalations SET status = 'answered', answer = 'ok', answered_at = 't' WHERE plan = 'pay' AND id = 'api-a1-q1';";
const STOP: &str =
    "UPDATE attempts SET stop_request = 'timed_out' WHERE plan = 'pay' AND id = 'api-a1';";

#[test]
fn an_open_escalation_of_the_attempt_is_not_a_stall() {
    let live = live("[timeouts]\nstall_seconds = 1");
    let mut tool = None;
    let (id, _) = live.drive("api", |live, _, nap| {
        if nap == 1 {
            tool = Some(start_tool_call(live, ""));
            live.fixture(OPEN_ESCALATION);
        }
        assert!(
            live.fx.fakes.spawn.alive(tool.unwrap()),
            "the tool call was killed at poll {nap} with an open Escalation"
        );
        assert_eq!(live.fx.fakes.spawn.signals(), []);
        if nap == 6 {
            live.fixture(STOP);
        }
    });
    assert_eq!(live.attempt(&id)["cause"], "timed_out");
}

#[test]
fn a_stall_is_judged_again_once_the_escalation_closes() {
    let live = live("[timeouts]\nstall_seconds = 2");
    let mut tool = None;
    let (id, _) = live.drive("api", |live, _, nap| {
        match nap {
            1 => {
                tool = Some(start_tool_call(live, ""));
                live.fixture(OPEN_ESCALATION);
            }
            7 => live.fixture(CLOSE_ESCALATION),
            _ => {}
        }
        let tool = tool.unwrap();
        match nap {
            1..=8 => {
                assert!(
                    live.fx.fakes.spawn.alive(tool),
                    "the tool call was killed at poll {nap}"
                );
                assert_eq!(live.fx.fakes.spawn.signals(), [], "signalled at poll {nap}");
            }
            9..=14 if nap == 14 => {
                assert!(
                    !live.fx.fakes.spawn.alive(tool),
                    "no stall judged within 6 polls after the close"
                );
                live.fixture(STOP);
            }
            _ => {}
        }
    });
    assert_eq!(live.fx.fakes.spawn.signals().len(), 1);
    assert_eq!(live.attempt(&id)["cause"], "timed_out");
}
