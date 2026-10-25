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
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, FakeRunner, Handler, Reply, Runner};
use cpd_core::ids::{AttemptId, PlanId};
use serde_json::{Value, json};

use super::driver::{JOB, PLAN, PR_TITLE, PR_URL, add_job_in, fixture, seed_home, seed_rows, store};

/// The Gate's hook, a reply of the fake `sh`: it notes the test slot and the commit the hook runs on (the head of its worktree, read
/// when it runs: the gate worktree is gone when the Gate ends) and answers `reply`.
struct Hooks {
    runner: Arc<dyn Runner>,
    runs: Mutex<Vec<String>>,
    reply: Mutex<Reply>,
}

impl Handler for Hooks {
    fn handle(&self, call: &Call) -> Reply {
        let slot = call.envs.iter().find(|(k, _)| k == "CPD_TEST_SLOT").and_then(|(_, v)| v.clone()).unwrap_or_default();
        let mut head = Command::new("git");
        head.args(["rev-parse", "HEAD"]).current_dir(call.cwd.as_ref().expect("a hook has a cwd"));
        let head = self.runner.output(&mut head, "git", Duration::from_secs(5)).expect("git rev-parse");
        self.runs.lock().unwrap().push(format!("slot={slot} head={}", String::from_utf8_lossy(&head.stdout).trim()));
        self.reply.lock().unwrap().clone()
    }
}

/// Plan `pay` as `pr start` leaves it, in this process: a repo `pay` whose `origin` is the bare repo `repos/origin.git` (a repo of the
/// fake git, which holds `main` and the Lead Branch), its Lead worktree, the rows (Job `api` queued) and the work session of the fake tmux.
struct Live {
    fx: Fx,
    repo: PathBuf,
    lead: PathBuf,
    origin: PathBuf,
    hooks: Arc<Hooks>,
}

