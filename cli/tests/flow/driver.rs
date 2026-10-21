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
         INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '{repo}', 'main', 'dispatch', 'cpd/pay', '{lead}', '{PR_TITLE}', 'config', cpd_txn(), 't', 't');",
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

    fn txns(&self) -> i64 {
        self.env.query("SELECT count(*) FROM txns")
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

/// Puts a `git` first on the env's `PATH` that runs the real git, except that a merge's
/// compare-and-swap (`update-ref -m "cpd: merge …"`) waits until `release` exists (at most
/// 60 s). There is no per-repo lock to hold any more (hs-d243), so a test that must see the
/// driver in `merging` before the merge lands holds the merge itself.
fn hold_merges_until(env: &TestEnv, release: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let real = std::env::split_paths(common::inherited_path())
        .filter(|dir| *dir != env.bin())
        .map(|dir| dir.join("git"))
        .find(|p| fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0))
        .expect("git on PATH");
    let script = format!(
        r#"#!/bin/sh
real='{real}'
cas= merge=
for a in "$@"; do
  case "$a" in
    update-ref) cas=1 ;;
    "cpd: merge "*) merge=1 ;;
  esac
done
if [ -n "$cas" ] && [ -n "$merge" ]; then
  i=0
  while [ ! -e '{release}' ] && [ "$i" -lt 600 ]; do sleep 0.1; i=$((i+1)); done
fi
exec "$real" "$@"
"#,
        real = real.display(),
        release = release.display(),
    );
    let wrapper = env.bin().join("git");
    write_executable(&wrapper, &script);
}

/// engine-G3: a driver log line starts with a time, as `watch.log`'s do (RFC 3339 UTC, milliseconds).
#[test]
fn driver_log_lines_start_with_a_time() {
    let fx = fx();
    fx.pi(WORKER);
    let out = fx.dispatch(JOB).ok();
    fx.wait_end(ATTEMPT, &out["pid"]);

    let log = fx.log(ATTEMPT);
    // The last line is the command's JSON output (stdout), not a log line.
    let lines: Vec<&str> = log.lines().filter(|l| !l.starts_with('{')).collect();
    assert!(lines.len() >= 3, "{log}");
    for line in lines {
        let (time, rest) = line.split_once(' ').unwrap_or_else(|| panic!("no time in {line:?}\n{log}"));
        let b = time.as_bytes();
        let shaped = time.len() == 24
            && b[4] == b'-' && b[7] == b'-' && b[10] == b'T' && b[13] == b':' && b[16] == b':' && b[19] == b'.' && b[23] == b'Z'
            && time.bytes().enumerate().all(|(i, c)| matches!(i, 4 | 7 | 10 | 13 | 16 | 19 | 23) || c.is_ascii_digit());
        assert!(shaped, "no time at the start of {line:?}\n{log}");
        assert!(rest.starts_with("cpd attempt run: "), "{line:?}");
    }
}

#[test]
fn the_workers_window_is_killed_when_the_driver_sees_merging() {
    let fx = fx();
    fx.pi(WORKER);
    // The merge's compare-and-swap waits for `release`, so A9's kill (after the merge) cannot be what is seen.
    let release = fx.env.root().join("release");
    hold_merges_until(&fx.env, &release);

    let out = fx.dispatch(JOB).ok();
    fx.wait_status(ATTEMPT, "merging");
    common::wait_some("the Worker's window to close in `merging`", Duration::from_secs(30), || {
        let windows = fx.windows();
        if windows.contains("pay--api-a1") { Err(format!("{windows}\nlog: {}", fx.log(ATTEMPT))) } else { Ok(()) }
    });
    assert_eq!(fx.attempt(ATTEMPT)["status"], "merging");

    fs::write(&release, "").unwrap();
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
    // time-claim: no `list-panes` over six polls of a running Worker; no per-poll signal from the driver (that
    // it forks nothing at a poll is the claim).
    std::thread::sleep(POLL * 6);
    let during = list_panes_calls(&fx) - before;
    assert_eq!(during, 0, "list-panes calls in six polls of a running Worker\nlog: {}", fx.log(ATTEMPT));

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

/// hs-n278: holds `locks/store.lock` as a process that migrates the store does (contract §1.4: the lock, and its
/// marker in the file) and releases it after `hold`. Returns once the lock is held.
fn migrating_for(env: &TestEnv, hold: Duration) -> std::thread::JoinHandle<()> {
    use nix::fcntl::{Flock, FlockArg};
    use std::io::Write;
    let locks = env.cpd_home().join("locks");
    fs::create_dir_all(&locks).unwrap();
    let file = OpenOptions::new().create(true).append(true).open(locks.join("store.lock")).unwrap();
    let mut lock = Flock::lock(file, FlockArg::LockExclusiveNonblock).map_err(|(_, e)| e).unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
    let marker = json!({"migrating": true, "pid": std::process::id(), "started_unix_ms": now, "from": 3, "to": 20, "command": "cpd migrating-holder"});
    lock.write_all(marker.to_string().as_bytes()).unwrap();
    std::thread::spawn(move || {
        std::thread::sleep(hold);
        let _ = lock.set_len(0);
        drop(lock);
    })
}

/// hs-n278: a driver that starts while another cpd migrates the store waits for it, as it waits for a busy
/// store (contract §3.4), instead of dying `lock_timeout` after `store_busy_ms`. Its Attempt is not
/// `starting`, so once the store opens it refuses with `illegal_transition`, which proves it got that far.
#[test]
fn a_driver_waits_out_a_migration_longer_than_store_busy_ms() {
    let fx = fx_with("[timeouts]\nstore_busy_ms = 300\n");
    let lock = starting(&fx);
    fx.fixture("UPDATE attempts SET status = 'running' WHERE plan = 'pay' AND id = 'api-a1';");
    let release = migrating_for(&fx.env, Duration::from_millis(1500));
    let start = Instant::now();
    let out = fx
        .env
        .command("sh")
        .args(["-c", "exec 3<\"$1\"; shift; exec \"$@\"", "sh"])
        .arg(&lock)
        .args([CPD, "attempt", "run", ATTEMPT, "--plan", PLAN])
        .output()
        .unwrap();
    let waited = start.elapsed();
    release.join().unwrap();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["error"]["code"], "illegal_transition", "{out:?}");
    assert!(waited >= Duration::from_millis(1000), "did not wait: {waited:?}");
    assert!(String::from_utf8_lossy(&out.stderr).contains("waiting for cpd pid"), "{out:?}");
}

