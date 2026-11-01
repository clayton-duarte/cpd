//! The Attempt driver (contract §3.4; commands.md §5.5, §6.4), in this process: `job dispatch` records the driver's `Launch` on the
//! `FakeSpawn`, and the test runs it through `run_with` ([`Live::drive`]) on the fake git, the fake tmux and the `ManualClock`, with
//! the hooks and `gh` as replies of the `FakeRunner`. The Worker is the test's own closure, called after the driver's `n`th poll: it
//! does what pi would do in its pane (commit, `cpd attempt submit`), or its pi ends, on the same thread, with no process and no wait.
//!
//! There is no Reviewer (R19, Escalation rs-driver-a1-q1): a submitted Attempt
//! goes straight to the merge. `attempt submit` runs no quick check and no Gate;
//! the merge (A8) runs `gate.merge` (the repo's `pre-commit`) for every
//! Job and `gate.pr` (the repo's `pre-merge`) for an `open`/`update` Job, while the
//! Attempt is still `merging` (the tree shows it yellow). Contract §7.4 step 6, hs-d679.
//!
//! The `dispatch_refuses_*` tests refuse before any worktree, window or process exists: they run the command tree
//! in this process (`Fx`, [`Rows`]) against rows alone, with no repo, no tmux server and no fake pi.

use crate::common;

use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use common::fx::Fx as InProcess;
use common::{Run, TestEnv, refused};
use cpd_core::error::CpdError;
use cpd_core::home::CpdHome;
use cpd_core::host::deadline::Failed;
use cpd_core::host::runner::{Call, Reply, Runner};
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::plan::{jobs, locked_fingerprint};
use cpd_core::ports::Ports;
use cpd_core::store::{Store, WriteTxn};
use serde_json::{Value, json};

use super::driver_open::{Live, live};

pub(super) const PLAN: &str = "pay";
pub(super) const JOB: &str = "api";
/// The stored title of PR `repo`; the shape it is given is `config` (its Jobs are `work` Jobs).
pub(super) const PR_TITLE: &str = "Charge the card at checkout";
pub(super) const PR_URL: &str = "https://github.com/acme/pay/pull/812";
const ATTEMPT: &str = "api-a1";

/// O supports efforts 0–5, S 1–5, H 0–3.
const EFFORTS: [(&str, &str, &str); 3] =
    [("O", "p/opus", "[0,1,2,3,4,5]"), ("S", "p/sonnet", "[1,2,3,4,5]"), ("H", "p/haiku", "[0,1,2,3]")];

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

/// The config on this env's tmux server (`config_extra` is more TOML), a fake pi install whose Tier catalog is
/// seeded in the cache by [`seed_rows`], and the policy file of the repo `acme/pay`.
pub(super) fn seed_home(env: &TestEnv, config_extra: &str) {
    env.write_models();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\n{config_extra}\n[tmux]\nsocket = \"{}\"\n",
            env.tmux_socket()
        ),
    )
    .unwrap();
    fs::create_dir_all(env.root().join("install")).unwrap();
    fs::write(env.root().join("install/current-version"), "1.0.0\n").unwrap();
    let scope = env.root().join("install/releases/1.0.0/node_modules/@earendil-works");
    for package in ["pi-coding-agent", "pi-ai"] {
        fs::create_dir_all(scope.join(package).join("dist")).unwrap();
        fs::write(scope.join(package).join("dist/index.js"), "").unwrap();
    }
    // The PRs are of the repo `acme/pay` (their `name`): its policy file is required, empty unless a test sets hooks.
    env.repo_policy("acme", "pay", "");
}

/// The Tier catalog, Context, Lead Session, Plan (phase `dispatch`) and PR `repo` of Plan `pay` in the store of `env`;
/// `repo` and `lead` are the paths the PR names.
pub(super) fn seed_rows(env: &TestEnv, repo: &Path, lead: &Path) {
    let catalog: String = EFFORTS
        .iter()
        .map(|(letter, model, efforts)| {
            format!("INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('1.0.0', '{letter}', '{model}', '{efforts}', 't');")
        })
        .collect();
    fixture(
        env,
        &format!(
            "{catalog}
         INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
         INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, lead_worktree, title, description, shape, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '{repo}', 'pay', 'main', 'dispatch', 'cpd/pay/repo', '{lead}', '{PR_TITLE}', 'A description', 'config', cpd_txn(), 't', 't');",
            repo = repo.display(),
            lead = lead.display()
        ),
    );
}

pub(super) fn store(env: &TestEnv) -> Store {
    Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap()
}

pub(super) fn fixture(env: &TestEnv, sql: &str) {
    store(env).write("fixture", |tx| exec(tx, sql)).unwrap();
}

