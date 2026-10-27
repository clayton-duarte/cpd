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

use super::driver::{JOB, PLAN, PR_TITLE, PR_URL, add_job_in, fixture, seed_home, seed_rows, store};

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
        let head = self.runner.output(&mut head, "git", Duration::from_secs(5)).expect("git rev-parse");
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
    fx.git(fx.env.root(), &["init", "--bare", "-q", "-b", "main", origin.to_str().unwrap()]);
    fx.local_origin(&repo, &origin);
    // The Lead worktree as `pr start` makes it, and both branches pushed (R35).
    let lead = repo.parent().unwrap().join("pay.pay");
    fx.git(&repo, &["worktree", "add", "-q", "-b", "cpd/pay/repo", lead.to_str().unwrap(), "main"]);
    fx.git(&repo, &["push", "-q", "origin", "main", "cpd/pay/repo"]);
    seed_home(&fx.env, config_extra);
    // The work session as the engine leaves it: its `watch` window, and remain-on-exit (§6.1).
    for args in [&["new-session", "-d", "-s", "cpd-work", "-n", "watch", "sleep", "600"][..], &["set", "-g", "remain-on-exit", "on"]] {
        assert!(fx.tmux(args).status.success(), "tmux {args:?}");
    }
    seed_rows(&fx.env, &repo, &lead);
    add_job_in(&fx.env, "repo", JOB, &[]);
    // The test binary stands for cpd's own executable: a launched driver is run by the test, never started.
    fx.fakes.spawn.set_self_exe(std::env::current_exe().unwrap());
    let hooks = Arc::new(Hooks { runner: fx.fakes.runner.weak(), runs: Mutex::new(Vec::new()), reply: Mutex::new(Reply::ok("")) });
    let _ = (*fx.fakes.runner).clone().route("sh", hooks.clone());
    Live { fx, repo, lead, origin, hooks }
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
        store(&self.fx.env).read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?)).unwrap()
    }

    pub(super) fn job_status(&self, key: &str) -> String {
        self.value(&format!("SELECT status FROM jobs WHERE plan = 'pay' AND key = '{key}'"))
    }

    pub(super) fn phase(&self) -> String {
        self.value("SELECT phase FROM plans WHERE id = 'pay'")
    }

    /// The worktree Attempt `id` runs in, as the starter recorded it: the PR's Lead worktree for a linear Attempt (the only
    /// live Attempt of its PR, hs-d680), its own `pay.pay--<id>` for a parallel one. Before the Attempt exists, the parallel path.
    pub(super) fn worktree(&self, id: &str) -> PathBuf {
        let stored: Option<String> =
            store(&self.fx.env).read(|c| Ok(c.query_row("SELECT worktree FROM attempts WHERE plan = 'pay' AND id = ?1", [id], |r| r.get(0)).ok())).unwrap();
        stored.map_or_else(|| self.repo.parent().unwrap().join(format!("pay.pay--{id}")), PathBuf::from)
    }

    /// [`Live::worktree`] as a hook sees it as its cwd (the hook runner canonicalizes it): the Attempt's worktree is gone once it ends.
    fn canonical_worktree(&self, id: &str) -> PathBuf {
        self.worktree(id).canonicalize().unwrap_or_else(|_| self.repo.canonicalize().unwrap().parent().unwrap().join(format!("pay.pay--{id}")))
    }

    pub(super) fn lead_sha(&self) -> String {
        self.fx.git(&self.repo, &["rev-parse", "refs/heads/cpd/pay/repo"])
    }

    pub(super) fn remote(&self, branch: &str) -> String {
        self.fx.git(&self.origin, &["rev-parse", &format!("refs/heads/{branch}")])
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
        self.fx.git(&self.lead, &["commit", "-q", "--allow-empty", "-m", "hooks"]);
    }

    /// What the Gates noted: `<command> head=<sha>`, one per run of a hook, in order.
    pub(super) fn gate_runs(&self) -> Vec<String> {
        self.hooks.runs.lock().unwrap().iter().map(|r| format!("{} head={}", r.command, r.head)).collect()
    }

    /// The directory each run of a hook was in, in order.
    fn gate_dirs(&self) -> Vec<PathBuf> {
        self.hooks.runs.lock().unwrap().iter().map(|r| r.cwd.clone()).collect()
    }

    /// There is no test slot (#425): no hook was given one, and no slot lock file exists in `$CPD_HOME/run`, and no gate worktree is left
    /// beside the repo.
    fn assert_no_slot_and_no_gate_worktree(&self) {
        assert!(self.hooks.runs.lock().unwrap().iter().all(|r| !r.slotted), "a hook was given a test slot");
        let names = |dir: &Path| -> Vec<String> {
            fs::read_dir(dir).map(|d| d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect()).unwrap_or_default()
        };
        let slots: Vec<String> = names(&self.fx.env.cpd_home().join("run")).into_iter().filter(|n| n.starts_with("test-")).collect();
        assert!(slots.is_empty(), "{slots:?}");
        let gates: Vec<String> = names(self.repo.parent().unwrap()).into_iter().filter(|n| n.contains(".gate-")).collect();
        assert!(gates.is_empty(), "{gates:?}");
    }

    /// The fake `gh`: no pull request of the branch yet, `pr create` answers [`PR_URL`], and the lookup of the pull request answers it.
    pub(super) fn gh_creates(&self) {
        self.runner()
            .on("gh", |c| c.args[..2] == ["pr", "list"], Reply::ok("[]"))
            .on("gh", |c| c.args[..2] == ["pr", "create"], Reply::ok(PR_URL))
            .on("gh", |c| c.args[..2] == ["api", "graphql"], Reply::ok(&common::pay_lookup()));
    }

    /// `gh`'s calls that are not the poll's or a lookup's: `pr list`, `pr create`.
    pub(super) fn gh_pr_calls(&self) -> Vec<Vec<String>> {
        self.fx.calls("gh").into_iter().map(|c| c.args).filter(|a| a[0] == "pr").collect()
    }

    /// The Events of `kind` of Plan `pay`: `(txn, data)`, in order.
    pub(super) fn plan_events(&self, kind: &str) -> Vec<(i64, Value)> {
        store(&self.fx.env)
            .read(|c| {
                let mut stmt = c.prepare("SELECT txn, data FROM events WHERE plan = 'pay' AND kind = ?1 ORDER BY id")?;
                let rows = stmt
                    .query_map([kind], |r| Ok((r.get::<_, i64>(0)?, serde_json::from_str(&r.get::<_, String>(1)?).unwrap())))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap()
    }

    /// `pr` of the open PR or the PR that is `dispatch`: `(status, pr_number, pr_opened_at)`.
    pub(super) fn pr_row(&self) -> (String, Option<i64>, Option<String>) {
        store(&self.fx.env)
            .read(|c| Ok(c.query_row("SELECT status, pr_number, pr_opened_at FROM prs WHERE plan = 'pay' AND key = 'repo'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?))
            .unwrap()
    }

    pub(super) fn attempt(&self, id: &str) -> Value {
        self.fx.cpd(&["attempt", "show", id, "--plan", PLAN]).ok()["attempt"].clone()
    }

    /// The Events about Attempt `id`, in order.
    pub(super) fn events(&self, id: &str) -> Vec<Value> {
        let all = self.fx.cpd(&["events", "list", "--plan", PLAN]).ok()["events"].as_array().unwrap().clone();
        all.into_iter().filter(|e| e["attempt"] == id).collect()
    }

    fn event(&self, id: &str, kind: &str) -> Value {
        self.events(id).into_iter().find(|e| e["kind"] == kind).unwrap_or_else(|| panic!("no {kind} for {id}"))
    }

    /// Job `key` of PR `repo` as cpd adds an `open` or `update` Job: `appended`, no Dependencies, `queued` with its fingerprint.
    pub(super) fn add_cpd_job(&self, key: &str, kind: &str) {
        self.fixture(&format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, created_txn, created_at)
             VALUES ('pay', 'repo', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 1, '{kind}', cpd_txn(), 't');"
        ));
        let plan = PlanId::parse(PLAN).unwrap();
        let all = store(&self.fx.env).read(|conn| cpd_core::plan::jobs::load(conn, &plan)).unwrap();
        let job = all.iter().find(|j| j.key == key).unwrap();
        self.fixture(&format!(
            "UPDATE jobs SET status = 'queued', fingerprint = '{}' WHERE plan = 'pay' AND key = '{key}';",
            cpd_core::plan::locked_fingerprint(job)
        ));
    }

    /// Dispatches Job `key` and runs its driver on this thread: `hook(self, id, n)` is called after the driver's `n`th nap (from 1),
    /// where the Worker's pi acts. Returns the id of the Attempt and what the driver printed.
    pub(super) fn drive(&self, key: &str, hook: impl FnMut(&Live, &str, usize)) -> (String, common::Run) {
        self.drive_as(key, false, hook)
    }

    /// [`Live::drive`] for an Attempt that is PARALLEL (hs-d683: its PR has a live sibling): `job dispatch` always asks for the linear
    /// one, so the Attempt it records, still `starting`, is given its own `cpd/<plan>--<attempt>` branch and worktree before its
    /// driver runs, as the starter records a parallel one. The driver then creates the worktree and runs `pre-start` (A2), rebases
    /// and fast-forwards with the compare-and-swap (§7.4) and removes the worktree at its end (A9).
    pub(super) fn drive_parallel(&self, key: &str, hook: impl FnMut(&Live, &str, usize)) -> (String, common::Run) {
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

    fn drive_as(&self, key: &str, parallel: bool, mut hook: impl FnMut(&Live, &str, usize)) -> (String, common::Run) {
        let out = self.dispatch(key).ok();
        let id = out["attempt"].as_str().unwrap_or_else(|| panic!("no attempt in {out}")).to_owned();
        if parallel {
            self.make_parallel(&id);
        }
        let launch = self.fx.take_launch(self.fx.fakes.spawn.launches().len() - 1);
        let home = CpdHome::resolve(Some(self.fx.env.cpd_home().into()), None).unwrap();
        let lock = home.attempt_lock(&PlanId::parse(PLAN).unwrap(), &AttemptId::parse(&id).unwrap());
        let _fd3 = self.fx.hand_over(fs::File::open(lock).unwrap());
        let ran = self.fx.run_launch_with(&launch, |_, nap| hook(self, &id, nap));
        assert_eq!(ran.exit, 0, "{}", ran.stdout);
        (id, ran)
    }

    /// [`Live::drive`] with a Worker that acts once, in the driver's first poll: returns the Attempt it ended and what `worker`
    /// returned. `worker` gets the id of the Attempt.
    pub(super) fn run_job<W>(&self, key: &str, worker: impl FnOnce(&Live, &str) -> W) -> (Value, W) {
        let (mut worker, mut did) = (Some(worker), None);
        let (id, _) = self.drive(key, |live, id, nap| {
            if nap == 1 {
                did = Some((worker.take().expect("the first nap is once"))(live, id));
            }
        });
        (self.attempt(&id), did.expect("the driver napped: the Worker ran"))
    }

    /// The Worker's pi ends without submitting again: its window is gone from the fake server.
    pub(super) fn end_the_worker(&self) {
        for pane in self.fx.panes().iter().filter(|p| p.window_name != "main") {
            assert!(self.fx.tmux(&["kill-window", "-t", &pane.window]).status.success());
        }
    }

    /// `job dispatch <key>`: the driver's `Launch` is recorded, nothing is started.
    pub(super) fn dispatch(&self, key: &str) -> common::Run {
        self.fx.cpd(&["job", "dispatch", key, "--plan", PLAN])
    }
}

/// What `attempt submit` answered the Worker.
pub(super) fn submit(live: &Live, id: &str) -> Value {
    // The environment of the Worker's window: its Plan and its Attempt.
    let r = live.fx.cpd_with(&live.worktree(id), &["attempt", "submit", id], &[("CPD_PLAN", PLAN), ("CPD_ATTEMPT", id)]).json();
    r
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
    live.fx.git(&wt, &["merge", "-q", "--no-edit", "-X", "ours", "origin/main"]);
    submit(live, id)
}

/// PR `repo` as `pr start` leaves it, with a bare `origin` that holds `main` and the Lead Branch as the lock pushed it, the policy
/// both Gates `local` (`[gate] merge` and `pr`, contract §8.3) with a `pre-commit` and a `pre-merge` hook that note the commit they run on, Job `api` merged (its commit `api.txt` is on
/// the Lead Branch, **not pushed**), and the PR's `open` Job `open-repo` queued. `gh` is the test's to script.
pub(super) fn open_live() -> Live {
    let live = live("");
    live.fx.env.owner_policy("acme", "[gate]\nmerge = \"local\"\npr = \"local\"\n");
    let merged = live.commit(&live.lead, "api.txt", "api\n", "api");
    live.fixture(&format!(
        "UPDATE jobs SET status = 'running' WHERE key = 'api'; UPDATE jobs SET status = 'merged', merged_sha = '{merged}' WHERE key = 'api';"
    ));
    live.hooks("[pre-commit]\ngate = \"make quick\"\n\n[pre-merge]\ngate = \"make test\"\n");
    live.add_cpd_job("open-repo", "open");
    live.fixture("UPDATE prs SET started_at = '2026-10-27T09:00:00.000Z' WHERE key = 'repo';");
    live
}

/// The same with the pull request already open (812, as `pr open` leaves it), the Plan in `deploy`, and no `open` Job.
pub(super) fn open_pr_live() -> Live {
    let live = open_live();
    live.fixture(&format!(
        "UPDATE jobs SET status = 'cancelled', cancel_reason = 'opened', cancel_requested_at = 't' WHERE key = 'open-repo';
         INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, mergeable, merge_state, created_at, merged_at, changed_at)
           VALUES ('acme/pay', 812, '{PR_URL}', '{PR_TITLE}', 0, 'open', 'none', 'none', 'mergeable', 'clean', 'opened', NULL, 'opened');
         UPDATE prs SET status = 'open', pr_number = 812, pr_url = '{PR_URL}', pr_opened_at = 'opened', pull_request = (SELECT id FROM pull_requests) WHERE key = 'repo';
         UPDATE plans SET phase = 'deploy' WHERE id = 'pay';"
    ));
    live
}

/// A Worker that did nothing, with the base unmoved or moved without a conflict, submits the Lead Branch as it is: its submit runs no
/// Gate, and its merge merges nothing (the Lead Branch does not move), runs both Gates (`gate.merge`, then `gate.pr`) in the Attempt
/// worktree at its HEAD, which is the Lead tip, with no test slot, pushes it, and runs `gh pr create` (never with `--draft`), writing
/// `pr open`'s rows and Events in the transaction that ends the Job `merged` (contract §7.4 step 6, hs-d679).
#[test]
fn an_open_job_with_no_conflict_merges_nothing_runs_both_gates_at_its_merge_pushes_and_opens_the_pull_request() {
    let live = open_live();
    live.gh_creates();
    let tip = live.lead_sha();
    // The base moved, without a conflict: that is no reason to merge it.
    let base = live.advance_base("base.txt", "base\n");
    assert_ne!(live.remote("cpd/pay/repo"), tip, "the Lead's commit is not pushed yet");

    // The submit runs no Gate: the Worker's closure ends right after it, before the driver merges.
    let (attempt, submitted) = live.run_job("open-repo", |live, id| {
        let submitted = submit(live, id);
        assert_eq!(live.gate_runs(), Vec::<String>::new(), "the submit ran a Gate");
        submitted
    });

    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("merged"), &json!("merged")), "{attempt}");
    assert_eq!(submitted["ok"], true, "{submitted}");
    assert_eq!(live.job_status("open-repo"), "merged");
    // Nothing merged: the Lead Branch is where it was, and does not hold the base.
    assert_eq!(live.lead_sha(), tip);
    assert_eq!(attempt["merged_sha"], tip);
    assert!(!live.lead.join("base.txt").exists());
    assert_ne!(live.fx.git(&live.repo, &["merge-base", &tip, &base]), base, "the base is not an ancestor of the Lead Branch");
    // Both Gates ran at the merge, once each and in this order, at the Lead tip, in the Attempt's own worktree (not a gate worktree, no slot).
    assert_eq!(live.gate_runs(), [format!("make quick head={tip}"), format!("make test head={tip}")]);
    let worktree = live.canonical_worktree("open-repo-a1");
    assert_eq!(live.gate_dirs(), [worktree.clone(), worktree]);
    live.assert_no_slot_and_no_gate_worktree();
    let logs = live.fx.env.cpd_home().join("run/pay/logs");
    let log = fs::read_to_string(logs.join("open-repo-a1.pre-merge.log")).unwrap();
    assert!(!log.contains("slot"), "{log}");
    assert!(logs.join("open-repo-a1.pre-commit.log").exists(), "the gate.merge log is the Attempt's");
    // Pushed, and the pull request created: ready, never a draft.
    assert_eq!(live.remote("cpd/pay/repo"), tip);
    let calls = live.gh_pr_calls();
    assert_eq!(calls[0], ["pr", "list", "--head", "cpd/pay/repo", "--state", "all", "--json", "number,url,state,createdAt"]);
    let created: Vec<&Vec<String>> = calls.iter().filter(|c| c[1] == "create").collect();
    assert_eq!(created.len(), 1, "{calls:?}");
    assert_eq!(created[0][..8], ["pr", "create", "--base", "main", "--head", "cpd/pay/repo", "--title", PR_TITLE]);
    assert!(live.fx.calls("gh").iter().all(|c| !c.args.iter().any(|a| a == "--draft" || a == "ready" || a == "convert-to-draft")), "never a draft");
    // `pr open`'s rows and Events, with `job-merged`, in one transaction.
    let (status, number, opened_at) = live.pr_row();
    assert_eq!((status.as_str(), number, opened_at.is_some()), ("open", Some(812), true));
    assert_eq!(live.phase(), "deploy");
    assert_eq!(live.value::<String>("SELECT pr_url FROM prs WHERE plan = 'pay'"), PR_URL);
    assert_eq!(live.value::<i64>("SELECT count(*) FROM pull_requests WHERE repo = 'acme/pay' AND number = 812 AND state = 'open' AND draft = 0"), 1);
    assert!(live.value::<Option<i64>>("SELECT pull_request FROM prs WHERE plan = 'pay'").is_some());
    let (merged, opened, phase) = (live.plan_events("job-merged"), live.plan_events("pr-opened"), live.plan_events("plan-phase"));
    let ours: Vec<&(i64, Value)> = merged.iter().filter(|e| e.1["sha"] == tip.as_str() && e.1["kind"] == "open").collect();
    assert_eq!(ours.len(), 1, "{merged:?}");
    assert_eq!((&ours[0].1["pr"], &ours[0].1["number"], &ours[0].1["url"]), (&json!("repo"), &json!(812), &json!(PR_URL)));
    assert_eq!(opened.iter().map(|e| e.1.clone()).collect::<Vec<_>>(), [json!({"pr": "repo", "number": 812, "url": PR_URL})]);
    assert_eq!((phase.len(), &phase[0].1["from"], &phase[0].1["to"]), (1, &json!("dispatch"), &json!("deploy")));
    assert_eq!((opened[0].0, phase[0].0), (ours[0].0, ours[0].0), "one transaction");
}