/// hs-n278: a driver that ends on a store newer than its binary (an install raced a migration) leaves the marker
/// E2 reads, so the Attempt is not its Worker's failure. It ends no Attempt itself.
#[test]
fn a_driver_ending_on_store_too_new_leaves_the_infra_marker_and_ends_nothing() {
    let fx = fx();
    let lock = starting(&fx);
    // Above any user_version this binary knows (contract §4.5).
    rusqlite::Connection::open(fx.home().db()).unwrap().pragma_update(None, "user_version", 4999).unwrap();
    let plan = PlanId::parse(PLAN).unwrap();
    let attempt = AttemptId::parse(ATTEMPT).unwrap();
    let marker = fx.home().attempt_infra(&plan, &attempt);
    assert!(!marker.exists());

    let out = fx
        .env
        .command("sh")
        .args(["-c", "exec 3<\"$1\"; shift; exec \"$@\"", "sh"])
        .arg(&lock)
        .args([CPD, "attempt", "run", ATTEMPT, "--plan", PLAN])
        .output()
        .unwrap();

    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["error"]["code"], "store_too_new", "{out:?}");
    assert_eq!(fx.home().infra(&plan, &attempt).unwrap().map(|(infra, _)| infra.code()), Some("store_too_new"));
}

/// Another failure of the driver leaves no marker: the ladder counts it.
#[test]
fn a_driver_refusing_for_another_reason_leaves_no_infra_marker() {
    let fx = fx();
    let lock = starting(&fx);
    fx.fixture("UPDATE attempts SET status = 'running' WHERE plan = 'pay' AND id = 'api-a1';");
    let out = fx
        .env
        .command("sh")
        .args(["-c", "exec 3<\"$1\"; shift; exec \"$@\"", "sh"])
        .arg(&lock)
        .args([CPD, "attempt", "run", ATTEMPT, "--plan", PLAN])
        .output()
        .unwrap();
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["error"]["code"], "illegal_transition", "{out:?}");
    let marker = fx.home().attempt_infra(&PlanId::parse(PLAN).unwrap(), &AttemptId::parse(ATTEMPT).unwrap());
    assert!(!marker.exists());
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
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM attempts WHERE job = 'api'"), 0);
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
    fx.env.cpd(&["job", "retry", JOB, "--tier", "S4", "--plan", PLAN]).ok();

    let out = fx.dispatch(JOB).ok();

    assert_eq!((&out["attempt"], &out["tier"], &out["origin"]), (&json!("api-a2"), &json!("S4"), &json!("retry")));
    let (attempt, _) = fx.wait_end("api-a2", &out["pid"]);
    assert_eq!(attempt["origin"], "retry");
    assert_eq!(fx.event("api-a2", "attempt-started")["data"], json!({"number": 2, "tier": "S4", "origin": "retry"}));
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

// ---------- the open Job and the update Job (contract §7.4, §7.7; hs-d340, hs-d490, hs-d491, hs-d497) ----------