fn live(config_extra: &str) -> Live {
    let fx = Fx::with_ports();
    let repo = fx.keyed_repo("pay");
    let origin = fx.env.root().join("repos/origin.git");
    fx.git(fx.env.root(), &["init", "--bare", "-q", "-b", "main", origin.to_str().unwrap()]);
    fx.local_origin(&repo, &origin);
    // The Lead worktree as `pr start` makes it, and both branches pushed (R35).
    let lead = repo.parent().unwrap().join("pay.pay");
    fx.git(&repo, &["worktree", "add", "-q", "-b", "cpd/pay", lead.to_str().unwrap(), "main"]);
    fx.git(&repo, &["push", "-q", "origin", "main", "cpd/pay"]);
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
    fn runner(&self) -> FakeRunner {
        (*self.fx.fakes.runner).clone()
    }

    fn fixture(&self, sql: &str) {
        fixture(&self.fx.env, sql);
    }

    /// One value of the store, read through a store opened and closed here: `env.query` keeps a connection, and with it a
    /// descriptor, open, which could be the fd 3 a driver is handed.
    fn query<T: rusqlite::types::FromSql>(&self, sql: &str) -> T {
        store(&self.fx.env).read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?)).unwrap()
    }

    fn job_status(&self, key: &str) -> String {
        self.query(&format!("SELECT status FROM jobs WHERE plan = 'pay' AND key = '{key}'"))
    }

    fn phase(&self) -> String {
        self.query("SELECT phase FROM plans WHERE id = 'pay'")
    }

    fn worktree(&self, id: &str) -> PathBuf {
        self.repo.parent().unwrap().join(format!("pay.pay--{id}"))
    }

    fn lead_sha(&self) -> String {
        self.fx.git(&self.repo, &["rev-parse", "refs/heads/cpd/pay"])
    }

    fn remote(&self, branch: &str) -> String {
        self.fx.git(&self.origin, &["rev-parse", &format!("refs/heads/{branch}")])
    }

    /// Commits `path` with `content` in `dir` and returns the commit.
    fn commit(&self, dir: &Path, path: &str, content: &str, message: &str) -> String {
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

    /// What the Gate noted: the lines `slot=<n> head=<sha>`, one per run of a hook.
    fn gate_runs(&self) -> Vec<String> {
        self.hooks.runs.lock().unwrap().clone()
    }

    /// The fake `gh`: no pull request of the branch yet, `pr create` answers [`PR_URL`], and the lookup of the pull request answers it.
    fn gh_creates(&self) {
        self.runner()
            .on("gh", |c| c.args[..2] == ["pr", "list"], Reply::ok("[]"))
            .on("gh", |c| c.args[..2] == ["pr", "create"], Reply::ok(PR_URL))
            .on("gh", |c| c.args[..2] == ["api", "graphql"], Reply::ok(&common::pay_lookup()));
    }

    /// `gh`'s calls that are not the poll's or a lookup's: `pr list`, `pr create`.
    fn gh_pr_calls(&self) -> Vec<Vec<String>> {
        self.fx.calls("gh").into_iter().map(|c| c.args).filter(|a| a[0] == "pr").collect()
    }

    /// The Events of `kind` of Plan `pay`: `(txn, data)`, in order.
    fn plan_events(&self, kind: &str) -> Vec<(i64, Value)> {
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
    fn pr_row(&self) -> (String, Option<i64>, Option<String>) {
        store(&self.fx.env)
            .read(|c| Ok(c.query_row("SELECT status, pr_number, pr_opened_at FROM prs WHERE plan = 'pay' AND key = 'repo'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?))
            .unwrap()
    }

    fn attempt(&self, id: &str) -> Value {
        self.fx.cpd(&["attempt", "show", id, "--plan", PLAN]).ok()["attempt"].clone()
    }

    /// The Events about Attempt `id`, in order.
    fn events(&self, id: &str) -> Vec<Value> {
        let all = self.fx.cpd(&["events", "list", "--plan", PLAN]).ok()["events"].as_array().unwrap().clone();
        all.into_iter().filter(|e| e["attempt"] == id).collect()
    }

    fn event(&self, id: &str, kind: &str) -> Value {
        self.events(id).into_iter().find(|e| e["kind"] == kind).unwrap_or_else(|| panic!("no {kind} for {id}"))
    }

    /// Job `key` of PR `repo` as cpd adds an `open` or `update` Job: `appended`, no Dependencies, `queued` with its fingerprint.
    fn add_cpd_job(&self, key: &str, kind: &str) {
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

    /// Dispatches Job `key`, runs its driver and returns the Attempt it ended and what `worker` returned: `worker` is the Worker's pi,
    /// called once, in the driver's first poll, with the id of the Attempt.
    fn run_job<W>(&self, key: &str, worker: impl FnOnce(&Live, &str) -> W) -> (Value, W) {
        let out = self.dispatch(key).ok();
        let id = out["attempt"].as_str().unwrap_or_else(|| panic!("no attempt in {out}")).to_owned();
        let launch = self.fx.take_launch(self.fx.fakes.spawn.launches().len() - 1);
        let home = CpdHome::resolve(Some(self.fx.env.cpd_home().into()), None).unwrap();
        let lock = home.attempt_lock(&PlanId::parse(PLAN).unwrap(), &AttemptId::parse(&id).unwrap());
        let _fd3 = self.fx.hand_over(fs::File::open(lock).unwrap());
        let (mut worker, mut did) = (Some(worker), None);
        let ran = self.fx.run_launch_with(&launch, |_, nap| {
            if nap == 1 {
                did = Some((worker.take().expect("the first nap is once"))(self, &id));
            }
        });
        assert_eq!(ran.exit, 0, "{}", ran.stdout);
        (self.attempt(&id), did.expect("the driver napped: the Worker ran"))
    }

    /// The Worker's pi ends without submitting again: its window is gone from the fake server.
    fn end_the_worker(&self) {
        for pane in self.fx.panes().iter().filter(|p| p.window_name != "main") {
            assert!(self.fx.tmux(&["kill-window", "-t", &pane.window]).status.success());
        }
    }

    /// `job dispatch <key>`: the driver's `Launch` is recorded, nothing is started.
    fn dispatch(&self, key: &str) -> common::Run {
        self.fx.cpd(&["job", "dispatch", key, "--plan", PLAN])
    }
}

/// What `attempt submit` answered the Worker.
fn submit(live: &Live, id: &str) -> Value {
    // The environment of the Worker's window: its Plan and its Attempt.
    let r = live.fx.cpd_with(&live.worktree(id), &["attempt", "submit", id], &[("CPD_PLAN", PLAN), ("CPD_ATTEMPT", id)]).json();
    r
}

/// A Worker that did nothing: it submits the Lead Branch as it is.
fn submit_only(live: &Live, id: &str) -> Value {
    submit(live, id)
}

/// A Worker that submits, is refused while the base conflicts, merges the base resolving the conflict on its own side and submits
/// again: `(refused, submitted)`. (`FakeGit` has no `MERGE_HEAD`: a conflicted merge cannot be concluded by a `commit`, so the Worker
/// resolves in the merge, with `-X ours`.)
fn merge_the_base(live: &Live, id: &str) -> (Value, Value) {
    let refused = submit(live, id);
    let wt = live.worktree(id);
    live.fx.git(&wt, &["fetch", "-q", "origin", "main"]);
    live.fx.git(&wt, &["merge", "-q", "--no-edit", "-X", "ours", "origin/main"]);
    (refused, submit(live, id))
}

/// PR `repo` as `pr start` leaves it, with a bare `origin` that holds `main` and the Lead Branch as the lock pushed it, the policy
/// `pr.gate = "local"` with a `pre-merge` hook that notes the slot and the commit it gates, Job `api` merged (its commit `api.txt` is on
/// the Lead Branch, **not pushed**), and the PR's `open` Job `open-repo` queued. `gh` is the test's to script.
fn open_live() -> Live {
    let live = live("");
    live.fx.env.owner_policy("acme", "[pr]\ngate = \"local\"\n");
    let merged = live.commit(&live.lead, "api.txt", "api\n", "api");
    live.fixture(&format!(
        "UPDATE jobs SET status = 'running' WHERE key = 'api'; UPDATE jobs SET status = 'merged', merged_sha = '{merged}' WHERE key = 'api';"
    ));
    live.hooks("[pre-merge]\ngate = \"make test\"\n");
    live.add_cpd_job("open-repo", "open");
    live
}

/// The same with the pull request already open (812, as `pr open` leaves it), the Plan in `deploy`, and no `open` Job.
fn open_pr_live() -> Live {
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

/// A Worker that did nothing, with the base unmoved or moved without a conflict, submits the Lead Branch as it is: its merge merges
/// nothing (the Lead Branch does not move), runs the Gate in a test slot at the Lead tip, pushes it, and runs `gh pr create` (never with
/// `--draft`), writing `pr open`'s rows and Events in the transaction that ends the Job `merged`.
#[test]
fn an_open_job_with_no_conflict_merges_nothing_gates_in_a_slot_pushes_and_opens_the_pull_request() {
    let live = open_live();
    live.gh_creates();
    let tip = live.lead_sha();
    // The base moved, without a conflict: that is no reason to merge it.
    let base = live.advance_base("base.txt", "base\n");
    assert_ne!(live.remote("cpd/pay"), tip, "the Lead's commit is not pushed yet");

    let (attempt, submitted) = live.run_job("open-repo", submit_only);

    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("merged"), &json!("merged")), "{attempt}");
    assert_eq!(submitted["ok"], true, "{submitted}");
    assert_eq!(live.job_status("open-repo"), "merged");
    // Nothing merged: the Lead Branch is where it was, and does not hold the base.
    assert_eq!(live.lead_sha(), tip);
    assert_eq!(attempt["merged_sha"], tip);
    assert!(!live.lead.join("base.txt").exists());
    assert_ne!(live.fx.git(&live.repo, &["merge-base", &tip, &base]), base, "the base is not an ancestor of the Lead Branch");
    // The Gate ran once, at the Lead tip, in a test slot; its gate worktree is gone.
    assert_eq!(live.gate_runs(), [format!("slot=0 head={tip}")]);
    let log = fs::read_to_string(live.fx.env.cpd_home().join("run/pay/logs/pr-repo.pre-merge.log")).unwrap();
    assert!(log.starts_with("gate: slot 0 taken after "), "{log}");
    let left: Vec<String> =
        fs::read_dir(live.repo.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.contains(".gate-")).collect();
    assert!(left.is_empty(), "{left:?}");
    // Pushed, and the pull request created: ready, never a draft.
    assert_eq!(live.remote("cpd/pay"), tip);
    let calls = live.gh_pr_calls();
    assert_eq!(calls[0], ["pr", "list", "--head", "cpd/pay", "--state", "all", "--json", "number,url,state"]);
    let created: Vec<&Vec<String>> = calls.iter().filter(|c| c[1] == "create").collect();
    assert_eq!(created.len(), 1, "{calls:?}");
    assert_eq!(created[0][..8], ["pr", "create", "--base", "main", "--head", "cpd/pay", "--title", PR_TITLE]);
    assert!(live.fx.calls("gh").iter().all(|c| !c.args.iter().any(|a| a == "--draft" || a == "ready" || a == "convert-to-draft")), "never a draft");
    // `pr open`'s rows and Events, with `job-merged`, in one transaction.
    let (status, number, opened_at) = live.pr_row();
    assert_eq!((status.as_str(), number, opened_at.is_some()), ("open", Some(812), true));
    assert_eq!(live.phase(), "deploy");
    assert_eq!(live.query::<String>("SELECT pr_url FROM prs WHERE plan = 'pay'"), PR_URL);
    assert_eq!(live.query::<i64>("SELECT count(*) FROM pull_requests WHERE repo = 'acme/pay' AND number = 812 AND state = 'open' AND draft = 0"), 1);
    assert!(live.query::<Option<i64>>("SELECT pull_request FROM prs WHERE plan = 'pay'").is_some());
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
    let txns: String = live.query(
        "SELECT group_concat(txn) FROM (SELECT txn FROM events WHERE kind = 'attempt-status' AND attempt = 'open-repo-a1' ORDER BY rowid)",
    );
    let txns: Vec<&str> = txns.split(',').collect();
    assert_eq!(txns[2], txns[3], "the end and running -> merging share a transaction");
}

/// With a base that conflicts with the Lead Branch, the Worker's merge commit is required: the submit is refused `base_conflict` (before
/// the Gate, so the Gate never runs on a head that the base would refuse), with the files and the commands, until the Worker has merged
/// the base and resolved it. The merge commit then lands as it is (no rebase), the Gate runs on it, and the pull request opens.
#[test]
fn an_open_job_with_a_conflicting_base_is_refused_base_conflict_until_the_worker_merges_the_base() {
    let live = open_live();
    live.gh_creates();
    // The Lead Branch changed README.md, and so did the base.
    live.commit(&live.lead, "README.md", "mine\n", "mine");
    let tip = live.lead_sha();
    let base = live.advance_base("README.md", "theirs\n");

    let (attempt, (refused, submitted)) = live.run_job("open-repo", merge_the_base);

    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(refused["error"]["code"], "base_conflict");
    let details = &refused["error"]["details"];
    assert_eq!((&details["pr"], &details["base"], &details["base_sha"], &details["sha"], &details["files"]), (&json!("repo"), &json!("main"), &json!(base), &json!(tip), &json!(["README.md"])));
    let next = details["next"].as_array().unwrap();
    assert!(next[0].as_str().unwrap().starts_with("git -C ") && next[0].as_str().unwrap().ends_with(" merge --no-edit origin/main"), "{next:?}");
    assert_eq!(next[1], "cpd attempt submit open-repo-a1");

    // Then the Worker's merge commit lands as it is, and the PR opens.
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("merged"), &json!("merged")), "{attempt}");
    assert_eq!(submitted["ok"], true, "{submitted}");
    let new = live.lead_sha();
    assert_ne!(new, tip);
    assert_eq!(live.fx.git(&live.repo, &["rev-parse", &format!("{new}~1")]), tip);
    assert!(live.fx.git_try(&live.repo, &["merge-base", "--is-ancestor", &base, &new]).status.success(), "the merge commit holds the base");
    assert_eq!(fs::read_to_string(live.lead.join("README.md")).unwrap(), "mine\n");
    assert_eq!(live.gate_runs(), [format!("slot=0 head={new}")], "the Gate ran once, on the resolved head");
    assert_eq!(live.remote("cpd/pay"), new);
    assert_eq!(live.gh_pr_calls().iter().filter(|c| c[1] == "create").count(), 1);
    assert_eq!(live.pr_row().0, "open");
    assert_eq!(live.job_status("open-repo"), "merged");
}

/// A Gate failure fails the Attempt like any other (§7.7): the submit is refused `gate_failed` (the Worker fixes it), and nothing is pushed,
/// created or written. A Worker that does not fix it ends crashed, through the ladder.
#[test]
fn an_open_jobs_gate_failure_is_gate_failed_and_pushes_nothing_creates_nothing_and_fails_the_attempt() {
    let live = open_live();
    live.gh_creates();
    live.hooks("[pre-merge]\ngate = \"echo the gate says no; exit 1\"\n");
    *live.hooks.reply.lock().unwrap() = Reply::exit(1, "the gate says no\n", "");
    let before = live.remote("cpd/pay");

    // The Worker does not fix it: its pi ends.
    let (attempt, refused) = live.run_job("open-repo", |live, id| {
        let refused = submit(live, id);
        live.end_the_worker();
        refused
    });

    assert_eq!((&refused["ok"], &refused["error"]["code"]), (&json!(false), &json!("gate_failed")), "{refused}");
    assert!(refused["error"]["details"]["tail"].as_str().unwrap().contains("the gate says no"), "{refused}");
    assert!(refused["error"]["details"]["log"].as_str().unwrap().ends_with("pr-repo.pre-merge.log"), "{refused}");
    assert_eq!(attempt["outcome"], "failed");
    assert_ne!(live.job_status("open-repo"), "merged");
    assert_eq!(live.remote("cpd/pay"), before, "nothing was pushed");
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
    assert_eq!(live.remote("cpd/pay"), tip, "the push happened before gh");
    assert_eq!((live.pr_row().0.as_str(), live.phase().as_str()), ("dispatch", "dispatch"));
    assert!(live.plan_events("pr-opened").is_empty());
}

/// The update Job on a conflict: the same path as the open Job without `gh`. The Worker merges the base (the submit refuses until it has),
/// cpd gates and pushes the resolution in the merge; the pull request is already open, so nothing is created and no PR row changes.
#[test]
fn an_update_job_with_a_conflicting_base_is_refused_until_the_worker_merges_it_and_its_merge_pushes_the_resolution() {
    let live = open_pr_live();
    live.add_cpd_job("up-repo", "update");
    live.commit(&live.lead, "README.md", "mine\n", "mine");
    let tip = live.lead_sha();
    let base = live.advance_base("README.md", "theirs\n");
    let row = live.pr_row();

    let (attempt, (refused, _)) = live.run_job("up-repo", merge_the_base);

    assert_eq!((&refused["error"]["code"], &refused["error"]["details"]["files"], &refused["error"]["details"]["base_sha"]), (&json!("base_conflict"), &json!(["README.md"]), &json!(base)), "{refused}");
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("merged"), &json!("merged")), "{attempt}");
    let new = live.lead_sha();
    assert_eq!(live.fx.git(&live.repo, &["rev-parse", &format!("{new}~1")]), tip);
    assert_eq!(live.remote("cpd/pay"), new, "its merge pushes the resolution");
    assert_eq!(live.gate_runs(), [format!("slot=0 head={new}")]);
    assert!(live.gh_pr_calls().is_empty(), "the pull request exists: {:?}", live.gh_pr_calls());
    assert_eq!(live.pr_row(), row, "an update Job writes no PR row");
    assert!(live.plan_events("pr-opened").is_empty());
    let merged: Vec<Value> = live.plan_events("job-merged").into_iter().map(|e| e.1).filter(|d| d["kind"] == "update").collect();
    assert_eq!(merged.len(), 1, "{merged:?}");
}