/// An `open` Job's checks (base check, Gate) are one `checks` span, closed in the transaction that makes the Attempt `merging`.
#[test]
fn an_open_jobs_checks_are_one_span_that_ends_in_the_transaction_that_makes_the_attempt_merging() {
    let live = open_live();
    live.gh_creates();
    live.run_job("open-repo", submit_only);
    let span = |edge: &str| json!({"from": "running", "to": "running", "check": edge, "what": "checks"});
    let statuses: Vec<Value> = live.events("open-repo-a1").iter().filter(|e| e["kind"] == "attempt-status").map(|e| e["data"].clone()).collect();
    assert_eq!(statuses[1..4], [span("start"), span("end"), json!({"from": "running", "to": "merging"})], "{statuses:?}");
    let txns: String = live.value(
        "SELECT group_concat(txn) FROM (SELECT txn FROM events WHERE kind = 'attempt-status' AND attempt = 'open-repo-a1' ORDER BY rowid)",
    );
    let txns: Vec<&str> = txns.split(',').collect();
    assert_eq!(txns[2], txns[3], "the end and running -> merging share a transaction");
}

/// With a base that conflicts with the Lead Branch, the submit is accepted (it runs no base check, no Gate: #425) and the base check is
/// the first step of the Attempt's merge (§7.4 step 6), before both Gates: the Attempt ends `failed` / `merge_conflict`, a ladder cause,
/// the conflicted files and the command that merges the base in its detail, and nothing is gated, pushed or opened. The Job goes back to
/// `queued`; the next Attempt's Worker merges the base and resolves it, its merge commit lands as it is (no rebase), both Gates run on
/// it, and the pull request opens.
#[test]
fn an_open_job_with_a_conflicting_base_fails_its_merge_merge_conflict_until_the_next_attempt_merges_the_base() {
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
    assert_eq!((&first["outcome"], &first["cause"]), (&json!("failed"), &json!("merge_conflict")), "{first}");
    // The detail of the failure is the feedback the next Attempt's Spec carries.
    let retry = live.event("open-repo-a1", "attempt-retry");
    let detail = retry["data"]["feedback"].as_str().unwrap();
    assert!(detail.contains("README.md") && detail.contains(&base[..7]), "the conflicted file and the base are named: {retry}");
    assert!(detail.contains("merge it into your branch") && detail.contains("cpd attempt submit open-repo-a1"), "what the Worker does is named: {retry}");
    assert!(detail.contains("Nothing was gated, pushed or opened"), "{retry}");
    assert_eq!(live.job_status("open-repo"), "queued", "a ladder cause");
    // Before the Gates: nothing gated, pushed, created or written.
    assert_eq!(live.gate_runs(), Vec::<String>::new(), "no Gate ran on a head the base refuses");
    assert_eq!((live.lead_sha(), live.remote("cpd/pay/repo")), (tip.clone(), remote));
    assert!(live.gh_pr_calls().iter().all(|c| c[1] != "create"), "{:?}", live.gh_pr_calls());
    assert_eq!((live.pr_row().0.as_str(), live.phase().as_str()), ("dispatch", "dispatch"));

    // The next Attempt's Worker merges the base and resolves the conflict: its merge commit lands as it is, and the PR opens.
    let (second, submitted) = live.run_job("open-repo", merge_the_base_and_submit);

    assert_eq!((&second["outcome"], &second["cause"]), (&json!("merged"), &json!("merged")), "{second}");
    assert_eq!(second["id"], "open-repo-a2");
    assert_eq!(submitted["ok"], true, "{submitted}");
    let new = live.lead_sha();
    assert_ne!(new, tip);
    assert_eq!(live.fx.git(&live.repo, &["rev-parse", &format!("{new}~1")]), tip);
    assert!(live.fx.git_try(&live.repo, &["merge-base", "--is-ancestor", &base, &new]).status.success(), "the merge commit holds the base");
    assert_eq!(fs::read_to_string(live.lead.join("README.md")).unwrap(), "mine\n");
    assert_eq!(live.gate_runs(), [format!("make quick head={new}"), format!("make test head={new}")], "both Gates ran once, on the resolved head");
    live.assert_no_slot_and_no_gate_worktree();
    assert_eq!(live.remote("cpd/pay/repo"), new);
    assert_eq!(live.gh_pr_calls().iter().filter(|c| c[1] == "create").count(), 1);
    assert_eq!(live.pr_row().0, "open");
    assert_eq!(live.job_status("open-repo"), "merged");
}