/// A Worker that only submits (an `open` or `update` Job's Worker whose base did not move: nothing to merge), writing the answer to
/// `../submit-<attempt>.json`; a refused submit ends it (exit 4), as a Worker that gives up does.
const SUBMIT_ONLY: &str = r#""$CPD" attempt submit "$CPD_ATTEMPT" > ../submit-$CPD_ATTEMPT.json 2>&1 || exit 4
sleep 120"#;

/// A Worker whose base conflicts: it submits (refused `base_conflict`), merges `origin/main`, resolves `README.md`, commits and submits again
/// (a second refusal ends it).
const MERGE_THE_BASE: &str = r#""$CPD" attempt submit "$CPD_ATTEMPT" > ../refused-$CPD_ATTEMPT.json 2>&1
git fetch -q origin main
git merge --no-edit origin/main > /dev/null 2>&1
echo resolved > README.md
git add -A && git commit -qm "merge the base"
"$CPD" attempt submit "$CPD_ATTEMPT" > ../submit-$CPD_ATTEMPT.json 2>&1 || exit 4
sleep 120"#;

/// The fake `gh`: no pull request of the branch yet, `pr create` answers [`PR_URL`], and the lookup of the pull request answers it.
fn gh_creates(env: &TestEnv) {
    fs::write(env.root().join("gh-lookup.json"), common::pay_lookup()).unwrap();
    env.fake_gh(&format!(
        "case \"$1 $2\" in\n\
           \"pr list\") echo '[]' ;;\n\
           \"pr create\") echo '{PR_URL}' ;;\n\
           \"api graphql\") cat '{}/gh-lookup.json' ;;\n\
           *) echo \"unexpected gh call: $*\" >&2; exit 9 ;;\n\
         esac",
        env.root().display()
    ));
}

impl Fx {
    /// Job `key` of PR `repo` as cpd adds an `open` or `update` Job: `appended`, no Dependencies, `queued` with its fingerprint.
    fn add_cpd_job(&self, key: &str, kind: &str) {
        self.fixture(&format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, kind, created_txn, created_at)
             VALUES ('pay', 'repo', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', 'draft', 1, '{kind}', cpd_txn(), 't');"
        ));
        let plan = PlanId::parse(PLAN).unwrap();
        let all = self.store().read(|conn| jobs::load(conn, &plan)).unwrap();
        let job = all.iter().find(|j| j.key == key).unwrap();
        self.fixture(&format!(
            "UPDATE jobs SET status = 'queued', fingerprint = '{}' WHERE plan = 'pay' AND key = '{key}';",
            locked_fingerprint(job)
        ));
    }

    /// What `attempt submit` printed in the Worker's pane, as [`SUBMIT_ONLY`] and [`MERGE_THE_BASE`] saved it: `<prefix>-<attempt>.json`.
    fn saved(&self, prefix: &str, attempt: &str) -> Value {
        let path = self.env.root().join("repos").join(format!("{prefix}-{attempt}.json"));
        let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        serde_json::from_str(text.lines().last().unwrap_or("")).unwrap_or_else(|e| panic!("{e}: {text}"))
    }

    /// `pr` of the open PR or the PR that is `dispatch`: `(status, pr_number, pr_opened_at)`.
    fn pr_row(&self) -> (String, Option<i64>, Option<String>) {
        self.store()
            .read(|c| Ok(c.query_row("SELECT status, pr_number, pr_opened_at FROM prs WHERE plan = 'pay' AND key = 'repo'", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?))
            .unwrap()
    }

    fn phase(&self) -> String {
        self.env.query("SELECT phase FROM plans WHERE id = 'pay'")
    }

    /// The Events of `kind` of Plan `pay`: `(txn, data)`, in order.
    fn plan_events(&self, kind: &str) -> Vec<(i64, Value)> {
        self.store()
            .read(|c| {
                let mut stmt = c.prepare("SELECT txn, data FROM events WHERE plan = 'pay' AND kind = ?1 ORDER BY id")?;
                let rows = stmt
                    .query_map([kind], |r| Ok((r.get::<_, i64>(0)?, serde_json::from_str(&r.get::<_, String>(1)?).unwrap())))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                Ok(rows)
            })
            .unwrap()
    }

    /// `gh`'s calls that are not the poll's or a lookup's: `pr list`, `pr create`.
    fn gh_pr_calls(&self) -> Vec<Vec<String>> {
        self.env.calls("gh").into_iter().filter(|c| c[0] == "pr").collect()
    }

    fn remote(&self, origin: &std::path::Path, branch: &str) -> String {
        self.env.git(origin, &["rev-parse", &format!("refs/heads/{branch}")])
    }

    /// What the Gate wrote: the lines `slot=<n> head=<sha>` of `<root>/gate.log`, one per run of `pre-merge`.
    fn gate_runs(&self) -> Vec<String> {
        fs::read_to_string(self.env.root().join("gate.log")).unwrap_or_default().lines().map(str::to_owned).collect()
    }

    /// Moves `main` on `origin` (through the main checkout) with a commit writing `path`.
    fn advance_base(&self, path: &str, content: &str) -> String {
        fs::write(self.repo.join(path), content).unwrap();
        self.env.git(&self.repo, &["add", "-A"]);
        self.env.git(&self.repo, &["commit", "-q", "-m", &format!("base {path}")]);
        self.env.git(&self.repo, &["push", "-q", "origin", "main"]);
        self.env.git(&self.repo, &["rev-parse", "HEAD"])
    }
}