/// A feedback fix Job on an open PR reaches the remote through the update path (hs-d497): `job add` queues the PR's `update` Job in its own
/// transaction (once, however many fixes are added; `reason` `feedback`), which is Ready only when the fixes have merged, gates the Lead
/// Branch in a slot and pushes it. No `pr rebase` or `pr push` is run by anyone.
#[test]
fn a_feedback_fix_job_on_an_open_pr_reaches_the_remote_through_the_update_job() {
    let live = open_pr_live();
    let spec = live.fx.env.root().join("fix-spec.json");
    fs::write(&spec, r#"{"goal": "Fix it.", "context": "The checks failed.", "touches": ["fix.txt"], "acceptance": ["ok"], "out_of_scope": []}"#).unwrap();
    let add = |key: &str| live.fx.cpd(&["job", "add", key, "--title", "Fix the checks", "--tier", "S2", "--spec", spec.to_str().unwrap(), "--pr", "repo", "--plan", "pay"]).ok();
    assert_eq!(add("fix")["job"]["status"], "queued");
    assert_eq!(add("fix2")["job"]["status"], "queued");

    // The Job `job add` queued: one, `up-repo`, from `job add`, for the feedback.
    let update: Vec<(String, String)> = store(&live.fx.env)
        .read(|c| {
            let mut stmt = c.prepare("SELECT key, status FROM jobs WHERE kind = 'update' ORDER BY key")?;
            Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap();
    assert_eq!(update, [("up-repo".to_owned(), "queued".to_owned())]);
    let added: Vec<Value> = live.plan_events("job-added").into_iter().map(|e| e.1).filter(|d| d["kind"] == "update").collect();
    assert_eq!(added.len(), 1, "{added:?}");
    assert_eq!((&added[0]["by"], &added[0]["reason"], &added[0]["pr"]), (&json!("job add"), &json!("feedback"), &json!("repo")));
    // It waits for the fixes: not Ready while they are not merged.
    common::refused(&live.dispatch("up-repo"), "job_not_ready", 3);

    // The merges of the fixes push best-effort (R35); make those pushes fail, as they do when origin is unreachable, so that what reaches
    // the pull request is the update Job's.
    let blocked = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let flag = blocked.clone();
    live.runner().on("git", move |c| flag.load(std::sync::atomic::Ordering::SeqCst) && c.args.iter().any(|a| a == "push"), Reply::exit(1, "", "remote: nope\n"));
    for key in ["fix", "fix2"] {
        live.run_job(key, |live, id| {
            live.commit(&live.worktree(id), &format!("{key}.txt"), "fixed\n", key);
            submit(live, id)
        });
    }
    let lead = live.lead_sha();
    assert!(live.lead.join("fix.txt").exists() && live.lead.join("fix2.txt").exists());
    assert_ne!(live.remote("cpd/pay"), lead, "the fixes' own pushes failed: the remote lacks them");
    blocked.store(false, std::sync::atomic::Ordering::SeqCst);
    let before = live.gate_runs();

    let (attempt, submitted) = live.run_job("up-repo", submit_only);

    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("merged"), &json!("merged")), "{attempt}");
    assert_eq!(submitted["ok"], true, "{submitted}");
    assert_eq!(live.lead_sha(), lead, "nothing to merge: the base did not move");
    assert_eq!(live.remote("cpd/pay"), lead);
    assert_eq!(live.gate_runs(), [before, vec![format!("slot=0 head={lead}")]].concat(), "the Gate ran on the Lead tip, in a slot");
    assert!(live.gh_pr_calls().is_empty());
}

/// The newest pull request of the Lead Branch is already `merged` on GitHub and cpd never recorded it (§7.7 submit step 1, §7.4 step 8b): the `open`
/// Job adopts it. Its submit runs no Gate, its merge pushes nothing and creates nothing, and step 10 writes the rows of `pr open` step 0: the PR
/// `merged`, `pr-merged` (no `pr-opened`), and the Plan `done` with the last PR.
#[test]
fn an_open_job_adopts_a_pull_request_merged_on_github_with_no_gate_and_no_push() {
    let live = open_live();
    let merged = common::pay_lookup().replace("\"state\":\"OPEN\"", "\"state\":\"MERGED\"").replace("\"mergedAt\":null", "\"mergedAt\":\"2026-10-20T12:00:00Z\"");
    live.runner()
        .on("gh", |c| c.args[..2] == ["pr", "list"], Reply::ok(&format!("[{{\"number\":812,\"url\":\"{PR_URL}\",\"state\":\"MERGED\"}}]")))
        .on("gh", |c| c.args[..2] == ["api", "graphql"], Reply::ok(&merged));
    let before = live.remote("cpd/pay");

    let (attempt, submitted) = live.run_job("open-repo", submit_only);

    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("merged"), &json!("merged")), "{attempt}");
    assert_eq!(submitted["ok"], true, "{submitted}");
    assert_eq!(live.gate_runs(), Vec::<String>::new(), "no Gate");
    assert_eq!(live.remote("cpd/pay"), before, "no push");
    assert!(live.gh_pr_calls().iter().all(|c| c[1] != "create"));
    assert_eq!(live.pr_row().0, "merged");
    assert_eq!(live.phase(), "done");
    assert!(live.plan_events("pr-opened").is_empty());
    assert_eq!(live.plan_events("pr-merged").len(), 1);
}