/// A Gate failure is the merge's, not the submit's (#425): the submit is accepted (it runs no Gate), the merge runs `gate.pr` in the Attempt
/// worktree, and its failure ends the Attempt `failed` / `gate_failed` with the hook's tail and log in the detail, nothing pushed, created or
/// written. `gate_failed` is no ladder cause: the Job fails and the Lead decides.
#[test]
fn an_open_jobs_gate_failure_ends_its_merge_gate_failed_and_pushes_nothing_creates_nothing_and_fails_the_attempt() {
    let live = open_live();
    live.gh_creates();
    live.hooks("[pre-merge]\ngate = \"echo the gate says no; exit 1\"\n");
    *live.hooks.reply.lock().unwrap() = Reply::exit(1, "the gate says no\n", "");
    let before = live.remote("cpd/pay/repo");

    let (attempt, submitted) = live.run_job("open-repo", submit_only);

    assert_eq!(submitted["ok"], true, "the submit runs no Gate: {submitted}");
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("failed"), &json!("gate_failed")), "{attempt}");
    let failed = live.plan_events("job-failed");
    assert_eq!(failed.len(), 1, "{failed:?}");
    let detail = failed[0].1["detail"].as_str().unwrap();
    assert!(detail.contains("pre-merge failed (exit 1)"), "{detail}");
    assert!(detail.contains("the gate says no"), "the hook's tail is in the detail: {detail}");
    assert!(detail.contains("open-repo-a1.pre-merge.log"), "the hook's log is in the detail: {detail}");
    assert_eq!(live.job_status("open-repo"), "failed", "gate_failed is no ladder cause: the Lead decides");
    assert_eq!(live.gate_dirs(), [live.canonical_worktree("open-repo-a1")], "the Gate ran once, in the Attempt worktree");
    live.assert_no_slot_and_no_gate_worktree();
    assert_eq!(live.remote("cpd/pay/repo"), before, "nothing was pushed");
    assert!(live.gh_pr_calls().iter().all(|c| c[1] != "create"), "{:?}", live.gh_pr_calls());
    assert_eq!((live.pr_row().0.as_str(), live.phase().as_str()), ("dispatch", "dispatch"));
    assert!(live.plan_events("pr-opened").is_empty());
}

