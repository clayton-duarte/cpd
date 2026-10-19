//! The Attempt driver (contract §3.4; commands.md §5.5, §6.4): `job dispatch`
//! starts the driver detached with the driver lock as fd 3, and the driver runs
//! A1–A9 against a temporary repo, a private tmux server and a fake pi Worker.
//!
//! There is no Reviewer (R19, Escalation rs-driver-a1-q1): a submitted Attempt
//! goes straight to the merge.

use crate::common;

use std::fs::{self, File, OpenOptions};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use common::{CPD, Run, SLEEP_SECONDS, TestEnv, refused, write_executable};
use cpd_core::error::CpdError;
use cpd_core::git::{Setup, create_lead_worktree, pr_lead_worktree_path, prepare_lead_worktree};
use cpd_core::home::CpdHome;
use cpd_core::ids::{AttemptId, PlanId, PrKey};
use cpd_core::plan::{jobs, locked_fingerprint};
use cpd_core::store::{Store, WriteTxn};
use serde_json::{Value, json};

const PLAN: &str = "pay";
const JOB: &str = "api";
const ATTEMPT: &str = "api-a1";

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
    // The work session as the engine leaves it: its `watch` window, and
    // remain-on-exit (§6.1) so a dead Worker pane stays to be read.
    for args in [&["new-session", "-d", "-s", "cpd-work", "-n", "watch", "sleep", "600"][..], &["set", "-g", "remain-on-exit", "on"]] {
        assert!(env.tmux(args).status.success(), "tmux {args:?}");
    }
    let fx = Fx { env, repo, lead };
    fx.pi("exit 0");
    let catalog: String = EFFORTS
        .iter()
        .map(|(letter, model, efforts)| {
            format!("INSERT INTO tier_catalog (pi_version, letter, model, efforts, read_at) VALUES ('1.0.0', '{letter}', '{model}', '{efforts}', 't');")
        })
        .collect();
    fx.fixture(&format!(
        "{catalog}
         INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '{repo}', 'main', 'dispatch', 'cpd/pay', '{lead}', cpd_txn(), 't', 't');",
        repo = fx.repo.display(),
        lead = fx.lead.display()
    ));
    fx.add_job(JOB, &[]);
    fx
}

impl Fx {
    fn home(&self) -> CpdHome {
        CpdHome::resolve(Some(self.env.cpd_home().into()), None).unwrap()
    }

    fn store(&self) -> Store {
        Store::open(&self.home(), 5000).unwrap()
    }

    fn fixture(&self, sql: &str) {
        self.store().write("fixture", |tx| exec(tx, sql)).unwrap();
    }

    fn query<T: rusqlite::types::FromSql>(&self, sql: &str) -> T {
        self.store().read(|conn| Ok(conn.query_row(sql, [], |r| r.get(0))?)).unwrap()
    }

    /// Job `key` of PR `repo` of Plan `pay`, queued with its Locked-Job fingerprint.
    fn add_job(&self, key: &str, needs: &[&str]) {
        self.add_job_in("repo", key, needs);
    }

    /// PR `key` of Plan `pay` in a new repo `<key>`, started: Lead Branch `cpd/pay/<key>`
    /// and its Lead worktree. Returns the repo and the Lead worktree.
    fn add_pr(&self, key: &str) -> (PathBuf, PathBuf) {
        let repo = self.env.git_repo(key).canonicalize().unwrap();
        for (name, value) in [("user.name", "cpd test"), ("user.email", "test@cpd.invalid"), ("commit.gpgsign", "false")] {
            self.env.git(&repo, &["config", name, value]);
        }
        let plan = PlanId::parse(PLAN).unwrap();
        let branch = format!("cpd/pay/{key}");
        let path = pr_lead_worktree_path(&repo, &plan, &PrKey::parse(key).unwrap(), &branch).unwrap();
        let setup = Setup { timeout: Duration::from_secs(30), log: self.env.root().join("lead.log") };
        prepare_lead_worktree(&repo, &plan, &path, &branch, "main", &setup).map_err(|f| f.error).unwrap();
        self.fixture(&format!(
            "INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
               VALUES ('pay', '{key}', '{}', 'main', 'dispatch', '{branch}', '{}', cpd_txn(), 't', 't');",
            repo.display(),
            path.display()
        ));
        (repo, path)
    }