/// Job `key` of PR `pr` of Plan `pay`, queued with its Locked-Job fingerprint.
pub(super) fn add_job_in(env: &TestEnv, pr: &str, key: &str, needs: &[&str]) {
    let mut sql = format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
         VALUES ('pay', '{pr}', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 0, cpd_txn(), 't');"
    );
    for need in needs {
        sql += &format!("INSERT INTO dependencies (plan, job, needs) VALUES ('pay', '{key}', '{need}');");
    }
    fixture(env, &sql);
    let plan = PlanId::parse(PLAN).unwrap();
    let all = store(env).read(|conn| jobs::load(conn, &plan)).unwrap();
    let job = all.iter().find(|j| j.key == key).unwrap();
    fixture(env, &format!("UPDATE jobs SET status = 'queued', fingerprint = '{}' WHERE plan = 'pay' AND key = '{key}';", locked_fingerprint(job)));
}

/// [`Fx`] without a repo, a worktree, a tmux server, a fake pi or a child process: the same rows (Plan `pay`, PR
/// `repo`, Job `api` queued), written by SQL in an [`InProcess`] env whose `job dispatch` runs in this process. The
/// paths the rows name (`repo`, `lead`) do not exist: a refusal is made before anything reads them.
struct Rows {
    fx: InProcess,
    repo: PathBuf,
}

fn rows() -> Rows {
    rows_with("")
}

fn rows_with(config_extra: &str) -> Rows {
    let fx = InProcess::new();
    let repo = fx.env.root().join("repos/pay");
    seed_home(&fx.env, config_extra);
    seed_rows(&fx.env, &repo, &fx.env.root().join("repos/pay.pay"));
    add_job_in(&fx.env, "repo", JOB, &[]);
    Rows { fx, repo }
}

impl Rows {
    fn home(&self) -> CpdHome {
        CpdHome::resolve(Some(self.fx.env.cpd_home().into()), None).unwrap()
    }

    fn fixture(&self, sql: &str) {
        fixture(&self.fx.env, sql);
    }

    /// Job `key` of PR `repo` of Plan `pay`, queued with its Locked-Job fingerprint.
    fn add_job(&self, key: &str, needs: &[&str]) {
        self.add_job_in("repo", key, needs);
    }

    /// [`Rows::add_job`] in PR `pr`.
    fn add_job_in(&self, pr: &str, key: &str, needs: &[&str]) {
        add_job_in(&self.fx.env, pr, key, needs);
    }

    /// `job dispatch <key>`, in this process.
    fn dispatch(&self, key: &str) -> Run {
        self.fx.cpd(&["job", "dispatch", key, "--plan", PLAN])
    }

    fn job_status(&self, key: &str) -> String {
        self.fx.env.query(&format!("SELECT status FROM jobs WHERE plan = 'pay' AND key = '{key}'"))
    }

    /// The number of transactions, read through a store opened and closed here: `env.query` keeps a connection, and with
    /// it a descriptor, open, which could be the fd 3 a driver is handed.
    fn txns(&self) -> i64 {
        store(&self.fx.env).read(|c| Ok(c.query_row("SELECT count(*) FROM txns", [], |r| r.get(0))?)).unwrap()
    }

    fn attempt(&self, id: &str) -> Value {
        self.fx.cpd(&["attempt", "show", id, "--plan", PLAN]).ok()["attempt"].clone()
    }

    fn worktree(&self, id: &str) -> PathBuf {
        self.repo.parent().unwrap().join(format!("pay.pay--{id}"))
    }

    /// `attempt run api-a1` in this process, with `lock` (or nothing) as its fd 3.
    fn run_driver(&self, lock: Option<&Path>) -> Run {
        let _fd3 = lock.map(|path| self.fx.hand_over(File::open(path).unwrap()));
        self.fx.cpd(&["attempt", "run", ATTEMPT, "--plan", PLAN])
    }

    /// Attempt `api-a1` in `starting` and its lock file, as a starter would leave them: no worktree, no window.
    fn starting(&self) -> PathBuf {
        let lock = self.home().attempt_lock(&PlanId::parse(PLAN).unwrap(), &AttemptId::parse(ATTEMPT).unwrap());
        fs::create_dir_all(lock.parent().unwrap()).unwrap();
        File::create(&lock).unwrap();
        self.fixture(&format!(
            "UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = 'api';
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
               VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', 'starting', 'cpd/pay--api-a1', '{}', 't');",
            self.worktree(ATTEMPT).display()
        ));
        lock
    }
}

// ---------- what the Worker does, and what the tests read ----------

/// What `attempt submit` answered the Worker, run as pi runs it in its window: cwd the worktree, its Plan and its Attempt.
fn submit(live: &Live, id: &str) -> Value {
    live.fx.cpd_with(&live.worktree(id), &["attempt", "submit", id], &[("CPD_PLAN", PLAN), ("CPD_ATTEMPT", id)]).json()
}

/// A Worker that commits `api.txt` and submits, then stays (pi idles).
fn work(live: &Live, id: &str) -> Value {
    live.commit(&live.worktree(id), "api.txt", "api\n", "api");
    submit(live, id)
}