/// A `gh` failure after the fast-forward ends the Attempt `failed` / `crashed` with the detail `gh_failed`: the push stands, the PR stays
/// `dispatch`, and the Job goes through the ladder (the next Attempt finds the pull request, or creates it).
#[test]
fn an_open_jobs_gh_failure_after_the_push_ends_the_attempt_crashed_with_gh_failed_and_the_pr_stays_dispatch() {
    let live = open_live();
    live.runner().on("gh", |c| c.args[..2] == ["pr", "list"], Reply::ok("[]")).on("gh", |_| true, Reply::exit(1, "", "gh: boom\n"));
    let tip = live.lead_sha();

    let (attempt, _) = live.run_job("open-repo", submit_only);

    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("failed"), &json!("crashed")), "{attempt}");
    let retry = live.event("open-repo-a1", "attempt-retry");
    assert!(retry["data"]["feedback"].as_str().unwrap().contains("gh_failed"), "{retry}");
    assert_eq!(live.job_status("open-repo"), "queued", "a ladder cause");
    assert_eq!(live.remote("cpd/pay/repo"), tip, "the push happened before gh");
    assert_eq!((live.pr_row().0.as_str(), live.phase().as_str()), ("dispatch", "dispatch"));
    assert!(live.plan_events("pr-opened").is_empty());
}