    /// [`Fx::add_job`] in PR `pr`.
    fn add_job_in(&self, pr: &str, key: &str, needs: &[&str]) {
        let mut sql = format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at)
             VALUES ('pay', '{pr}', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 0, cpd_txn(), 't');"
        );
        for need in needs {
            sql += &format!("INSERT INTO dependencies (plan, job, needs) VALUES ('pay', '{key}', '{need}');");
        }
        self.fixture(&sql);
        let plan = PlanId::parse(PLAN).unwrap();
        let all = self.store().read(|conn| jobs::load(conn, &plan)).unwrap();
        let job = all.iter().find(|j| j.key == key).unwrap();
        self.fixture(&format!(
            "UPDATE jobs SET status = 'queued', fingerprint = '{}' WHERE plan = 'pay' AND key = '{key}';",
            locked_fingerprint(job)
        ));
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

    /// Commits `.config/wt.toml` = `text` on the Lead Branch, where the
    /// Attempt worktrees are cut from.
    fn wt_toml(&self, text: &str) {
        fs::create_dir_all(self.lead.join(".config")).unwrap();
        fs::write(self.lead.join(".config/wt.toml"), text).unwrap();
        self.env.git(&self.lead, &["add", "-A"]);
        self.env.git(&self.lead, &["commit", "-q", "-m", "wt.toml"]);
    }

    /// `cpd <args>` with this env's CPD_HOME, as a hook or the Worker calls it.
    fn cpd_line(&self, args: &str) -> String {
        format!("CPD_HOME='{}' '{CPD}' {args}", self.env.cpd_home().display())
    }

    fn dispatch(&self, key: &str) -> Run {
        self.env.cpd(&["job", "dispatch", key, "--plan", PLAN])
    }

    fn attempt(&self, id: &str) -> Value {
        self.env.cpd(&["attempt", "show", id, "--plan", PLAN]).ok()["attempt"].clone()
    }

    fn job_status(&self, key: &str) -> String {
        self.query(&format!("SELECT status FROM jobs WHERE plan = 'pay' AND key = '{key}'"))
    }

    /// Waits until Attempt `id` is `status` (at most 60 s); returns it.
    fn wait_status(&self, id: &str, status: &str) -> Value {
        common::wait_some(&format!("Attempt {id} to become {status}"), Duration::from_secs(60), || {
            let attempt = self.attempt(id);
            if attempt["status"] == status { Ok(attempt) } else { Err(format!("{attempt}\nlog: {}", self.log(id))) }
        })
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
            self.query(&format!("SELECT worker_session FROM attempts WHERE plan = 'pay' AND id = '{id}'"));
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

    fn txns(&self) -> i64 {
        self.query("SELECT count(*) FROM txns")
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

#[test]
fn a_dispatched_attempt_runs_its_worker_merges_and_ends_merged() {
    let fx = fx();
    fx.pi(WORKER);
    let old = fx.lead_sha();

    let out = fx.dispatch(JOB).ok();
    assert_eq!(out["attempt"], ATTEMPT);
    assert_eq!(out["tier"], "S2");
    assert_eq!(out["origin"], "ladder");
    let (attempt, driver) = fx.wait_end(ATTEMPT, &out["pid"]);

    assert_eq!(driver, json!({"ok": true, "attempt": ATTEMPT, "outcome": "merged", "cause": "merged"}));
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("merged"), &json!("merged")));
    let lead = fx.lead_sha();
    assert_ne!(lead, old);
    assert_eq!(attempt["merged_sha"], lead);
    assert_eq!(attempt["base_sha"], old);
    assert_eq!(attempt["driver_pid"], out["pid"]);
    assert_eq!(attempt["metrics"]["turns"], 1);
    assert_eq!(attempt["metrics"]["tokens_out"], 20);
    assert_eq!(fx.job_status(JOB), "merged");
    assert_eq!(fs::read_to_string(fx.lead.join("api.txt")).unwrap(), "api\n");
    assert_eq!(
        fx.kinds(ATTEMPT),
        ["attempt-started", "session-registered", "attempt-status", "session-moved", "attempt-status", "job-merged"]
    );
    let statuses: Vec<Value> =
        fx.events(ATTEMPT).into_iter().filter(|e| e["kind"] == "attempt-status").map(|e| e["data"].clone()).collect();
    assert_eq!(statuses, [json!({"from": "starting", "to": "running"}), json!({"from": "running", "to": "merging"})]);
    assert_eq!(fx.event(ATTEMPT, "session-moved")["data"]["place"], "work");

    // The Worker: its Session, its argv (§6.5) and its Spec (§16.1).
    let session = attempt["worker_session"].as_str().unwrap().to_owned();
    let row: (String, String, String) = fx
        .store()
        .read(|c| Ok(c.query_row("SELECT role, name, file FROM sessions WHERE id = ?1", [&session], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?))
        .unwrap();
    assert_eq!((row.0.as_str(), row.1.as_str()), ("worker", "pay api a1"));
    assert!(row.2.ends_with(&format!("_{session}.jsonl")), "{row:?}");
    let spec = fx.env.cpd_home().join("run/pay/specs/api-a1.md");
    assert_eq!(attempt["spec_path"], spec.to_str().unwrap());
    let text = fs::read_to_string(&spec).unwrap();
    assert!(text.starts_with("# Worker Spec: api, Attempt 1\n"), "{text}");
    let calls = fx.env.calls("pi");
    assert_eq!(calls.len(), 1, "{calls:?}");
    let argv = &calls[0];
    assert_eq!(&argv[..4], ["--approve", "--no-skills", "--exclude-tools", cpd_core::host::pi::WORKER_EXCLUDED_TOOLS]);
    let at = |flag: &str| argv[argv.iter().position(|a| a == flag).unwrap() + 1].clone();
    assert_eq!(at("--model"), "p/sonnet:medium");
    assert_eq!(at("--session-id"), session);
    assert_eq!(at("-n"), "pay api a1");
    assert_eq!(argv.last().unwrap(), &format!("Read {} and follow it exactly. Your attempt id is api-a1.", spec.display()));

    // A9: the window is gone, the worktree and branch are removed.
    assert!(!fx.windows().contains("pay--api-a1"), "{}", fx.windows());
    assert!(!fx.worktree(ATTEMPT).exists());
    let branches = fx.env.git(&fx.repo, &["branch", "--list", "cpd/pay--api-a1"]);
    assert_eq!(branches, "");

    // A9: the driver lock file is removed.
    assert!(!fx.home().attempt_lock(&PlanId::parse(PLAN).unwrap(), &AttemptId::parse(ATTEMPT).unwrap()).exists());
    let left = fs::read_dir(fx.home().locks_dir()).unwrap().map(|e| e.unwrap().file_name()).collect::<Vec<_>>();
    assert!(left.iter().all(|n| !n.to_string_lossy().starts_with("attempt-")), "{left:?}");
}

#[test]
fn the_workers_window_is_killed_when_the_driver_sees_merging() {
    let fx = fx();
    fx.pi(WORKER);
    // The merge waits for the per-repo lock, so A9's kill (after the merge) cannot be what is seen.
    let lock = cpd_core::merge::RepoLock::wait(&fx.home(), &fx.repo).unwrap();

    let out = fx.dispatch(JOB).ok();
    fx.wait_status(ATTEMPT, "merging");
    let deadline = Instant::now() + Duration::from_secs(30);
    while fx.windows().contains("pay--api-a1") {
        assert!(Instant::now() < deadline, "the Worker's window outlived `merging`: {}\nlog: {}", fx.windows(), fx.log(ATTEMPT));
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(fx.attempt(ATTEMPT)["status"], "merging");

    drop(lock);
    let (attempt, _) = fx.wait_end(ATTEMPT, &out["pid"]);
    assert_eq!(attempt["outcome"], "merged");
}

// ---------- PRs across repos (R34) ----------

#[test]
fn an_attempt_of_a_pr_in_another_repo_is_cut_from_and_merged_into_that_prs_branch() {
    let fx = fx();
    fx.pi(WORKER);
    let (web, web_lead) = fx.add_pr("web");
    fx.add_job_in("web", "ui", &[]);
    // The Lead Branch of the web PR defines its hooks: pre-start sees it as {{ base }}.
    fs::create_dir_all(web_lead.join(".config")).unwrap();
    fs::write(web_lead.join(".config/wt.toml"), "[pre-start]\nmark = \"echo {{ base }} > ../base-seen.txt\"\n").unwrap();
    fx.env.git(&web_lead, &["add", "-A"]);
    fx.env.git(&web_lead, &["commit", "-q", "-m", "wt.toml"]);
    let (pay_old, web_old) =
        (fx.lead_sha(), fx.env.git(&web, &["rev-parse", "refs/heads/cpd/pay/web"]));

    let out = fx.dispatch("ui").ok();
    let (attempt, driver) = fx.wait_end("ui-a1", &out["pid"]);

    assert_eq!(driver["outcome"], "merged", "{driver}");
    let web_new = fx.env.git(&web, &["rev-parse", "refs/heads/cpd/pay/web"]);
    assert_ne!(web_new, web_old);
    assert_eq!(attempt["merged_sha"], web_new);
    assert_eq!(attempt["base_sha"], web_old);
    // The other PR's repo and branch did not move.
    assert_eq!(fx.lead_sha(), pay_old);
    assert!(!fx.lead.join("api.txt").exists());
    assert_eq!(fs::read_to_string(web_lead.join("api.txt")).unwrap(), "api\n");
    assert_eq!(fs::read_to_string(fx.env.root().join("repos/base-seen.txt")).unwrap(), "cpd/pay/web\n");
    assert_eq!(fx.job_status("ui"), "merged");
    // A9 in the PR's repo.
    assert!(!web.parent().unwrap().join("web.pay--ui-a1").exists());
    assert_eq!(fx.env.git(&web, &["branch", "--list", "cpd/pay--ui-a1"]), "");
}

#[test]
fn dispatch_refuses_a_job_of_a_pr_that_has_not_started() {
    let fx = fx();
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

/// A step of the driver that meets the write lock held for longer than `store_busy_ms` (here 200 ms)
/// waits and completes: the Attempt runs to its end, it is not `crashed`.
#[test]
fn a_driver_step_that_meets_a_held_write_lock_waits_and_the_attempt_is_not_crashed() {
    let fx = fx_with("[timeouts]\nstore_busy_ms = 200\n");
    // A2's pre-start takes a second, so the driver's first write comes while the lock is held.
    fx.wt_toml("[pre-start]\nwait = \"sleep 1\"\n");
    fx.pi(WORKER);

    let out = fx.dispatch(JOB).ok();
    let holder = rusqlite::Connection::open(fx.home().db()).unwrap();
    holder.busy_timeout(Duration::from_secs(10)).unwrap();
    holder.execute_batch("BEGIN IMMEDIATE").unwrap();
    // The driver logs its first `store_busy` once the lock outlasted `store_busy_ms`: it met the lock and waits.
    common::wait_until("the driver to meet the lock", || fx.log(ATTEMPT).contains("store_busy: "));
    holder.execute_batch("ROLLBACK").unwrap();

    let (attempt, driver) = fx.wait_end(ATTEMPT, &out["pid"]);

    assert_eq!(driver, json!({"ok": true, "attempt": ATTEMPT, "outcome": "merged", "cause": "merged"}), "{}", fx.log(ATTEMPT));
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("merged"), &json!("merged")));
    let log = fx.log(ATTEMPT);
    assert!(log.contains("store_busy: "), "{log}");
    assert!(log.contains("the store is clear again after "), "{log}");
}

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
fn a_dead_worker_pane_ends_crashed_through_the_ladder() {
    let fx = fx();
    fx.pi("exit 3");

    let out = fx.dispatch(JOB).ok();
    let (attempt, driver) = fx.wait_end(ATTEMPT, &out["pid"]);

    assert_eq!(driver["cause"], "crashed");
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("failed"), &json!("crashed")));
    assert_eq!(fx.job_status(JOB), "queued");
    let retry = fx.event(ATTEMPT, "attempt-retry");
    assert_eq!(retry["data"]["next_tier"], "S3");
    assert!(retry["data"]["feedback"].as_str().unwrap().contains("pane died"), "{retry}");
    assert!(!fx.windows().contains("pay--api-a1"), "{}", fx.windows());
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
    let deadline = Instant::now() + Duration::from_secs(30);
    while !answered.exists() {
        assert!(Instant::now() < deadline, "the hiccup never happened\nlog: {}", fx.log(ATTEMPT));
        std::thread::sleep(Duration::from_millis(100));
    }
    // The poll after the hiccup lists the panes again: the failed one judged nothing.
    common::wait_until("the driver to list the panes again", || list_panes_calls(&fx) >= 2);
    // time-claim: the Worker is still running a poll after the retried listing; no per-poll signal from the driver
    // once it has the pane's pid (it forks nothing).
    std::thread::sleep(Duration::from_millis(1000));
    let attempt = fx.attempt(ATTEMPT);
    assert_eq!(attempt["status"], "running", "{attempt}\nlog: {}", fx.log(ATTEMPT));

    fx.fixture("UPDATE attempts SET stop_request = 'timed_out' WHERE plan = 'pay' AND id = 'api-a1';");
    let (attempt, _) = fx.wait_end(ATTEMPT, &out["pid"]);
    assert_eq!(attempt["cause"], "timed_out", "log: {}", fx.log(ATTEMPT));
}