/// `job cancel api --reason <reason>` as the Worker runs it in its window.
fn cancel(live: &Live, id: &str, reason: &str) {
    live.fx.cpd_with(&live.worktree(id), &["job", "cancel", JOB, "--reason", reason], &[("CPD_PLAN", PLAN), ("CPD_ATTEMPT", id)]).ok();
}

/// The Worker's pi ends with `status`: its pane is dead (remain-on-exit keeps the window), as after `exit <status>`.
fn worker_exits(live: &Live, id: &str, status: u8) {
    let window = format!("{PLAN}--{id}");
    let pane = live.fx.panes().into_iter().find(|p| p.window_name == window).unwrap_or_else(|| panic!("no pane in window {window}"));
    let out = live.fx.tmux(&["__ctl", "pane-exit", &pane.pane, "--status", &status.to_string()]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

/// [`Live::drive`] with what `job dispatch` printed: `(its output, what the driver printed)`.
fn drive_with_out(live: &Live, key: &str, hook: impl FnMut(&Live, &str, usize)) -> (Value, Run) {
    drive_with_out_as(live, key, false, hook)
}

/// [`drive_with_out`] for a parallel Attempt ([`Live::drive_parallel`]).
fn drive_parallel_with_out(live: &Live, key: &str, hook: impl FnMut(&Live, &str, usize)) -> (Value, Run) {
    drive_with_out_as(live, key, true, hook)
}

fn drive_with_out_as(live: &Live, key: &str, parallel: bool, mut hook: impl FnMut(&Live, &str, usize)) -> (Value, Run) {
    let out = live.dispatch(key).ok();
    let id = out["attempt"].as_str().unwrap_or_else(|| panic!("no attempt in {out}")).to_owned();
    if parallel {
        live.make_parallel(&id);
    }
    let launch = live.fx.take_launch(live.fx.fakes.spawn.launches().len() - 1);
    let home = CpdHome::resolve(Some(live.fx.env.cpd_home().into()), None).unwrap();
    let _fd3 = live.fx.hand_over(File::open(home.attempt_lock(&PlanId::parse(PLAN).unwrap(), &AttemptId::parse(&id).unwrap())).unwrap());
    let ran = live.fx.run_launch_with(&launch, |_, nap| hook(live, &id, nap));
    assert_eq!(ran.exit, 0, "{}", ran.stdout);
    (out, ran)
}

/// The driver's log of Attempt `id`, for a failure's message.
fn log(live: &Live, id: &str) -> String {
    fs::read_to_string(live.fx.env.cpd_home().join(format!("run/pay/logs/attempt-{id}.log"))).unwrap_or_default()
}

/// The Events about Attempt `id` or its Worker, in order.
fn events(live: &Live, id: &str) -> Vec<Value> {
    let session: Option<String> = live.value(&format!("SELECT worker_session FROM attempts WHERE plan = 'pay' AND id = '{id}'"));
    let all = live.fx.cpd(&["events", "list", "--plan", PLAN]).ok()["events"].as_array().unwrap().clone();
    all.into_iter().filter(|e| e["attempt"] == id || (session.is_some() && e["session"].as_str() == session.as_deref())).collect()
}

/// The kinds of [`events`], in order.
fn kinds(live: &Live, id: &str) -> Vec<String> {
    events(live, id).iter().map(|e| e["kind"].as_str().unwrap().to_owned()).collect()
}

fn event(live: &Live, id: &str, kind: &str) -> Value {
    events(live, id).into_iter().find(|e| e["kind"] == kind).unwrap_or_else(|| panic!("no {kind} for {id}"))
}

/// The names of the windows of the fake tmux server, one per line.
fn windows(live: &Live) -> String {
    let state = live.fx.tmux_state().expect("the fake tmux server");
    state.windows().into_iter().map(|w| w.name).collect::<Vec<_>>().join("\n")
}

/// Sets the `[hooks]` of the repo `acme/pay`'s policy file: `text` is written as a `.config/wt.toml` was
/// (`pre-start = "cmd"`, `[pre-merge]`), and an empty commit is made on the Lead Branch, where the Attempt worktrees are cut from.
fn set_hooks(live: &Live, text: &str) {
    let file = format!("[hooks]\n{text}").replace("\n[", "\n[hooks.");
    live.fx.env.repo_policy("acme", "pay", &file);
    live.fx.git(&live.lead, &["commit", "-q", "--allow-empty", "-m", "hooks"]);
}

/// A `Runner` over the fake one that runs `then`, once, right after the first call of `program` whose argv `when` accepts and that
/// exited 0: what a program does by itself after its work (git's `post-rewrite` hook after a rebase) or a hook's own command (a
/// `pre-start` hook that runs `cpd job cancel`). `then` runs on the thread that made the call, on the ports under this runner.
struct After {
    inner: Arc<dyn Runner>,
    program: &'static str,
    when: Box<dyn Fn(&[String]) -> bool + Send + Sync>,
    then: Box<dyn Fn() + Send + Sync>,
    fired: AtomicBool,
}

impl After {
    /// The ports of `live` with a runner that runs `cpd <command>` after the first `program` call `when` accepts.
    fn install(live: &mut Live, program: &'static str, when: impl Fn(&[String]) -> bool + Send + Sync + 'static, command: &'static [&'static str]) {
        let inner = live.fx.fakes.ports.runner.clone();
        let ports = Ports { runner: inner.clone(), ..live.fx.fakes.ports.clone() };
        let argv: Vec<OsString> = std::iter::once("cpd").chain(command.iter().copied()).map(OsString::from).collect();
        let then = move || {
            let mut out = Vec::new();
            cpd::run_with(&ports, argv.clone(), &mut out);
            let out: Value = serde_json::from_slice(&out).expect("cpd printed JSON");
            assert_eq!(out["ok"], true, "{out}");
        };
        let after = After { inner, program, when: Box::new(when), then: Box::new(then), fired: AtomicBool::new(false) };
        live.fx.fakes.ports.runner = Arc::new(after);
    }

    fn after(&self, program: &str, args: &[String], result: &Result<Output, Failed>) {
        let name = Path::new(program).file_name().and_then(|n| n.to_str());
        let ok = result.as_ref().is_ok_and(|out| out.status.success());
        if ok && name == Some(self.program) && (self.when)(args) && !self.fired.swap(true, Ordering::SeqCst) {
            (self.then)();
        }
    }
}

impl Runner for After {
    fn output(&self, cmd: &mut Command, tool: &str, deadline: Duration) -> Result<Output, Failed> {
        let result = self.inner.output(cmd, tool, deadline);
        let args: Vec<String> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        self.after(&cmd.get_program().to_string_lossy(), &args, &result);
        result
    }

    fn output_unbounded(&self, cmd: &mut Command, tool: &str) -> std::io::Result<Output> {
        self.inner.output_unbounded(cmd, tool)
    }

    fn run(&self, call: &Call) -> Result<Output, Failed> {
        let result = self.inner.run(call);
        self.after(&call.program, &call.args, &result);
        result
    }
}

// ---------- job-merged's data.turn (contract §5.2, hs-d514) ----------

/// A `draft` Job `key` in PR `pr` of Plan `pay`.
fn add_draft(live: &Live, pr: &str, key: &str) {
    fixture(
        &live.fx.env,
        &format!(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
             VALUES ('pay', '{pr}', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 0, cpd_txn(), 't');"
        ),
    );
}

/// `data` of the one `job-merged` Event of Job `key`.
fn merged_data(live: &Live, key: &str) -> Value {
    let all: Vec<Value> = store(&live.fx.env)
        .read(|c| {
            let mut stmt = c.prepare("SELECT data FROM events WHERE plan = 'pay' AND job = ?1 AND kind = 'job-merged'")?;
            let rows = stmt.query_map([key], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows.iter().map(|d| serde_json::from_str(d).unwrap()).collect())
        })
        .unwrap();
    assert_eq!(all.len(), 1, "one job-merged for {key}: {all:?}");
    all[0].clone()
}

#[test]
fn job_merged_does_not_turn_while_other_jobs_run() {
    let live = live("");
    add_draft(&live, "repo", "later");
    add_job_in(&live.fx.env, "repo", "web", &[]); // queued: the Plan still has work in flight
    let (attempt, _) = live.run_job(JOB, work);
    assert_eq!(attempt["outcome"], "merged", "{attempt}");
    assert_eq!(merged_data(&live, JOB)["turn"], json!(false), "{}", merged_data(&live, JOB));
}

// ---------- PRs across repos (R34) ----------

#[test]
fn dispatch_refuses_a_job_of_a_pr_that_has_not_started() {
    let fx = rows();
    fx.fixture(&format!(
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('pay', 'web', '{}', 'main', 'waiting', 'cpd/pay/web', cpd_txn(), 't');",
        fx.repo.display()
    ));
    fx.add_job_in("web", "ui", &[]);
    let before = fx.txns();

    let run = fx.dispatch("ui");

    refused(&run, "job_not_ready", 3);
    assert_eq!(run.error()["details"]["pr"], "web");
    assert_eq!(run.error()["details"]["pr_status"], "waiting");
    assert_eq!(fx.txns(), before);
    assert_eq!(fx.job_status("ui"), "queued");
    // The first PR's Job is unaffected.
    assert_eq!(fx.job_status(JOB), "queued");
}

// ---------- a busy store (contract §3.4, §4.2) ----------

// ---------- failures ----------

#[test]
fn a_merge_conflict_ends_failed_and_the_next_attempt_retries_the_same_tier_with_the_files() {
    let live = live("");
    // The Worker edits README.md while another Job's merge edits it on the Lead Branch.
    let (_, ran) = drive_parallel_with_out(&live, JOB, |live, id, nap| {
        if nap == 1 {
            live.commit(&live.lead, "README.md", "theirs\n", "other");
            live.commit(&live.worktree(id), "README.md", "ours\n", "ours");
            submit(live, id);
        }
    });
    let attempt = live.attempt(ATTEMPT);

    assert_eq!(ran.ok()["outcome"], "failed");
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("failed"), &json!("merge_conflict")));
    assert_eq!(live.job_status(JOB), "queued");
    let retry = event(&live, ATTEMPT, "attempt-retry");
    assert_eq!(retry["data"]["next_tier"], "S2");
    assert!(retry["data"]["feedback"].as_str().unwrap().contains("README.md"), "{retry}");
    // Failed Attempts keep their worktree for debugging.
    assert!(live.worktree(ATTEMPT).exists());

    // The next Attempt is at the same Tier and its Spec names the files; its Worker's pi ends without submitting.
    let (next, _) = drive_with_out(&live, JOB, |live, id, nap| {
        if nap == 1 {
            worker_exits(live, id, 4);
        }
    });
    assert_eq!((&next["attempt"], &next["tier"], &next["origin"]), (&json!("api-a2"), &json!("S2"), &json!("ladder")));
    assert_eq!(live.attempt("api-a2")["cause"], "crashed");
    let spec = fs::read_to_string(live.fx.env.cpd_home().join("run/pay/specs/api-a2.md")).unwrap();
    assert!(spec.contains("### Attempt 1 (Tier `S2`)"), "{spec}");
    assert!(spec.contains("- Cause: merge_conflict"), "{spec}");
    assert!(spec.contains("  - `README.md`"), "{spec}");
}