/// PR `repo` as `pr start` leaves it, with a bare `origin` that holds `main` and the Lead Branch as the lock pushed it, the policy
/// `pr.gate = "local"` with a `pre-merge` hook that notes the slot and the commit it gates, Job `api` merged (its commit `api.txt` is on
/// the Lead Branch, **not pushed**), a fake `gh` that creates pull request 812, and the PR's `open` Job `open-repo` queued. Returns the
/// bare `origin`.
fn open_fx() -> (Fx, PathBuf) {
    let fx = fx();
    let env = &fx.env;
    env.owner_policy("acme", "[pr]\ngate = \"local\"\n");
    let origin = env.root().join("repos/origin.git");
    env.git(env.root(), &["init", "--bare", "-q", "-b", "main", origin.to_str().unwrap()]);
    env.local_origin(&fx.repo, &origin);
    env.git(&fx.repo, &["push", "-q", "origin", "main", "cpd/pay"]);
    fs::write(fx.lead.join("api.txt"), "api\n").unwrap();
    env.git(&fx.lead, &["add", "-A"]);
    env.git(&fx.lead, &["commit", "-q", "-m", "api"]);
    let merged = env.git(&fx.lead, &["rev-parse", "HEAD"]);
    fx.fixture(&format!(
        "UPDATE jobs SET status = 'running' WHERE key = 'api'; UPDATE jobs SET status = 'merged', merged_sha = '{merged}' WHERE key = 'api';"
    ));
    fx.wt_toml(&format!("[pre-merge]\ngate = \"echo slot=$CPD_TEST_SLOT head=$(git rev-parse HEAD) >> {}/gate.log\"\n", env.root().display()));
    gh_creates(env);
    fx.add_cpd_job("open-repo", "open");
    (fx, origin)
}

/// The same with the pull request already open (812, as `pr open` leaves it), the Plan in `deploy`, and no `open` Job.
fn open_pr_fx() -> (Fx, PathBuf) {
    let (fx, origin) = open_fx();
    fx.fixture(&format!(
        "UPDATE jobs SET status = 'cancelled', cancel_reason = 'opened', cancel_requested_at = 't' WHERE key = 'open-repo';
         INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, mergeable, merge_state, created_at, merged_at, changed_at)
           VALUES ('acme/pay', 812, '{PR_URL}', '{PR_TITLE}', 0, 'open', 'none', 'none', 'mergeable', 'clean', 'opened', NULL, 'opened');
         UPDATE prs SET status = 'open', pr_number = 812, pr_url = '{PR_URL}', pr_opened_at = 'opened', pull_request = (SELECT id FROM pull_requests) WHERE key = 'repo';
         UPDATE plans SET phase = 'deploy' WHERE id = 'pay';"
    ));
    (fx, origin)
}

/// Runs Job `key` to its end: dispatches it, waits for its Attempt `<key>-a1` and its driver, returns the Attempt.
fn run_job(fx: &Fx, key: &str) -> Value {
    let out = fx.dispatch(key).ok();
    fx.wait_end(&format!("{key}-a1"), &out["pid"]).0
}