/// engine-F1: A4 tests the Worker's pane by its pid; it does not fork `tmux list-panes`
/// at every poll while the Worker runs.
#[test]
fn a_running_worker_is_watched_without_listing_panes_at_every_poll() {
    let fx = fx();
    fx.pi("sleep 120");
    fx.env.fake("tmux", &format!("exec '{}' \"$@\"", real_tmux(&fx)));

    let out = fx.dispatch(JOB).ok();
    fx.wait_status(ATTEMPT, "running");
    // The pid is recorded at the first poll, A4's first `list-panes` (the driver lists no panes before A4).
    common::wait_until("A4's first poll to list the panes", || list_panes_calls(&fx) >= 1);
    let before = list_panes_calls(&fx);
    // time-claim: no `list-panes` over six 0.5 s polls of a running Worker; no per-poll signal from the driver (that
    // it forks nothing at a poll is the claim).
    std::thread::sleep(Duration::from_secs(3));
    let during = list_panes_calls(&fx) - before;
    assert_eq!(during, 0, "list-panes calls in 3 s of a running Worker\nlog: {}", fx.log(ATTEMPT));

    fx.fixture("UPDATE attempts SET stop_request = 'timed_out' WHERE plan = 'pay' AND id = 'api-a1';");
    let (attempt, _) = fx.wait_end(ATTEMPT, &out["pid"]);
    assert_eq!(attempt["cause"], "timed_out");
}