fn kill_window_calls(live: &Live) -> usize {
    live.fx.calls("tmux").iter().filter(|c| c.args.iter().any(|a| a == "kill-window")).count()
}

#[test]
fn a_failed_kill_window_is_retried_and_appends_no_error() {
    let live = live("");
    // Fails the first `kill-window` only; every other call is the fake tmux's.
    (*live.fx.fakes.runner).clone().once("tmux", |c| c.args.iter().any(|a| a == "kill-window"), Reply::exit(1, "", "fake: kill-window failed"));

    // The Worker's pi exits 3 at once; its pane stays, dead.
    let (id, _) = live.drive(JOB, |live, id, nap| {
        if nap == 1 {
            worker_exits(live, id, 3);
        }
    });

    assert_eq!(kill_window_calls(&live), 2, "the first kill-window failed and was tried again");
    assert!(!windows(&live).contains("pay--api-a1"), "{}", windows(&live));
    assert!(!kinds(&live, &id).contains(&"error".to_owned()), "{:?}", kinds(&live, &id));
}

fn list_panes_calls(live: &Live) -> usize {
    live.fx.calls("tmux").iter().filter(|argv| argv.args.iter().any(|a| a == "list-panes")).count()
}

/// engine-W8 (§6.3 liveness): one tmux hiccup, the server answering neither the first
/// `list-panes` nor the `list-sessions` after it, is not "the pane is gone": the driver
/// lists again at the next poll and the Worker keeps running.
#[test]
fn one_failed_list_panes_does_not_end_a_running_attempt_crashed() {
    let live = live("");
    let (fired, answered) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    let (first, second) = (fired.clone(), (fired.clone(), answered.clone()));
    let runner = (*live.fx.fakes.runner).clone();
    let sub = |c: &Call, name: &str| c.args.iter().any(|a| a == name);
    runner
        .on("tmux", move |c| sub(c, "list-panes") && !first.swap(true, Ordering::SeqCst), Reply::exit(1, "", "lost server"))
        .on("tmux", move |c| sub(c, "list-sessions") && second.0.load(Ordering::SeqCst) && !second.1.swap(true, Ordering::SeqCst), Reply::exit(1, "", "lost server"));

    // The Worker's pi just runs. Poll 1 is the one that fails; the next polls list again.
    let (id, _) = live.drive(JOB, |live, id, nap| match nap {
        2 => assert!(answered.load(Ordering::SeqCst), "the hiccup did not happen\nlog: {}", log(live, id)),
        // The poll after the hiccup lists the panes again: the failed one judged nothing.
        4 => {
            assert!(list_panes_calls(live) >= 2);
            // The Worker is still running two polls after the retried listing.
            let attempt = live.attempt(id);
            assert_eq!(attempt["status"], "running", "{attempt}\nlog: {}", log(live, id));
            fixture(&live.fx.env, "UPDATE attempts SET stop_request = 'timed_out' WHERE plan = 'pay' AND id = 'api-a1';");
        }
        _ => {}
    });

    assert_eq!(live.attempt(&id)["cause"], "timed_out", "log: {}", log(&live, &id));
}