/// A Worker that did nothing, with the base unmoved or moved without a conflict, submits the Lead Branch as it is: its merge merges
/// nothing (the Lead Branch does not move), runs the Gate in a test slot at the Lead tip, pushes it, and runs `gh pr create` (never with
/// `--draft`), writing `pr open`'s rows and Events in the transaction that ends the Job `merged`.
#[test]
fn an_open_job_with_no_conflict_merges_nothing_gates_in_a_slot_pushes_and_opens_the_pull_request() {
    let (fx, origin) = open_fx();
    fx.pi(SUBMIT_ONLY);
    let tip = fx.lead_sha();
    // The base moved, without a conflict: that is no reason to merge it.
    let base = fx.advance_base("base.txt", "base\n");
    assert_ne!(fx.remote(&origin, "cpd/pay"), tip, "the Lead's commit is not pushed yet");

    let attempt = run_job(&fx, "open-repo");

    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("merged"), &json!("merged")), "{attempt}");
    assert_eq!(fx.saved("submit", "open-repo-a1")["ok"], true);
    assert_eq!(fx.job_status("open-repo"), "merged");
    // Nothing merged: the Lead Branch is where it was, and does not hold the base.
    assert_eq!(fx.lead_sha(), tip);
    assert_eq!(attempt["merged_sha"], tip);
    assert!(!fx.lead.join("base.txt").exists());
    assert_ne!(fx.env.git(&fx.repo, &["merge-base", &tip, &base]), base, "the base is not an ancestor of the Lead Branch");
    // The Gate ran once, at the Lead tip, in a test slot; its gate worktree is gone.
    assert_eq!(fx.gate_runs(), [format!("slot=0 head={tip}")]);
    let log = fs::read_to_string(fx.env.cpd_home().join("run/pay/logs/pr-repo.pre-merge.log")).unwrap();
    assert!(log.starts_with("gate: slot 0 taken after "), "{log}");
    let left: Vec<String> = fs::read_dir(fx.repo.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).filter(|n| n.contains(".gate-")).collect();
    assert!(left.is_empty(), "{left:?}");
    // Pushed, and the pull request created: ready, never a draft.
    assert_eq!(fx.remote(&origin, "cpd/pay"), tip);
    let calls = fx.gh_pr_calls();
    assert_eq!(calls[0], ["pr", "list", "--head", "cpd/pay", "--state", "all", "--json", "number,url,state"]);
    let created: Vec<&Vec<String>> = calls.iter().filter(|c| c[1] == "create").collect();
    assert_eq!(created.len(), 1, "{calls:?}");
    assert_eq!(created[0][..8], ["pr", "create", "--base", "main", "--head", "cpd/pay", "--title", PR_TITLE]);
    assert!(fx.env.calls("gh").iter().all(|c| !c.iter().any(|a| a == "--draft" || a == "ready" || a == "convert-to-draft")), "never a draft");
    // `pr open`'s rows and Events, with `job-merged`, in one transaction.
    let (status, number, opened_at) = fx.pr_row();
    assert_eq!((status.as_str(), number, opened_at.is_some()), ("open", Some(812), true));
    assert_eq!(fx.phase(), "deploy");
    assert_eq!(fx.env.query::<String>("SELECT pr_url FROM prs WHERE plan = 'pay'"), PR_URL);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM pull_requests WHERE repo = 'acme/pay' AND number = 812 AND state = 'open' AND draft = 0"), 1);
    assert_eq!(fx.env.query::<Option<i64>>("SELECT pull_request FROM prs WHERE plan = 'pay'").is_some(), true);
    let (merged, opened, phase) = (fx.plan_events("job-merged"), fx.plan_events("pr-opened"), fx.plan_events("plan-phase"));
    let ours: Vec<&(i64, Value)> = merged.iter().filter(|e| e.1["sha"] == tip.as_str() && e.1["kind"] == "open").collect();
    assert_eq!(ours.len(), 1, "{merged:?}");
    assert_eq!((&ours[0].1["pr"], &ours[0].1["number"], &ours[0].1["url"]), (&json!("repo"), &json!(812), &json!(PR_URL)));
    assert_eq!(opened.iter().map(|e| e.1.clone()).collect::<Vec<_>>(), [json!({"pr": "repo", "number": 812, "url": PR_URL})]);
    assert_eq!((phase.len(), &phase[0].1["from"], &phase[0].1["to"]), (1, &json!("dispatch"), &json!("deploy")));
    assert_eq!((opened[0].0, phase[0].0), (ours[0].0, ours[0].0), "one transaction");
}