#[test]
fn a_failing_pre_start_ends_setup_failed_and_fails_the_job() {
    let fx = fx();
    fx.wt_toml("pre-start = \"exit 7\"\n");

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
    fx.wt_toml(&format!("pre-start = \"{}\"\n", fx.cpd_line("job cancel api --reason early --plan pay > /dev/null")));

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

/// Attempt `api-a1` in `starting` and its lock file, as a starter would leave them.
fn starting(fx: &Fx) -> PathBuf {
    let lock = fx.home().attempt_lock(&PlanId::parse(PLAN).unwrap(), &cpd_core::ids::AttemptId::parse(ATTEMPT).unwrap());
    fs::create_dir_all(lock.parent().unwrap()).unwrap();
    File::create(&lock).unwrap();
    let worktree = fx.worktree(ATTEMPT);
    fx.fixture(&format!(
        "UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = 'api';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
           VALUES ('pay', 'api-a1', 'api', 1, 'S2', 'ladder', 'starting', 'cpd/pay--api-a1', '{}', 't');",
        worktree.display()
    ));
    lock
}

#[test]
fn a_driver_without_the_lock_as_fd_3_exits_without_writing() {
    let fx = fx();
    starting(&fx);
    let before = fx.txns();

    // fd 3 not open.
    let run = fx.env.cpd(&["attempt", "run", ATTEMPT, "--plan", PLAN]);
    assert_eq!(run.exit, 1, "{run:?}");
    assert_eq!(run.error()["code"], "driver_lock_missing");

    // fd 3 is another file.
    let other = fx.env.root().join("other.lock");
    fs::write(&other, "").unwrap();
    let out = fx
        .env
        .command("sh")
        .args(["-c", "exec 3<\"$1\"; shift; exec \"$@\"", "sh"])
        .arg(&other)
        .args([CPD, "attempt", "run", ATTEMPT, "--plan", PLAN])
        .output()
        .unwrap();
    let run = Run {
        exit: out.status.code().unwrap(),
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    };
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
    let fx = fx();
    let lock = starting(&fx);
    fx.fixture("UPDATE attempts SET status = 'running' WHERE plan = 'pay' AND id = 'api-a1';");
    let before = fx.txns();

    let out = fx
        .env
        .command("sh")
        .args(["-c", "exec 3<\"$1\"; shift; exec \"$@\"", "sh"])
        .arg(&lock)
        .args([CPD, "attempt", "run", ATTEMPT, "--plan", PLAN])
        .output()
        .unwrap();

    assert_eq!(out.status.code(), Some(3), "{out:?}");
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["error"]["code"], "illegal_transition");
    assert_eq!(fx.txns(), before);
}