/// The update Job on a conflict: the same path as the open Job without `gh`. Its first Attempt is accepted at the submit and fails at the
/// merge `merge_conflict` (the base check, before the Gates); the next Attempt's Worker merges the base, and cpd runs both Gates on it and
/// pushes the resolution in the merge; the pull request is already open, so nothing is created and no PR row changes.
#[test]
fn an_update_job_with_a_conflicting_base_fails_its_merge_until_the_next_attempt_merges_it_and_its_merge_pushes_the_resolution() {
    let live = open_pr_live();
    live.add_cpd_job("up-repo", "update");
    live.commit(&live.lead, "README.md", "mine\n", "mine");
    let tip = live.lead_sha();
    let base = live.advance_base("README.md", "theirs\n");
    let row = live.pr_row();
    let remote = live.remote("cpd/pay/repo");

    let (first, submitted) = live.run_job("up-repo", submit_only);

    assert_eq!(submitted["ok"], true, "{submitted}");
    assert_eq!((&first["outcome"], &first["cause"]), (&json!("failed"), &json!("merge_conflict")), "{first}");
    let retry = live.event("up-repo-a1", "attempt-retry");
    let detail = retry["data"]["feedback"].as_str().unwrap();
    assert!(detail.contains("README.md") && detail.contains(&base[..7]), "{retry}");
    assert_eq!(live.job_status("up-repo"), "queued", "a ladder cause");
    assert_eq!(live.gate_runs(), Vec::<String>::new(), "the base check is before the Gates");
    assert_eq!((live.lead_sha(), live.remote("cpd/pay/repo")), (tip.clone(), remote), "nothing merged, nothing pushed");

    let (attempt, submitted) = live.run_job("up-repo", merge_the_base_and_submit);

    assert_eq!(submitted["ok"], true, "{submitted}");
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("merged"), &json!("merged")), "{attempt}");
    let new = live.lead_sha();
    assert_eq!(live.fx.git(&live.repo, &["rev-parse", &format!("{new}~1")]), tip);
    assert_eq!(live.remote("cpd/pay/repo"), new, "its merge pushes the resolution");
    assert_eq!(live.gate_runs(), [format!("make quick head={new}"), format!("make test head={new}")], "both Gates ran, once, on the resolved head");
    live.assert_no_slot_and_no_gate_worktree();
    assert!(live.gh_pr_calls().is_empty(), "the pull request exists: {:?}", live.gh_pr_calls());
    assert_eq!(live.pr_row(), row, "an update Job writes no PR row");
    assert!(live.plan_events("pr-opened").is_empty());
    let merged: Vec<Value> = live.plan_events("job-merged").into_iter().map(|e| e.1).filter(|d| d["kind"] == "update").collect();
    assert_eq!(merged.len(), 1, "{merged:?}");
}

