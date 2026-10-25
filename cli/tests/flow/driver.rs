//! The Attempt driver (contract §3.4; commands.md §5.5, §6.4): `job dispatch`
//! starts the driver detached with the driver lock as fd 3, and the driver runs
//! A1–A9 against a temporary repo, a private tmux server and a fake pi Worker.
//!
//! There is no Reviewer (R19, Escalation rs-driver-a1-q1): a submitted Attempt
//! goes straight to the merge.
//!
//! The `dispatch_refuses_*` tests refuse before any worktree, window or process exists: they run the command tree
//! in this process (`Fx`, [`Rows`]) against rows alone, with no repo, no tmux server and no fake pi.

use crate::common;

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::fx::Fx as InProcess;
use common::{CPD, Run, SLEEP_SECONDS, TestEnv, refused, write_executable};
use cpd_core::error::CpdError;
use cpd_core::git::{Setup, create_lead_worktree};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::plan::{jobs, locked_fingerprint};
use cpd_core::store::{Store, WriteTxn};
use serde_json::{Value, json};

const PLAN: &str = "pay";
const JOB: &str = "api";
/// The stored title of PR `repo`; the shape it is given is `config` (its Jobs are `work` Jobs).
const PR_TITLE: &str = "Charge the card at checkout";
const PR_URL: &str = "https://github.com/acme/pay/pull/812";
const ATTEMPT: &str = "api-a1";

/// The driver's polls as these tests set them (contract §1.2 "Test overrides"): A4's look at the Worker
/// (`DRIVER_POLL`, 0.5 s) and A9's pause between `kill-window` tries (`KILL_PAUSE`, 1 s).
const POLL: Duration = Duration::from_millis(50);
const DRIVER_ENV: [(&str, &str); 2] = [("CPD_TEST_DRIVER_POLL", "0.05"), ("CPD_TEST_KILL_PAUSE", "0.05")];

/// O supports efforts 0–5, S 1–5, H 0–3.
const EFFORTS: [(&str, &str, &str); 3] =
    [("O", "p/opus", "[0,1,2,3,4,5]"), ("S", "p/sonnet", "[1,2,3,4,5]"), ("H", "p/haiku", "[0,1,2,3]")];

struct Fx {
    env: TestEnv,
    repo: PathBuf,
    lead: PathBuf,
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

/// A repo `pay` with the Lead worktree of Plan `pay` (phase `dispatch`), a
/// config on this env's tmux server, a fake pi install whose Tier catalog is
/// seeded in the cache, and Job `api` (`queued`, Tier S2, Locked).
fn fx() -> Fx {
    fx_with("")
}

fn fx_with(config_extra: &str) -> Fx {
    let env = TestEnv::new();
    let repo = env.git_repo("pay").canonicalize().unwrap();
    // git runs in this process too, whose environment is not the TestEnv's.
    for (key, value) in [("user.name", "cpd test"), ("user.email", "test@cpd.invalid"), ("commit.gpgsign", "false")] {
        env.git(&repo, &["config", key, value]);
    }
    let setup = Setup { timeout: Duration::from_secs(30), log: env.root().join("lead.log") };
    let lead = create_lead_worktree(&repo, &PlanId::parse(PLAN).unwrap(), "main", &setup).unwrap().path;
    seed_home(&env, config_extra);
    // The work session as the engine leaves it: its `watch` window, and
    // remain-on-exit (§6.1) so a dead Worker pane stays to be read.
    for args in [&["new-session", "-d", "-s", "cpd-work", "-n", "watch", "sleep", "600"][..], &["set", "-g", "remain-on-exit", "on"]] {
        assert!(env.tmux(args).status.success(), "tmux {args:?}");
    }
    seed_rows(&env, &repo, &lead);
    let fx = Fx { env, repo, lead };
    fx.pi("exit 0");
    fx.add_job(JOB, &[]);
    fx
}

/// The config on this env's tmux server (`config_extra` is more TOML), a fake pi install whose Tier catalog is
/// seeded in the cache by [`seed_rows`], and the policy file of the repo `acme/pay`.
fn seed_home(env: &TestEnv, config_extra: &str) {
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\n{config_extra}\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n[tmux]\nsocket = \"{}\"\n",
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
fn seed_rows(env: &TestEnv, repo: &Path, lead: &Path) {
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
         INSERT INTO prs (plan, key, repo, name, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '{repo}', 'pay', 'main', 'dispatch', 'cpd/pay', '{lead}', '{PR_TITLE}', 'config', cpd_txn(), 't', 't');",
            repo = repo.display(),
            lead = lead.display()
        ),
    );
}

fn store(env: &TestEnv) -> Store {
    Store::open(&CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap(), 5000).unwrap()
}

fn fixture(env: &TestEnv, sql: &str) {
    store(env).write("fixture", |tx| exec(tx, sql)).unwrap();
}