#[test]
fn a_failing_pre_start_ends_setup_failed_and_fails_the_job() {
    let live = live("");
    set_hooks(&live, "pre-start = \"exit 7\"\n");
    live.fx.fake("sh", Reply::exit(7, "", ""));

    let (id, _) = live.drive_parallel(JOB, |_, _, _| {});
    let attempt = live.attempt(&id);

    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("failed"), &json!("setup_failed")));
    assert_eq!(live.job_status(JOB), "failed");
    assert_eq!(kinds(&live, ATTEMPT), ["attempt-started", "job-failed"]);
    assert!(live.fx.calls("pi").is_empty());
    assert!(!live.fx.calls("tmux").iter().any(|c| c.args.iter().any(|a| a == "new-window")), "no Worker window was made");
    assert!(!live.worktree(ATTEMPT).exists());
}

#[test]
fn a_timed_out_stop_request_ends_it_and_fails_the_job() {
    let live = live("");

    let (id, _) = live.drive(JOB, |live, _, nap| {
        if nap == 1 {
            fixture(&live.fx.env, "UPDATE attempts SET stop_request = 'timed_out' WHERE plan = 'pay' AND id = 'api-a1';");
        }
    });
    let attempt = live.attempt(&id);

    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("failed"), &json!("timed_out")));
    assert_eq!(live.job_status(JOB), "failed");
    assert!(!windows(&live).contains("pay--api-a1"), "{}", windows(&live));
}