// ---------- job dispatch refusals ----------

#[test]
fn dispatch_refuses_a_job_that_is_not_ready() {
    let fx = fx();
    fx.add_job("ui", &["api"]);
    let before = fx.txns();

    refused(&fx.dispatch("ui"), "job_not_ready", 3);
    refused(&fx.dispatch("nope"), "job_not_found", 4);
    assert_eq!(fx.txns(), before);
}

#[test]
fn dispatch_refuses_a_job_whose_fingerprint_does_not_match() {
    let fx = fx();
    fx.fixture(&format!("UPDATE jobs SET fingerprint = '{}' WHERE plan = 'pay' AND key = 'api';", "0".repeat(64)));
    refused(&fx.dispatch(JOB), "locked_job_tampered", 3);
    assert_eq!(fx.job_status(JOB), "queued");
}

#[test]
fn dispatch_refuses_when_the_driver_lock_is_held() {
    let fx = fx();
    let lock = fx.home().attempt_lock(&PlanId::parse(PLAN).unwrap(), &cpd_core::ids::AttemptId::parse(ATTEMPT).unwrap());
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
    let fx = fx_with("concurrency = 1");
    fx.add_job("ui", &[]);
    fx.fixture(
        "UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = 'ui';
         INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, started_at)
           VALUES ('pay', 'ui-a1', 'ui', 1, 'S2', 'ladder', 'running', 'cpd/pay--ui-a1', '/w', 't');",
    );

    refused(&fx.dispatch(JOB), "no_slot", 3);
    assert_eq!(fx.job_status(JOB), "queued");
    assert_eq!(fx.query::<i64>("SELECT count(*) FROM attempts WHERE job = 'api'"), 0);
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
    fx.env.cpd(&["job", "retry", JOB, "--tier", "O4", "--plan", PLAN]).ok();

    let out = fx.dispatch(JOB).ok();

    assert_eq!((&out["attempt"], &out["tier"], &out["origin"]), (&json!("api-a2"), &json!("O4"), &json!("retry")));
    let (attempt, _) = fx.wait_end("api-a2", &out["pid"]);
    assert_eq!(attempt["origin"], "retry");
    assert_eq!(fx.event("api-a2", "attempt-started")["data"], json!({"number": 2, "tier": "O4", "origin": "retry"}));
}