/// Job `key` of PR `pr` of Plan `pay`, queued with its Locked-Job fingerprint.
fn add_job_in(env: &TestEnv, pr: &str, key: &str, needs: &[&str]) {
    let mut sql = format!(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
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

    /// `job dispatch <key>` with [`DRIVER_ENV`], in this process.
    fn dispatch(&self, key: &str) -> Run {
        self.fx.cpd_with(self.fx.env.root(), &["job", "dispatch", key, "--plan", PLAN], &DRIVER_ENV)
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

impl Fx {
    fn store(&self) -> Store {
        store(&self.env)
    }

    fn fixture(&self, sql: &str) {
        fixture(&self.env, sql);
    }

    /// Job `key` of PR `repo` of Plan `pay`, queued with its Locked-Job fingerprint.
    fn add_job(&self, key: &str, needs: &[&str]) {
        self.add_job_in("repo", key, needs);
    }

    /// [`Fx::add_job`] in PR `pr`.
    fn add_job_in(&self, pr: &str, key: &str, needs: &[&str]) {
        add_job_in(&self.env, pr, key, needs);
    }

    /// The fake `pi`: `body` runs in the Worker's pane, cwd the worktree, with
    /// `$CPD` the binary and `$LEAD` the Lead worktree, once the Attempt is
    /// `running` (the driver records that right after the pane starts; a real
    /// pi takes far longer to boot than this fake).
    fn pi(&self, body: &str) {
        self.env.fake_pi(&format!(
            "CPD='{CPD}'\nLEAD='{}'\n\
             until \"$CPD\" attempt show \"$CPD_ATTEMPT\" | grep -q '\"status\":\"running\"'; do sleep 0.1; done\n{body}",
            self.lead.display()
        ));
    }

    /// Sets the `[hooks]` of the repo `acme/pay`'s policy file: `text` is written as a `.config/wt.toml` was
    /// (`pre-start = "cmd"`, `[pre-merge]`), and an empty commit is made on the Lead Branch, where the Attempt worktrees are cut from.
    fn hooks(&self, text: &str) {
        let file = format!("[hooks]\n{text}").replace("\n[", "\n[hooks.");
        self.env.repo_policy("acme", "pay", &file);
        self.env.git(&self.lead, &["commit", "-q", "--allow-empty", "-m", "hooks"]);
    }

    /// `cpd <args>` with this env's CPD_HOME, as a hook or the Worker calls it.
    fn cpd_line(&self, args: &str) -> String {
        format!("CPD_HOME='{}' '{CPD}' {args}", self.env.cpd_home().display())
    }

    /// `job dispatch <key>`, its driver with [`DRIVER_ENV`].
    fn dispatch(&self, key: &str) -> Run {
        let mut cmd = self.env.command(CPD);
        cmd.envs(DRIVER_ENV).args(["job", "dispatch", key, "--plan", PLAN]).current_dir(self.env.root());
        let out = common::output(&mut cmd).expect("run cpd");
        Run {
            exit: out.status.code().expect("cpd was killed by a signal"),
            stdout: String::from_utf8(out.stdout).expect("stdout is UTF-8"),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        }
    }

    fn attempt(&self, id: &str) -> Value {
        self.env.cpd(&["attempt", "show", id, "--plan", PLAN]).ok()["attempt"].clone()
    }

    fn job_status(&self, key: &str) -> String {
        self.env.query(&format!("SELECT status FROM jobs WHERE plan = 'pay' AND key = '{key}'"))
    }

    /// Waits until Attempt `id` is `status` in the store (at most 60 s); returns it as `attempt show` then gives it.
    fn wait_status(&self, id: &str, status: &str) -> Value {
        common::wait_some(&format!("Attempt {id} to become {status}"), Duration::from_secs(60), || {
            let now: Option<String> = self.env.query(&format!("SELECT (SELECT status FROM attempts WHERE plan = '{PLAN}' AND id = '{id}')"));
            if now.as_deref() == Some(status) { Ok(()) } else { Err(format!("status {now:?}\nlog: {}", self.log(id))) }
        });
        self.attempt(id)
    }

    /// Waits until the Attempt has ended and its driver has exited; returns
    /// the Attempt and the driver's output, the last line of its log.
    fn wait_end(&self, id: &str, pid: &Value) -> (Value, Value) {
        let attempt = self.wait_status(id, "ended");
        let pid = nix::unistd::Pid::from_raw(pid.as_i64().unwrap() as i32);
        common::wait_some(&format!("the driver of {id} to exit"), Duration::from_secs(30), || {
            if nix::sys::signal::kill(pid, None).is_ok() { Err(format!("log: {}", self.log(id))) } else { Ok(()) }
        });
        let log = self.log(id);
        let last = log.lines().last().unwrap_or_else(|| panic!("empty log of {id}"));
        let out: Value = serde_json::from_str(last).unwrap_or_else(|e| panic!("{e}: {log}"));
        (attempt, out)
    }

    fn log(&self, id: &str) -> String {
        fs::read_to_string(self.env.cpd_home().join(format!("run/pay/logs/attempt-{id}.log"))).unwrap_or_default()
    }

    /// The kinds of the Events about Attempt `id` or its Worker, in order.
    fn kinds(&self, id: &str) -> Vec<String> {
        self.events(id).iter().map(|e| e["kind"].as_str().unwrap().to_owned()).collect()
    }

    fn events(&self, id: &str) -> Vec<Value> {
        let session: Option<String> =
            self.env.query(&format!("SELECT worker_session FROM attempts WHERE plan = 'pay' AND id = '{id}'"));
        let all = self.env.cpd(&["events", "list", "--plan", PLAN]).ok()["events"].as_array().unwrap().clone();
        all.into_iter()
            .filter(|e| e["attempt"] == id || (session.is_some() && e["session"].as_str() == session.as_deref()))
            .collect()
    }

    fn event(&self, id: &str, kind: &str) -> Value {
        self.events(id).into_iter().find(|e| e["kind"] == kind).unwrap_or_else(|| panic!("no {kind} for {id}"))
    }

    fn lead_sha(&self) -> String {
        self.env.git(&self.repo, &["rev-parse", "refs/heads/cpd/pay"])
    }

    fn worktree(&self, id: &str) -> PathBuf {
        self.repo.parent().unwrap().join(format!("pay.pay--{id}"))
    }

    fn windows(&self) -> String {
        let out = self.env.tmux(&["list-windows", "-a", "-F", "#{session_name}:#{window_name}"]);
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

/// A Worker that does its work, commits and submits, then stays (pi idles).
const WORKER: &str = r#"sid=""; prev=""
for a in "$@"; do [ "$prev" = "--session-id" ] && sid="$a"; prev="$a"; done
mkdir -p "$HOME/.pi/agent/sessions/w"
f="$HOME/.pi/agent/sessions/w/2026_$sid.jsonl"
printf '%s\n' '{"type":"session","timestamp":"2026-10-14T21:00:00.000Z"}' > "$f"
printf '%s\n' '{"type":"message","timestamp":"2026-10-14T21:00:05.000Z","message":{"role":"assistant","usage":{"input":10,"output":20,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.5}},"stopReason":"stop"}}' >> "$f"
echo api > api.txt
git add -A && git commit -qm "api"
"$CPD" attempt submit "$CPD_ATTEMPT" > ../submit-$CPD_ATTEMPT.json 2>&1
sleep 120"#;

// ---------- the end-to-end run ----------

// ---------- job-merged's data.turn (contract §5.2, hs-d514) ----------

impl Fx {
    /// A `draft` Job `key` in PR `pr` of Plan `pay`.
    fn add_draft(&self, pr: &str, key: &str) {
        self.fixture(&format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
             VALUES ('pay', '{pr}', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 0, cpd_txn(), 't');"
        ));
    }

    /// `data` of the one `job-merged` Event of Job `key`.
    fn merged_data(&self, key: &str) -> Value {
        let all: Vec<Value> = self
            .store()
            .read(|c| {
                let mut stmt = c.prepare("SELECT data FROM events WHERE plan = 'pay' AND job = ?1 AND kind = 'job-merged'")?;
                let rows = stmt.query_map([key], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows.iter().map(|d| serde_json::from_str(d).unwrap()).collect())
            })
            .unwrap();
        assert_eq!(all.len(), 1, "one job-merged for {key}: {all:?}");
        all[0].clone()
    }
}

#[test]
fn job_merged_does_not_turn_while_other_jobs_run() {
    let fx = fx();
    fx.pi(WORKER);
    fx.add_draft("repo", "later");
    fx.add_job("web", &[]); // queued: the Plan still has work in flight
    let attempt = run_job(&fx, JOB);
    assert_eq!(attempt["outcome"], "merged", "{attempt}");
    assert_eq!(fx.merged_data(JOB)["turn"], json!(false), "{}", fx.merged_data(JOB));
}

#[test]
fn a_merged_review_job_turns() {
    let fx = fx();
    // A review Job's Worker is a fork of the Lead: it waits for the Lead's session file to show a finished turn.
    let file = fx.env.home().join(".pi/agent/sessions/lead/2026_lead1.jsonl");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, "{\"type\":\"session\"}\n{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"stopReason\":\"stop\"}}\n").unwrap();
    File::options().write(true).open(&file).unwrap().set_modified(std::time::SystemTime::UNIX_EPOCH).unwrap();
    fx.fixture(&format!(
        "UPDATE sessions SET file = '{}' WHERE id = 'lead1';
         INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, review, created_txn, created_at)
         VALUES ('pay', 'repo', 'final', 'Title of final', 'S2', 'Review it.', 'Because.', '[\"run/pay/reviews/final.md\"]', '[\"findings written\"]', '[]', 'draft', 1, 1, cpd_txn(), 't');",
        file.display()
    ));
    let plan = PlanId::parse(PLAN).unwrap();
    let all = fx.store().read(|conn| jobs::load(conn, &plan)).unwrap();
    let job = all.iter().find(|j| j.key == "final").unwrap();
    fx.fixture(&format!("UPDATE jobs SET status = 'queued', fingerprint = '{}' WHERE plan = 'pay' AND key = 'final';", locked_fingerprint(job)));
    fx.pi(
        r#"mkdir -p "$CPD_HOME/run/pay/reviews"
echo "- one finding" > "$CPD_HOME/run/pay/reviews/final.md"
"$CPD" attempt submit "$CPD_ATTEMPT" > ../submit-$CPD_ATTEMPT.json 2>&1 || exit 4
sleep 120"#,
    );
    let attempt = run_job(&fx, "final");
    assert_eq!(attempt["outcome"], "merged", "{attempt}");
    // No draft Job exists and `api` is queued: only the review makes it a turn.
    assert_eq!(fx.merged_data("final")["turn"], json!(true), "{}", fx.merged_data("final"));
}

// ---------- PRs across repos (R34) ----------

#[test]
fn dispatch_refuses_a_job_of_a_pr_that_has_not_started() {
    let fx = rows();
    fx.fixture(&format!(
        "INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
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
fn a_merge_conflict_ends_failed_and_the_ladder_queues_the_next_attempt_with_the_files() {
    let fx = fx();
    // The Worker edits README.md while another Job's merge edits it on the Lead Branch.
    fx.pi(&format!(
        "if [ \"$CPD_ATTEMPT\" = api-a1 ]; then\n\
           echo theirs > \"$LEAD/README.md\"; git -C \"$LEAD\" commit -qam other\n\
           echo ours > README.md; git commit -qam ours\n\
           \"$CPD\" attempt submit \"$CPD_ATTEMPT\"\n\
           sleep 120\n\
         fi\n\
         exit 4"
    ));

    let out = fx.dispatch(JOB).ok();
    let (attempt, driver) = fx.wait_end(ATTEMPT, &out["pid"]);

    assert_eq!(driver["outcome"], "failed");
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("failed"), &json!("merge_conflict")));
    assert_eq!(fx.job_status(JOB), "queued");
    let retry = fx.event(ATTEMPT, "attempt-retry");
    assert_eq!(retry["data"]["next_tier"], "S3");
    assert!(retry["data"]["feedback"].as_str().unwrap().contains("README.md"), "{retry}");
    // Failed Attempts keep their worktree for debugging.
    assert!(fx.worktree(ATTEMPT).exists());

    // The next Attempt is at the next Tier and its Spec names the files.
    let next = fx.dispatch(JOB).ok();
    assert_eq!((&next["attempt"], &next["tier"], &next["origin"]), (&json!("api-a2"), &json!("S3"), &json!("ladder")));
    let (second, _) = fx.wait_end("api-a2", &next["pid"]);
    assert_eq!(second["cause"], "crashed");
    let spec = fs::read_to_string(fx.env.cpd_home().join("run/pay/specs/api-a2.md")).unwrap();
    assert!(spec.contains("### Attempt 1 (Tier `S2`)"), "{spec}");
    assert!(spec.contains("- Cause: merge_conflict"), "{spec}");
    assert!(spec.contains("  - `README.md`"), "{spec}");
}

#[test]
fn a_failed_kill_window_is_retried_and_appends_no_error() {
    let fx = fx();
    fx.pi("exit 3");
    let real = String::from_utf8(fx.env.command("sh").args(["-c", "command -v tmux"]).output().unwrap().stdout).unwrap();
    let real = real.trim();
    assert!(real.starts_with('/'), "{real}");
    // Fails the first `kill-window` only; every other call is the real tmux.
    let marker = fx.env.root().join("kill-failed");
    fx.env.fake(
        "tmux",
        &format!(
            "for a in \"$@\"; do if [ \"$a\" = kill-window ] && [ ! -e '{m}' ]; then : > '{m}'; echo 'fake: kill-window failed' >&2; exit 1; fi; done\nexec '{real}' \"$@\"",
            m = marker.display()
        ),
    );

    let out = fx.dispatch(JOB).ok();
    fx.wait_end(ATTEMPT, &out["pid"]);

    assert!(marker.exists(), "the fake tmux never failed a kill-window");
    assert!(!fx.windows().contains("pay--api-a1"), "{}", fx.windows());
    assert!(!fx.kinds(ATTEMPT).contains(&"error".to_owned()), "{:?}", fx.kinds(ATTEMPT));
}

/// The real `tmux` on `PATH`, for a fake that wraps it.
fn real_tmux(fx: &Fx) -> String {
    let real = String::from_utf8(fx.env.command("sh").args(["-c", "command -v tmux"]).output().unwrap().stdout).unwrap();
    let real = real.trim().to_owned();
    assert!(real.starts_with('/'), "{real}");
    real
}

/// The `list-panes` calls made so far, through a fake `tmux`.
fn list_panes_calls(fx: &Fx) -> usize {
    fx.env.calls("tmux").iter().filter(|argv| argv.iter().any(|a| a == "list-panes")).count()
}

/// engine-W8 (§6.3 liveness): one tmux hiccup, the server answering neither the first
/// `list-panes` nor the `list-sessions` after it, is not "the pane is gone": the driver
/// lists again at the next poll and the Worker keeps running.
#[test]
fn one_failed_list_panes_does_not_end_a_running_attempt_crashed() {
    let fx = fx();
    fx.pi("sleep 120");
    let (fired, answered) = (fx.env.root().join("list-panes-failed"), fx.env.root().join("list-sessions-failed"));
    fx.env.fake(
        "tmux",
        &format!(
            "case \" $* \" in\n\
               *\" list-panes \"*) [ -e '{f}' ] || {{ : > '{f}'; echo 'lost server' >&2; exit 1; }};;\n\
               *\" list-sessions \"*) if [ -e '{f}' ] && [ ! -e '{a}' ]; then : > '{a}'; echo 'lost server' >&2; exit 1; fi;;\n\
             esac\nexec '{real}' \"$@\"",
            f = fired.display(),
            a = answered.display(),
            real = real_tmux(&fx)
        ),
    );

    let out = fx.dispatch(JOB).ok();
    fx.wait_status(ATTEMPT, "running");
    common::wait_some("the hiccup", Duration::from_secs(30), || if answered.exists() { Ok(()) } else { Err(format!("log: {}", fx.log(ATTEMPT))) });
    // The poll after the hiccup lists the panes again: the failed one judged nothing.
    common::wait_until("the driver to list the panes again", || list_panes_calls(&fx) >= 2);
    // time-claim: the Worker is still running two polls after the retried listing; no per-poll signal from the driver
    // once it has the pane's pid (it forks nothing).
    std::thread::sleep(POLL * 2);
    let attempt = fx.attempt(ATTEMPT);
    assert_eq!(attempt["status"], "running", "{attempt}\nlog: {}", fx.log(ATTEMPT));

    fx.fixture("UPDATE attempts SET stop_request = 'timed_out' WHERE plan = 'pay' AND id = 'api-a1';");
    let (attempt, _) = fx.wait_end(ATTEMPT, &out["pid"]);
    assert_eq!(attempt["cause"], "timed_out", "log: {}", fx.log(ATTEMPT));
}

#[test]
fn a_failing_pre_start_ends_setup_failed_and_fails_the_job() {
    let fx = fx();
    fx.hooks("pre-start = \"exit 7\"\n");

    let out = fx.dispatch(JOB).ok();
    let (attempt, _) = fx.wait_end(ATTEMPT, &out["pid"]);

    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("failed"), &json!("setup_failed")));
    assert_eq!(fx.job_status(JOB), "failed");
    assert_eq!(fx.kinds(ATTEMPT), ["attempt-started", "job-failed"]);
    assert!(fx.env.calls("pi").is_empty());
    assert!(!fx.worktree(ATTEMPT).exists());
}

#[test]
fn a_timed_out_stop_request_ends_it_and_fails_the_job() {
    let fx = fx();
    fx.pi("sleep 120");
    let out = fx.dispatch(JOB).ok();
    fx.wait_status(ATTEMPT, "running");

    fx.fixture("UPDATE attempts SET stop_request = 'timed_out' WHERE plan = 'pay' AND id = 'api-a1';");
    let (attempt, _) = fx.wait_end(ATTEMPT, &out["pid"]);

    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("failed"), &json!("timed_out")));
    assert_eq!(fx.job_status(JOB), "failed");
    assert!(!fx.windows().contains("pay--api-a1"), "{}", fx.windows());
}