#[test]
fn an_idle_worker_is_nudged_once_then_ends_no_submit() {
    let live = live("");
    let msg = r#"{"type":"message","timestamp":"2026-10-14T21:00:05.000Z","message":{"role":"assistant","usage":{"input":1,"output":1,"cacheRead":0,"cacheWrite":0,"cost":{"total":0}},"stopReason":"stop"}}"#;
    let state = |live: &Live, sid: &str, state: &str| {
        live.fx.cpd(&["session", "state", state, "--session", sid]).ok();
    };

    let (id, _) = live.drive(JOB, |live, id, nap| {
        let sid: String = live.value("SELECT id FROM sessions WHERE role = 'worker'");
        let dir = live.fx.env.home().join(".pi/agent/sessions/w");
        let file = dir.join(format!("2026_{sid}.jsonl"));
        match nap {
            // pi's first turn ends: its session file shows it and its state is idle.
            1 => {
                fs::create_dir_all(&dir).unwrap();
                fs::write(&file, format!("{}\n{msg}\n", r#"{"type":"session","timestamp":"2026-10-14T21:00:00.000Z"}"#)).unwrap();
                state(live, &sid, "idle");
            }
            // The nudge came (the poll after it); pi answers it with a second turn and is idle again.
            2 => {
                assert!(events(live, id).iter().any(|e| e["kind"] == "attempt-nudge"), "{:?}", kinds(live, id));
                state(live, &sid, "working");
                let mut text = fs::read_to_string(&file).unwrap();
                text += &format!("{msg}\n");
                fs::write(&file, text).unwrap();
                state(live, &sid, "idle");
            }
            _ => {}
        }
    });
    let attempt = live.attempt(&id);

    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("failed"), &json!("no_submit")), "{}", log(&live, &id));
    assert_eq!(attempt["nudged"], true);
    let nudge = event(&live, ATTEMPT, "attempt-nudge");
    assert_eq!(nudge["session"], attempt["worker_session"]);
    assert!(nudge["data"]["prompt"].as_str().unwrap().starts_with("You ended your turn without running cpd attempt submit api-a1."));
    assert_eq!(kinds(&live, ATTEMPT).iter().filter(|k| *k == "attempt-nudge").count(), 1);
    assert_eq!(event(&live, ATTEMPT, "attempt-retry")["data"]["cause"], "no_submit");
}

// ---------- cancel at each safe point ----------

fn assert_cancelled(live: &Live, attempt: &Value, reason: &str) {
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("cancelled"), &json!("cancel_requested")), "{}", log(live, ATTEMPT));
    assert_eq!(live.job_status(JOB), "cancelled");
    assert_eq!(event(live, ATTEMPT, "job-cancelled")["data"], json!({"reason": reason}));
}

#[test]
fn a_cancel_before_a3_ends_it_before_the_worker_starts() {
    let mut live = live("");
    // pre-start runs in A2: the request is there at the safe point before A3.
    let hook = "cpd job cancel api --reason early --plan pay";
    set_hooks(&live, &format!("pre-start = \"{hook}\"\n"));
    After::install(&mut live, "sh", move |args| args.iter().any(|a| a == hook), &["job", "cancel", JOB, "--reason", "early", "--plan", PLAN]);

    let (id, ran) = live.drive_parallel(JOB, |_, _, _| {});
    let attempt = live.attempt(&id);

    assert_eq!(ran.ok()["outcome"], "cancelled");
    assert_cancelled(&live, &attempt, "early");
    assert!(attempt["worker_session"].is_null());
    assert!(live.fx.calls("pi").is_empty());
    assert_eq!(kinds(&live, ATTEMPT), ["attempt-started", "job-cancel-requested", "job-cancelled"]);
}