// ---------- the stall rule (§3.4 A4, hs-d195) ----------

/// A Worker whose session file ends in a `bash` tool call, with `result` (a
/// JSONL line, or nothing) after it, and whose tool call is a `sleep SLEEP_SECONDS`
/// child of the fake pi. Its pid goes to `../stall-child.pid`. When the child
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
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(pid) = fs::read_to_string(&path).ok().and_then(|t| t.trim().parse().ok()) {
            return nix::unistd::Pid::from_raw(pid);
        }
        assert!(Instant::now() < deadline, "the fake pi never started its tool call\nlog: {}", fx.log(ATTEMPT));
        std::thread::sleep(Duration::from_millis(100));
    }
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

/// The tool call's child stays alive, and no stall is logged, for `seconds`.
fn assert_not_killed(fx: &Fx, child: nix::unistd::Pid, seconds: u64) {
    let until = Instant::now() + Duration::from_secs(seconds);
    while Instant::now() < until {
        assert!(alive(child), "the tool call was killed\nlog: {}", fx.log(ATTEMPT));
        std::thread::sleep(Duration::from_millis(200));
    }
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
    let deadline = Instant::now() + Duration::from_secs(60);
    while !survived.exists() || !fx.log(ATTEMPT).contains("stall") {
        assert!(Instant::now() < deadline, "pi never got its tool's failure, or no kill was logged\nlog: {}", fx.log(ATTEMPT));
        std::thread::sleep(Duration::from_millis(100));
    }

    // The tool call is dead, pi lives on (it wrote the marker after the call
    // ended, then stays), and the Attempt keeps running.
    assert!(!alive(child), "the tool call is still alive");
    assert_eq!(fx.attempt(ATTEMPT)["status"], "running");
    let pane: String = fx.query("SELECT tmux_pane FROM sessions WHERE role = 'worker'");
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
    assert_not_killed(&fx, child, 5);

    stop(&fx, &out, child);
}

#[test]
fn stall_seconds_0_kills_nothing() {
    let fx = fx_with("[timeouts]\nstall_seconds = 0");
    fx.pi(&stalled_worker(""));

    let out = fx.dispatch(JOB).ok();
    let child = stall_child(&fx);
    assert_not_killed(&fx, child, 5);

    stop(&fx, &out, child);
}