#[test]
fn an_idle_worker_is_nudged_once_then_ends_no_submit() {
    let fx = fx();
    fx.pi(&format!(
        r#"sid=""; prev=""
for a in "$@"; do [ "$prev" = "--session-id" ] && sid="$a"; prev="$a"; done
mkdir -p "$HOME/.pi/agent/sessions/w"
f="$HOME/.pi/agent/sessions/w/2026_$sid.jsonl"
msg='{{"type":"message","timestamp":"2026-10-14T21:00:05.000Z","message":{{"role":"assistant","usage":{{"input":1,"output":1,"cacheRead":0,"cacheWrite":0,"cost":{{"total":0}}}},"stopReason":"stop"}}}}'
printf '%s\n' '{{"type":"session","timestamp":"2026-10-14T21:00:00.000Z"}}' "$msg" > "$f"
"$CPD" session state idle > /dev/null
i=0
until "$CPD" events list --kind attempt-nudge | grep -q nudge; do
  i=$((i+1)); [ $i -gt 300 ] && exit 9; sleep 0.2
done
"$CPD" session state working > /dev/null
printf '%s\n' "$msg" >> "$f"
"$CPD" session state idle > /dev/null
sleep 120"#
    ));

    let out = fx.dispatch(JOB).ok();
    let (attempt, _) = fx.wait_end(ATTEMPT, &out["pid"]);

    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("failed"), &json!("no_submit")), "{}", fx.log(ATTEMPT));
    assert_eq!(attempt["nudged"], true);
    let nudge = fx.event(ATTEMPT, "attempt-nudge");
    assert_eq!(nudge["session"], attempt["worker_session"]);
    assert!(nudge["data"]["prompt"].as_str().unwrap().starts_with("You ended your turn without running cpd attempt submit api-a1."));
    assert_eq!(fx.kinds(ATTEMPT).iter().filter(|k| *k == "attempt-nudge").count(), 1);
    assert_eq!(fx.event(ATTEMPT, "attempt-retry")["data"]["cause"], "no_submit");
}