/// With a base that conflicts with the Lead Branch, the Worker's merge commit is required: the submit is refused `base_conflict` (before
/// the Gate, so the Gate never runs on a head that the base would refuse), with the files and the commands, until the Worker has merged
/// the base and resolved it. The merge commit then lands as it is (no rebase), the Gate runs on it, and the pull request opens.
#[test]
fn an_open_job_with_a_conflicting_base_is_refused_base_conflict_until_the_worker_merges_the_base() {
    let (fx, origin) = open_fx();
    fx.pi(MERGE_THE_BASE);
    // The Lead Branch changed README.md, and so did the base.
    fs::write(fx.lead.join("README.md"), "mine\n").unwrap();
    fx.env.git(&fx.lead, &["add", "-A"]);
    fx.env.git(&fx.lead, &["commit", "-q", "-m", "mine"]);
    let tip = fx.lead_sha();
    let base = fx.advance_base("README.md", "theirs\n");

    let attempt = run_job(&fx, "open-repo");

    let refused = fx.saved("refused", "open-repo-a1");
    assert_eq!(refused["ok"], false, "{refused}");
    assert_eq!(refused["error"]["code"], "base_conflict");
    let details = &refused["error"]["details"];
    assert_eq!((&details["pr"], &details["base"], &details["base_sha"], &details["sha"], &details["files"]), (&json!("repo"), &json!("main"), &json!(base), &json!(tip), &json!(["README.md"])));
    let next = details["next"].as_array().unwrap();
    assert!(next[0].as_str().unwrap().starts_with("git -C ") && next[0].as_str().unwrap().ends_with(" merge --no-edit origin/main"), "{next:?}");
    assert_eq!(next[1], "cpd attempt submit open-repo-a1");

    // Then the Worker's merge commit lands as it is, and the PR opens.
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("merged"), &json!("merged")), "{attempt}");
    assert_eq!(fx.saved("submit", "open-repo-a1")["ok"], true);
    let new = fx.lead_sha();
    assert_ne!(new, tip);
    assert_eq!(fx.env.git(&fx.repo, &["rev-parse", &format!("{new}^1")]), tip);
    assert_eq!(fx.env.git(&fx.repo, &["rev-parse", &format!("{new}^2")]), base);
    assert_eq!(fs::read_to_string(fx.lead.join("README.md")).unwrap(), "resolved\n");
    assert_eq!(fx.gate_runs(), [format!("slot=0 head={new}")], "the Gate ran once, on the resolved head");
    assert_eq!(fx.remote(&origin, "cpd/pay"), new);
    assert_eq!(fx.gh_pr_calls().iter().filter(|c| c[1] == "create").count(), 1);
    assert_eq!(fx.pr_row().0, "open");
    assert_eq!(fx.job_status("open-repo"), "merged");
}

/// A Gate failure fails the Attempt like any other (§7.7): the submit is refused `gate_failed` (the Worker fixes it), and nothing is pushed,
/// created or written. A Worker that does not fix it ends crashed, through the ladder.
#[test]
fn an_open_jobs_gate_failure_is_gate_failed_and_pushes_nothing_creates_nothing_and_fails_the_attempt() {
    let (fx, origin) = open_fx();
    fx.wt_toml("[pre-merge]\ngate = \"echo the gate says no; exit 1\"\n");
    fx.pi(SUBMIT_ONLY);
    let before = fx.remote(&origin, "cpd/pay");

    let attempt = run_job(&fx, "open-repo");

    let refused = fx.saved("submit", "open-repo-a1");
    assert_eq!((&refused["ok"], &refused["error"]["code"]), (&json!(false), &json!("gate_failed")), "{refused}");
    assert!(refused["error"]["details"]["tail"].as_str().unwrap().contains("the gate says no"), "{refused}");
    assert!(refused["error"]["details"]["log"].as_str().unwrap().ends_with("pr-repo.pre-merge.log"), "{refused}");
    assert_eq!(attempt["outcome"], "failed");
    assert_ne!(fx.job_status("open-repo"), "merged");
    assert_eq!(fx.remote(&origin, "cpd/pay"), before, "nothing was pushed");
    assert!(fx.gh_pr_calls().iter().all(|c| c[1] != "create"), "{:?}", fx.gh_pr_calls());
    assert_eq!((fx.pr_row().0.as_str(), fx.phase().as_str()), ("dispatch", "dispatch"));
    assert!(fx.plan_events("pr-opened").is_empty());
}

/// A `gh` failure after the fast-forward ends the Attempt `failed` / `crashed` with the detail `gh_failed`: the push stands, the PR stays
/// `dispatch`, and the Job goes through the ladder (the next Attempt finds the pull request, or creates it).
#[test]
fn an_open_jobs_gh_failure_after_the_push_ends_the_attempt_crashed_with_gh_failed_and_the_pr_stays_dispatch() {
    let (fx, origin) = open_fx();
    fx.env.fake_gh("case \"$1 $2\" in\n\"pr list\") echo '[]' ;;\n*) echo 'gh: boom' >&2; exit 1 ;;\nesac");
    fx.pi(SUBMIT_ONLY);
    let tip = fx.lead_sha();

    let attempt = run_job(&fx, "open-repo");

    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("failed"), &json!("crashed")), "{attempt}");
    let retry = fx.event("open-repo-a1", "attempt-retry");
    assert!(retry["data"]["feedback"].as_str().unwrap().contains("gh_failed"), "{retry}");
    assert_eq!(fx.job_status("open-repo"), "queued", "a ladder cause");
    assert_eq!(fx.remote(&origin, "cpd/pay"), tip, "the push happened before gh");
    assert_eq!((fx.pr_row().0.as_str(), fx.phase().as_str()), ("dispatch", "dispatch"));
    assert!(fx.plan_events("pr-opened").is_empty());
}