#[test]
fn a_cancel_before_the_rebase_ends_it_without_merging() {
    let live = live("");
    let old = live.lead_sha();

    let (id, _) = live.drive_parallel(JOB, |live, id, nap| {
        if nap == 1 {
            live.commit(&live.worktree(id), "api.txt", "api\n", "api");
            cancel(live, id, "mid");
            submit(live, id);
        }
    });

    assert_cancelled(&live, &live.attempt(&id), "mid");
    assert_eq!(live.lead_sha(), old);
    assert!(kinds(&live, ATTEMPT).contains(&"attempt-status".to_owned()));
}

#[test]
fn a_cancel_before_the_compare_and_swap_ends_it_without_merging() {
    let mut live = live("");
    // The request arrives during the rebase, after the first safe point: git's `post-rewrite` hook runs `job cancel` once the rebase
    // has rewritten the Worker's commit.
    After::install(&mut live, "git", |args| args.iter().any(|a| a == "rebase"), &["job", "cancel", JOB, "--reason", "late", "--plan", PLAN]);

    let (id, _) = live.drive_parallel(JOB, |live, id, nap| {
        if nap == 1 {
            // Another Job merges first, so the rebase rewrites the Worker's commit.
            live.fx.git(&live.lead, &["commit", "-q", "--allow-empty", "-m", "other"]);
            work(live, id);
        }
    });

    assert_cancelled(&live, &live.attempt(&id), "late");
    let lead = live.lead_sha();
    assert_eq!(live.fx.git(&live.repo, &["log", "-1", "--format=%s", &lead]), "other");
}

// ---------- the driver lock ----------

#[test]
fn a_driver_without_the_lock_as_fd_3_exits_without_writing() {
    let fx = rows();
    fx.starting();
    let before = fx.txns();

    // fd 3 not open.
    let run = fx.run_driver(None);
    assert_eq!(run.exit, 1, "{run:?}");
    assert_eq!(run.error()["code"], "driver_lock_missing");

    // fd 3 is another file.
    let other = fx.fx.env.root().join("other.lock");
    fs::write(&other, "").unwrap();
    let run = fx.run_driver(Some(&other));
    assert_eq!(run.exit, 1, "{run:?}");
    let error = run.error();
    assert_eq!(error["code"], "driver_lock_missing");
    assert!(error["message"].as_str().unwrap().contains("another file"), "{error}");

    assert_eq!(fx.txns(), before);
    assert_eq!(fx.attempt(ATTEMPT)["status"], "starting");
    assert!(!fx.worktree(ATTEMPT).exists());
}

#[test]
fn a_driver_refuses_an_attempt_that_is_not_starting() {
    let fx = rows();
    let lock = fx.starting();
    fx.fixture("UPDATE attempts SET status = 'running' WHERE plan = 'pay' AND id = 'api-a1';");
    let before = fx.txns();

    let run = fx.run_driver(Some(&lock));

    assert_eq!(run.exit, 3, "{run:?}");
    assert_eq!(run.error()["code"], "illegal_transition");
    assert_eq!(fx.txns(), before);
}

/// hs-n278: a driver that ends on a store newer than its binary (an install raced a migration) leaves the marker
/// E2 reads, so the Attempt is not its Worker's failure. It ends no Attempt itself.
#[test]
fn a_driver_ending_on_store_too_new_leaves_the_infra_marker_and_ends_nothing() {
    let fx = rows();
    let lock = fx.starting();
    // Above any user_version this binary knows (contract §4.5).
    rusqlite::Connection::open(fx.home().db()).unwrap().pragma_update(None, "user_version", 4999).unwrap();
    let plan = PlanId::parse(PLAN).unwrap();
    let attempt = AttemptId::parse(ATTEMPT).unwrap();
    let marker = fx.home().attempt_infra(&plan, &attempt);
    assert!(!marker.exists());

    let run = fx.run_driver(Some(&lock));

    assert_eq!(run.error()["code"], "store_too_new", "{run:?}");
    assert_eq!(fx.home().infra(&plan, &attempt).unwrap().map(|(infra, _)| infra.code()), Some("store_too_new"));
}

/// Another failure of the driver leaves no marker: the ladder counts it.
#[test]
fn a_driver_refusing_for_another_reason_leaves_no_infra_marker() {
    let fx = rows();
    let lock = fx.starting();
    fx.fixture("UPDATE attempts SET status = 'running' WHERE plan = 'pay' AND id = 'api-a1';");

    let run = fx.run_driver(Some(&lock));

    assert_eq!(run.error()["code"], "illegal_transition", "{run:?}");
    let marker = fx.home().attempt_infra(&PlanId::parse(PLAN).unwrap(), &AttemptId::parse(ATTEMPT).unwrap());
    assert!(!marker.exists());
}

// ---------- job dispatch refusals ----------