// ---------- cancel at each safe point ----------

fn assert_cancelled(fx: &Fx, attempt: &Value, reason: &str) {
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("cancelled"), &json!("cancel_requested")), "{}", fx.log(ATTEMPT));
    assert_eq!(fx.job_status(JOB), "cancelled");
    assert_eq!(fx.event(ATTEMPT, "job-cancelled")["data"], json!({"reason": reason}));
}

#[test]
fn a_cancel_before_a3_ends_it_before_the_worker_starts() {
    let fx = fx();
    // pre-start runs in A2: the request is there at the safe point before A3.
    fx.hooks(&format!("pre-start = \"{}\"\n", fx.cpd_line("job cancel api --reason early --plan pay > /dev/null")));

    let out = fx.dispatch(JOB).ok();
    let (attempt, driver) = fx.wait_end(ATTEMPT, &out["pid"]);

    assert_eq!(driver["outcome"], "cancelled");
    assert_cancelled(&fx, &attempt, "early");
    assert!(attempt["worker_session"].is_null());
    assert!(fx.env.calls("pi").is_empty());
    assert_eq!(fx.kinds(ATTEMPT), ["attempt-started", "job-cancel-requested", "job-cancelled"]);
}

#[test]
fn a_cancel_before_the_rebase_ends_it_without_merging() {
    let fx = fx();
    let old = fx.lead_sha();
    fx.pi(
        "echo api > api.txt; git add -A; git commit -qm api\n\
         \"$CPD\" job cancel api --reason mid > /dev/null\n\
         \"$CPD\" attempt submit \"$CPD_ATTEMPT\" > /dev/null\n\
         sleep 120",
    );

    let out = fx.dispatch(JOB).ok();
    let (attempt, _) = fx.wait_end(ATTEMPT, &out["pid"]);

    assert_cancelled(&fx, &attempt, "mid");
    assert_eq!(fx.lead_sha(), old);
    assert!(fx.kinds(ATTEMPT).contains(&"attempt-status".to_owned()));
}