/// The update Job on a conflict: the same path as the open Job without `gh`. The Worker merges the base (the submit refuses until it has),
/// cpd gates and pushes the resolution in the merge; the pull request is already open, so nothing is created and no PR row changes.
#[test]
fn an_update_job_with_a_conflicting_base_is_refused_until_the_worker_merges_it_and_its_merge_pushes_the_resolution() {
    let (fx, origin) = open_pr_fx();
    fx.add_cpd_job("up-repo", "update");
    fx.pi(MERGE_THE_BASE);
    fs::write(fx.lead.join("README.md"), "mine\n").unwrap();
    fx.env.git(&fx.lead, &["add", "-A"]);
    fx.env.git(&fx.lead, &["commit", "-q", "-m", "mine"]);
    let tip = fx.lead_sha();
    let base = fx.advance_base("README.md", "theirs\n");
    let row = fx.pr_row();

    let attempt = run_job(&fx, "up-repo");

    let refused = fx.saved("refused", "up-repo-a1");
    assert_eq!((&refused["error"]["code"], &refused["error"]["details"]["files"], &refused["error"]["details"]["base_sha"]), (&json!("base_conflict"), &json!(["README.md"]), &json!(base)), "{refused}");
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("merged"), &json!("merged")), "{attempt}");
    let new = fx.lead_sha();
    assert_eq!(fx.env.git(&fx.repo, &["rev-parse", &format!("{new}^1")]), tip);
    assert_eq!(fx.remote(&origin, "cpd/pay"), new, "its merge pushes the resolution");
    assert_eq!(fx.gate_runs(), [format!("slot=0 head={new}")]);
    assert!(fx.gh_pr_calls().is_empty(), "the pull request exists: {:?}", fx.gh_pr_calls());
    assert_eq!(fx.pr_row(), row, "an update Job writes no PR row");
    assert!(fx.plan_events("pr-opened").is_empty());
    let merged: Vec<Value> = fx.plan_events("job-merged").into_iter().map(|e| e.1).filter(|d| d["kind"] == "update").collect();
    assert_eq!(merged.len(), 1, "{merged:?}");
}

/// A feedback fix Job on an open PR reaches the remote through the update path (hs-d497): `job add` queues the PR's `update` Job in its own
/// transaction (once, however many fixes are added; `reason` `feedback`), which is Ready only when the fixes have merged, gates the Lead
/// Branch in a slot and pushes it. No `pr rebase` or `pr push` is run by anyone.
#[test]
fn a_feedback_fix_job_on_an_open_pr_reaches_the_remote_through_the_update_job() {
    let (fx, origin) = open_pr_fx();
    let spec = fx.env.root().join("fix-spec.json");
    fs::write(&spec, r#"{"goal": "Fix it.", "context": "The checks failed.", "touches": ["fix.txt"], "acceptance": ["ok"], "out_of_scope": []}"#).unwrap();
    let add = |key: &str| fx.env.cpd(&["job", "add", key, "--title", "Fix the checks", "--tier", "S2", "--spec", spec.to_str().unwrap(), "--pr", "repo", "--plan", "pay"]).ok();
    assert_eq!(add("fix")["job"]["status"], "queued");
    assert_eq!(add("fix2")["job"]["status"], "queued");

    // The Job `job add` queued: one, `up-repo`, from `job add`, for the feedback.
    let update: Vec<(String, String)> = fx.env.read(|c| {
        let mut stmt = c.prepare("SELECT key, status FROM jobs WHERE kind = 'update' ORDER BY key")?;
        Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
    });
    assert_eq!(update, [("up-repo".to_owned(), "queued".to_owned())]);
    let added: Vec<Value> = fx.plan_events("job-added").into_iter().map(|e| e.1).filter(|d| d["kind"] == "update").collect();
    assert_eq!(added.len(), 1, "{added:?}");
    assert_eq!((&added[0]["by"], &added[0]["reason"], &added[0]["pr"]), (&json!("job add"), &json!("feedback"), &json!("repo")));
    // It waits for the fixes: not Ready while they are not merged.
    refused(&fx.dispatch("up-repo"), "job_not_ready", 3);

    // The merges of the fixes push best-effort (R35); make those pushes fail, as they do when origin is unreachable, so that what reaches
    // the pull request is the update Job's.
    let hook = origin.join("hooks/pre-receive");
    common::write_executable(&hook, "#!/bin/sh\necho nope >&2\nexit 1\n");
    fx.pi(&format!(
        "case \"$CPD_ATTEMPT\" in\n\
           fix-a1) echo fixed > fix.txt; git add -A; git commit -qm fix ;;\n\
           fix2-a1) echo fixed2 > fix2.txt; git add -A; git commit -qm fix2 ;;\n\
         esac\n{SUBMIT_ONLY}"
    ));
    run_job(&fx, "fix");
    run_job(&fx, "fix2");
    let lead = fx.lead_sha();
    assert!(fx.lead.join("fix.txt").exists() && fx.lead.join("fix2.txt").exists());
    assert_ne!(fx.remote(&origin, "cpd/pay"), lead, "the fixes' own pushes failed: the remote lacks them");
    fs::remove_file(&hook).unwrap();
    let before = fx.gate_runs();

    let attempt = run_job(&fx, "up-repo");

    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("merged"), &json!("merged")), "{attempt}");
    assert_eq!(fx.saved("submit", "up-repo-a1")["ok"], true);
    assert_eq!(fx.lead_sha(), lead, "nothing to merge: the base did not move");
    assert_eq!(fx.remote(&origin, "cpd/pay"), lead);
    assert_eq!(fx.gate_runs(), [before, vec![format!("slot=0 head={lead}")]].concat(), "the Gate ran on the Lead tip, in a slot");
    assert!(fx.gh_pr_calls().is_empty());
}