/// The newest pull request of the Lead Branch is already `merged` on GitHub and cpd never recorded it (§7.7 submit step 1, §7.4 step 8b): the `open`
/// Job adopts it. Its submit runs no Gate; its merge runs both Gates (step 6, before the adoption: #425 moved the adoption after them,
/// hs-d679) but pushes nothing and creates nothing, and step 10 writes the rows of `pr open` step 0: the PR
/// `merged`, `pr-merged` (no `pr-opened`), and the Plan `done` with the last PR.
#[test]
fn an_open_job_adopts_a_pull_request_merged_on_github_after_its_gates_with_no_push() {
    let live = open_live();
    let merged = common::pay_lookup().replace("\"state\":\"OPEN\"", "\"state\":\"MERGED\"").replace("\"mergedAt\":null", "\"mergedAt\":\"2026-10-20T12:00:00Z\"");
    live.runner()
        .on("gh", |c| c.args[..2] == ["pr", "list"], Reply::ok(&format!("[{{\"number\":812,\"url\":\"{PR_URL}\",\"state\":\"MERGED\",\"createdAt\":\"2026-10-28T10:00:00Z\"}}]")))
        .on("gh", |c| c.args[..2] == ["api", "graphql"], Reply::ok(&merged));
    let before = live.remote("cpd/pay/repo");

    let tip = live.lead_sha();

    let (attempt, submitted) = live.run_job("open-repo", |live, id| {
        let submitted = submit(live, id);
        assert_eq!(live.gate_runs(), Vec::<String>::new(), "the submit ran a Gate");
        submitted
    });

    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("merged"), &json!("merged")), "{attempt}");
    assert_eq!(submitted["ok"], true, "{submitted}");
    assert_eq!(live.gate_runs(), [format!("make quick head={tip}"), format!("make test head={tip}")], "both Gates ran at the merge, before the adoption");
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
        .on("gh", |c| c.args[..2] == ["pr", "list"], Reply::ok(&format!("[{{\"number\":265,\"url\":\"https://github.com/acme/pay/pull/265\",\"state\":\"MERGED\",\"createdAt\":\"2026-10-22T08:00:00Z\"}}]")));
    let tip = live.lead_sha();

    let (attempt, submitted) = live.run_job("open-repo", submit_only);

    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("merged"), &json!("merged")), "{attempt}");
    assert_eq!(submitted["ok"], true, "{submitted}");
    assert_eq!(live.gate_runs(), [format!("make quick head={tip}"), format!("make test head={tip}")]);
    let created = live.gh_pr_calls().into_iter().filter(|c| c[1] == "create").count();
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
    let mut text = format!("{}\n{call}\n", json!({"type": "session", "timestamp": "2026-10-14T21:00:00.000Z"}));
    if !result.is_empty() {
        text += &format!("{result}\n");
    }
    fs::write(&file, text).unwrap();
    // Quiet for an hour: the file's time is an hour before the virtual now, which is where the driver reads the quiet from.
    let hour_ago = std::time::SystemTime::from(live.fx.fakes.clock.utc()) - Duration::from_secs(3600);
    fs::OpenOptions::new().write(true).open(&file).unwrap().set_modified(hour_ago).unwrap();
    let pane = live.fx.tmux(&["list-panes", "-a", "-F", "#{window_name} #{pane_pid}"]);
    let pane = String::from_utf8(pane.stdout).unwrap();
    let pid: u32 = pane.lines().filter(|l| !l.starts_with("watch ") && !l.starts_with("main ")).find_map(|l| l.split(' ').nth(1)?.parse().ok()).expect("the Worker's pane");
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
    assert!(!live.events(&id).iter().any(|e| e["kind"] == "error"), "{:?}", live.events(&id));
}

#[test]
fn a_tool_call_with_its_result_is_not_a_stall() {
    let live = live("[timeouts]\nstall_seconds = 1");
    // The file is quiet past stall_seconds, but the call has its result.
    assert_not_killed(&live, r#"{"type":"message","message":{"role":"toolResult","toolCallId":"call_1","content":[{"type":"text","text":"done"}],"isError":false}}"#, 6);
}

#[test]
fn stall_seconds_0_kills_nothing() {
    let live = live("[timeouts]\nstall_seconds = 0");
    // The file is quiet for an hour, inside a tool call: only stall_seconds = 0 keeps the call alive.
    assert_not_killed(&live, "", 6);
}