#[test]
fn a_cancel_before_the_compare_and_swap_ends_it_without_merging() {
    let fx = fx();
    // The request arrives during the rebase, after the first safe point.
    let hook = fx.repo.join(".git/hooks/post-rewrite");
    write_executable(&hook, &format!("#!/bin/sh\n{} > /dev/null\n", fx.cpd_line("job cancel api --reason late --plan pay")));
    // Another Job merges first, so the rebase rewrites the Worker's commit.
    fx.pi(
        "git -C \"$LEAD\" commit -q --allow-empty -m other\n\
         echo api > api.txt; git add -A; git commit -qm api\n\
         \"$CPD\" attempt submit \"$CPD_ATTEMPT\" > /dev/null\n\
         sleep 120",
    );

    let out = fx.dispatch(JOB).ok();
    let (attempt, _) = fx.wait_end(ATTEMPT, &out["pid"]);

    assert_cancelled(&fx, &attempt, "late");
    let lead = fx.lead_sha();
    assert_eq!(fx.env.git(&fx.repo, &["log", "-1", "--format=%s", &lead]), "other");
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
    fx.add_job("ui", &[]);
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
    let fx = fx();
    fx.pi("exit 0");
    fx.fixture(
        "UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = 'api';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
           VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', 'running', 'cpd/pay--api-a1', '/w', 't');
         UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'timed_out', ended_at = 't' WHERE id = 'api-a1';
         UPDATE jobs SET status = 'failed', failed_cause = 'timed_out' WHERE plan = 'pay' AND key = 'api';",
    );
    fx.env.cpd(&["job", "retry", JOB, "--tier", "S3", "--plan", PLAN]).ok();

    let out = fx.dispatch(JOB).ok();

    assert_eq!((&out["attempt"], &out["tier"], &out["origin"]), (&json!("api-a2"), &json!("S3"), &json!("retry")));
    let (attempt, _) = fx.wait_end("api-a2", &out["pid"]);
    assert_eq!(attempt["origin"], "retry");
    assert_eq!(fx.event("api-a2", "attempt-started")["data"], json!({"number": 2, "tier": "S3", "origin": "retry"}));
}