/// The approval of a review queues the update Job for the fix drafts it queues on an open PR (hs-d497 (2); the other cause of `job add`): one
/// Job whatever the number of drafts, `reason` `feedback`, in the transaction of the approval. A draft of no open PR queues none.
#[test]
fn the_approval_of_a_review_queues_the_update_job_for_a_fix_draft_of_an_open_pr() {
    let (fx, _origin) = open_pr_fx();
    let spec = fx.env.root().join("fix-spec.json");
    fs::write(&spec, r#"{"goal": "Fix it.", "context": "A reviewer asked.", "touches": ["fix.txt"], "acceptance": ["ok"], "out_of_scope": []}"#).unwrap();
    for key in ["fix", "fix2"] {
        fx.env.cpd(&["job", "add", key, "--title", "Fix", "--tier", "S2", "--spec", spec.to_str().unwrap(), "--pr", "repo", "--plan", "pay", "--draft"]).ok();
    }
    let update_jobs = || -> Vec<(String, String)> {
        fx.env.read(|c| {
            let mut stmt = c.prepare("SELECT key, status FROM jobs WHERE kind = 'update' ORDER BY key")?;
            Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    };
    assert_eq!(update_jobs(), Vec::<(String, String)>::new(), "a draft queues nothing");
    fx.env.outline_for_review("pay");
    let review = fx.env.cpd(&["plan", "review", "--plan", "pay", "--title", "Fixes"]).ok();

    fx.env.cpd(&["plan", "decide", review["review"]["id"].as_str().unwrap(), "--approve", "--plan", "pay"]).ok();

    assert_eq!(update_jobs(), [("up-repo".to_owned(), "queued".to_owned())]);
    let added: Vec<Value> = fx.plan_events("job-added").into_iter().map(|e| e.1).filter(|d| d["kind"] == "update").collect();
    assert_eq!(added.len(), 1, "{added:?}");
    assert_eq!((&added[0]["reason"], &added[0]["pr"]), (&json!("feedback"), &json!("repo")));
}

/// The newest pull request of the Lead Branch is already `merged` on GitHub and cpd never recorded it (§7.7 submit step 1, §7.4 step 8b): the `open`
/// Job adopts it. Its submit runs no Gate, its merge pushes nothing and creates nothing, and step 10 writes the rows of `pr open` step 0: the PR
/// `merged`, `pr-merged` (no `pr-opened`), and the Plan `done` with the last PR.
#[test]
fn an_open_job_adopts_a_pull_request_merged_on_github_with_no_gate_and_no_push() {
    let (fx, origin) = open_fx();
    let merged = common::pay_lookup().replace("\"state\":\"OPEN\"", "\"state\":\"MERGED\"").replace("\"mergedAt\":null", "\"mergedAt\":\"2026-10-20T12:00:00Z\"");
    fs::write(fx.env.root().join("gh-lookup.json"), merged).unwrap();
    fx.env.fake_gh(&format!(
        "case \"$1 $2\" in\n\"pr list\") echo '[{{\"number\":812,\"url\":\"{PR_URL}\",\"state\":\"MERGED\"}}]' ;;\n\"api graphql\") cat '{}/gh-lookup.json' ;;\n*) echo \"unexpected gh call: $*\" >&2; exit 9 ;;\nesac",
        fx.env.root().display()
    ));
    fx.pi(SUBMIT_ONLY);
    let before = fx.remote(&origin, "cpd/pay");

    let attempt = run_job(&fx, "open-repo");

    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("merged"), &json!("merged")), "{attempt}");
    assert_eq!(fx.saved("submit", "open-repo-a1")["ok"], true);
    assert_eq!(fx.gate_runs(), Vec::<String>::new(), "no Gate");
    assert_eq!(fx.remote(&origin, "cpd/pay"), before, "no push");
    assert!(fx.gh_pr_calls().iter().all(|c| c[1] != "create"));
    assert_eq!(fx.pr_row().0, "merged");
    assert_eq!(fx.phase(), "done");
    assert!(fx.plan_events("pr-opened").is_empty());
    assert_eq!(fx.plan_events("pr-merged").len(), 1);
}