#[test]
fn dispatch_refuses_a_job_that_is_not_ready() {
    let fx = rows();
    fx.add_job("ui", &["api"]);
    let before = fx.txns();

    refused(&fx.dispatch("ui"), "job_not_ready", 3);
    refused(&fx.dispatch("nope"), "job_not_found", 4);
    assert_eq!(fx.txns(), before);
}

#[test]
fn dispatch_refuses_a_job_whose_fingerprint_does_not_match() {
    let fx = rows();
    fx.fixture(&format!("UPDATE jobs SET fingerprint = '{}' WHERE plan = 'pay' AND key = 'api';", "0".repeat(64)));
    refused(&fx.dispatch(JOB), "locked_job_tampered", 3);
    assert_eq!(fx.job_status(JOB), "queued");
}

#[test]
fn dispatch_refuses_when_the_driver_lock_is_held() {
    let fx = rows();
    let lock = fx.home().attempt_lock(&PlanId::parse(PLAN).unwrap(), &AttemptId::parse(ATTEMPT).unwrap());
    fs::create_dir_all(lock.parent().unwrap()).unwrap();
    let held = OpenOptions::new().create(true).truncate(false).write(true).open(&lock).unwrap();
    held.try_lock().unwrap();
    let before = fx.txns();

    refused(&fx.dispatch(JOB), "cas_lost", 3);
    assert_eq!(fx.txns(), before);
    assert_eq!(fx.job_status(JOB), "queued");
}

#[test]
fn dispatch_refuses_when_no_slot_is_free() {
    let fx = rows_with("concurrency = 1");
    // The live Attempt is of another PR: a sibling of `api`'s own PR would make the dispatch (always linear) lose, `cas_lost`,
    // before the slots are counted (hs-d683).
    fx.fixture(&format!(
        "INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at)
           VALUES ('pay', 'web', '{}', 'web', 'main', 'dispatch', 'cpd/pay/web', '{}', 'Web', 'config', cpd_txn(), 't', 't');",
        fx.repo.display(),
        fx.repo.with_file_name("pay.web").display()
    ));
    fx.add_job_in("web", "ui", &[]);
    fx.fixture(
        "UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = 'ui';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
           VALUES ('pay', 'ui-a1', 'ui', 1, 'S2', 'ladder', 'running', 'cpd/pay--ui-a1', '/w', 't');",
    );

    refused(&fx.dispatch(JOB), "no_slot", 3);
    assert_eq!(fx.job_status(JOB), "queued");
    assert_eq!(fx.fx.env.query::<i64>("SELECT count(*) FROM attempts WHERE job = 'api'"), 0);
}

#[test]
fn dispatch_after_job_retry_starts_a_retry_attempt_at_the_chosen_tier() {
    let live = live("");
    fixture(
        &live.fx.env,
        "UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = 'api';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
           VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', 'running', 'cpd/pay--api-a1', '/w', 't');
         UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'timed_out', ended_at = 't' WHERE id = 'api-a1';
         UPDATE jobs SET status = 'failed', failed_cause = 'timed_out' WHERE plan = 'pay' AND key = 'api';",
    );
    live.fx.cpd(&["job", "retry", JOB, "--tier", "S3", "--plan", PLAN]).ok();

    // The Worker's pi exits 0 at once.
    let (out, _) = drive_with_out(&live, JOB, |live, id, nap| {
        if nap == 1 {
            worker_exits(live, id, 0);
        }
    });

    assert_eq!((&out["attempt"], &out["tier"], &out["origin"]), (&json!("api-a2"), &json!("S3"), &json!("retry")));
    assert_eq!(live.attempt("api-a2")["origin"], "retry");
    assert_eq!(event(&live, "api-a2", "attempt-started")["data"], json!({"number": 2, "tier": "S3", "origin": "retry"}));
}

// ---------- the update Job ----------

impl Rows {
    /// PR `repo` as `pr open` leaves it, the pull request open (812), the Plan in `deploy`, Job `api` merged and no `open` Job:
    /// the policy `pr.gate = "local"` is the owner's.
    fn open_pr(&self) {
        self.fx.env.owner_policy("acme", "[pr]\ngate = \"local\"\n");
        self.fixture(&format!(
            "UPDATE jobs SET status = 'running' WHERE key = 'api'; UPDATE jobs SET status = 'merged', merged_sha = '{}' WHERE key = 'api';
             INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, mergeable, merge_state, created_at, merged_at, changed_at)
               VALUES ('acme/pay', 812, '{PR_URL}', '{PR_TITLE}', 0, 'open', 'none', 'none', 'mergeable', 'clean', 'opened', NULL, 'opened');
             UPDATE workflows SET status = 'open', pr_number = 812, pr_url = '{PR_URL}', pr_opened_at = 'opened', pull_request = (SELECT id FROM pull_requests) WHERE key = 'repo';
             UPDATE plans SET phase = 'deploy' WHERE id = 'pay';",
            "1".repeat(40)
        ));
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
}