// ---------- the stall rule (§3.4 A4, hs-d195) ----------

/// A Worker whose session file ends in a `bash` tool call, with `result` (a
/// JSONL line, or nothing) after it, and whose tool call is a `sleep SLEEP_SECONDS`
/// child of the fake pi. The file is backdated an hour, quiet for longer than any
/// `stall_seconds` here: the stall rule's time holds at the driver's first look.
/// The child's pid goes to `../stall-child.pid`. When the child
/// dies, pi writes the tool's failure, leaves `../stall-survived` and stays.
fn stalled_worker(result: &str) -> String {
    format!(
        r#"sid=""; prev=""
for a in "$@"; do [ "$prev" = "--session-id" ] && sid="$a"; prev="$a"; done
mkdir -p "$HOME/.pi/agent/sessions/w"
f="$HOME/.pi/agent/sessions/w/2026_$sid.jsonl"
printf '%s\n' '{{"type":"session","timestamp":"2026-10-14T21:00:00.000Z"}}' > "$f"
printf '%s\n' '{{"type":"message","timestamp":"2026-10-14T21:00:05.000Z","message":{{"role":"assistant","content":[{{"type":"text","text":"run it"}},{{"type":"toolCall","id":"call_1","name":"bash","arguments":{{"command":"sleep {SLEEP_SECONDS}"}}}}],"stopReason":"toolUse"}}}}' >> "$f"
{result}
touch -d '1 hour ago' "$f"
sleep {SLEEP_SECONDS} &
child=$!
echo "$child" > ../stall-child.pid
wait "$child"
printf '%s\n' '{{"type":"message","message":{{"role":"toolResult","toolCallId":"call_1","content":[{{"type":"text","text":"Terminated"}}],"isError":true}}}}' >> "$f"
: > ../stall-survived
exec sleep {SLEEP_SECONDS}"#,
        result = if result.is_empty() { String::new() } else { format!("printf '%s\\n' '{result}' >> \"$f\"") },
    )
}

/// A tool result for `call_1`, the stalled worker's tool call.
const CALL_1_RESULT: &str =
    r#"{"type":"message","message":{"role":"toolResult","toolCallId":"call_1","content":[{"type":"text","text":"done"}],"isError":false}}"#;

/// The pid the fake pi's tool call runs as, once it has started (at most 60 s).
fn stall_child(fx: &Fx) -> nix::unistd::Pid {
    let path = fx.env.root().join("repos/stall-child.pid");
    let pid = common::wait_some("the fake pi to start its tool call", Duration::from_secs(60), || {
        fs::read_to_string(&path).ok().and_then(|t| t.trim().parse().ok()).ok_or_else(|| format!("log: {}", fx.log(ATTEMPT)))
    });
    nix::unistd::Pid::from_raw(pid)
}

fn alive(pid: nix::unistd::Pid) -> bool {
    nix::sys::signal::kill(pid, None).is_ok()
}

/// Ends the Attempt and its driver, and the tool call if it is still there.
fn stop(fx: &Fx, out: &Value, child: nix::unistd::Pid) {
    let _ = nix::sys::signal::kill(child, nix::sys::signal::Signal::SIGKILL);
    fx.fixture("UPDATE attempts SET stop_request = 'timed_out' WHERE plan = 'pay' AND id = 'api-a1';");
    fx.wait_end(ATTEMPT, &out["pid"]);
}

/// The tool call's child stays alive, and no stall is logged, for `polls` of the driver after it has found the
/// Worker's session file (it records it: from then on each poll applies the stall rule).
fn assert_not_killed(fx: &Fx, child: nix::unistd::Pid, polls: u32) {
    common::wait_until("the driver to record the Worker's session file", || {
        fx.env.query::<Option<String>>("SELECT file FROM sessions WHERE role = 'worker'").is_some()
    });
    // time-claim: the call is not killed over `polls` polls of the driver; no per-poll signal from the driver (a poll
    // that kills nothing leaves nothing behind).
    assert!(common::poll(POLL * polls, || (!alive(child)).then_some(())).is_none(), "the tool call was killed\nlog: {}", fx.log(ATTEMPT));
    assert!(!fx.log(ATTEMPT).contains("stall"), "{}", fx.log(ATTEMPT));
    assert_eq!(fx.attempt(ATTEMPT)["status"], "running");
}

#[test]
fn a_stalled_tool_call_has_its_processes_killed_and_the_worker_keeps_running() {
    let fx = fx_with("[timeouts]\nstall_seconds = 2");
    fx.pi(&stalled_worker(""));

    let out = fx.dispatch(JOB).ok();
    let child = stall_child(&fx);
    let survived = fx.env.root().join("repos/stall-survived");
    common::wait_some("pi to get its tool's failure and the kill to be logged", Duration::from_secs(60), || {
        let log = fx.log(ATTEMPT);
        if survived.exists() && log.contains("stall") { Ok(()) } else { Err(format!("log: {log}")) }
    });

    // The tool call is dead, pi lives on (it wrote the marker after the call
    // ended, then stays), and the Attempt keeps running.
    assert!(!alive(child), "the tool call is still alive");
    assert_eq!(fx.attempt(ATTEMPT)["status"], "running");
    let pane: String = fx.env.query("SELECT tmux_pane FROM sessions WHERE role = 'worker'");
    let listed = fx.env.tmux(&["list-panes", "-a", "-F", "#{pane_id} #{pane_dead}"]);
    assert!(String::from_utf8_lossy(&listed.stdout).lines().any(|l| l == format!("{pane} 0")), "{listed:?}");
    // One line in the driver log: the pid, its command line and the idle time.
    let log = fx.log(ATTEMPT);
    let lines: Vec<&str> = log.lines().filter(|l| l.contains("stall")).collect();
    assert_eq!(lines.len(), 1, "{log}");
    assert!(lines[0].contains(&format!("{child} (sleep {SLEEP_SECONDS})")), "{log}");
    assert!(lines[0].contains("idle"), "{log}");
    assert!(!fx.kinds(ATTEMPT).contains(&"error".to_owned()), "{:?}", fx.kinds(ATTEMPT));

    stop(&fx, &out, child);
}

#[test]
fn a_tool_call_with_its_result_is_not_a_stall() {
    let fx = fx_with("[timeouts]\nstall_seconds = 1");
    fx.pi(&stalled_worker(CALL_1_RESULT));

    let out = fx.dispatch(JOB).ok();
    let child = stall_child(&fx);
    // The file is quiet past stall_seconds (backdated), but the call has its result.
    assert_not_killed(&fx, child, 6);

    stop(&fx, &out, child);
}

#[test]
fn stall_seconds_0_kills_nothing() {
    let fx = fx_with("[timeouts]\nstall_seconds = 0");
    fx.pi(&stalled_worker(""));

    let out = fx.dispatch(JOB).ok();
    let child = stall_child(&fx);
    // The file is quiet for an hour, inside a tool call: only stall_seconds = 0 keeps the call alive.
    assert_not_killed(&fx, child, 6);

    stop(&fx, &out, child);
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
             UPDATE prs SET status = 'open', pr_number = 812, pr_url = '{PR_URL}', pr_opened_at = 'opened', pull_request = (SELECT id FROM pull_requests) WHERE key = 'repo';
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

/// Runs Job `key` to its end: dispatches it, waits for its Attempt `<key>-a1` and its driver, returns the Attempt.
fn run_job(fx: &Fx, key: &str) -> Value {
    let out = fx.dispatch(key).ok();
    fx.wait_end(&format!("{key}-a1"), &out["pid"]).0
}

/// The approval of a review queues the update Job for the fix drafts it queues on an open PR (hs-d497 (2); the other cause of `job add`): one
/// Job whatever the number of drafts, `reason` `feedback`, in the transaction of the approval. A draft of no open PR queues none.
#[test]
fn the_approval_of_a_review_queues_the_update_job_for_a_fix_draft_of_an_open_pr() {
    let fx = rows();
    fx.open_pr();
    let spec = fx.fx.env.root().join("fix-spec.json");
    fs::write(&spec, r#"{"goal": "Fix it.", "context": "A reviewer asked.", "touches": ["fix.txt"], "acceptance": ["ok"], "out_of_scope": []}"#).unwrap();
    for key in ["fix", "fix2"] {
        fx.fx.cpd(&["job", "add", key, "--title", "Fix", "--tier", "S2", "--spec", spec.to_str().unwrap(), "--pr", "repo", "--plan", "pay", "--draft"]).ok();
    }
    let update_jobs = || -> Vec<(String, String)> {
        fx.fx.env.read(|c| {
            let mut stmt = c.prepare("SELECT key, status FROM jobs WHERE kind = 'update' ORDER BY key")?;
            Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    };
    assert_eq!(update_jobs(), Vec::<(String, String)>::new(), "a draft queues nothing");
    fx.fx.env.outline_with("pay", |args| fx.fx.cpd(args));
    let review = fx.fx.cpd(&["plan", "review", "--plan", "pay", "--title", "Fixes"]).ok();

    fx.fx.cpd(&["plan", "decide", review["review"]["id"].as_str().unwrap(), "--approve", "--plan", "pay"]).ok();

    assert_eq!(update_jobs(), [("up-repo".to_owned(), "queued".to_owned())]);
    let added: Vec<Value> = fx.plan_events("job-added").into_iter().map(|e| e.1).filter(|d| d["kind"] == "update").collect();
    assert_eq!(added.len(), 1, "{added:?}");
    assert_eq!((&added[0]["reason"], &added[0]["pr"]), (&json!("feedback"), &json!("repo")));
}

