//! Review Jobs (contract §2.2 Review Job, §3.4 "A review Job's Attempt", §7.3,
//! §7.4; commands.md §5.1, §6.1, §16.1): `job add --review`, a Worker that is a
//! fork of the Plan's Lead, findings in `run/<plan>/reviews/<k>.md`, a submit
//! without commits that only a review Job may make, and a merge that ends
//! `merged` and leaves the Lead Branch where it is. A fake pi stands for the Worker.

mod common;

use std::fs::{self, File};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use common::{CPD, Run, TestEnv, refused, wait_until};
use cpd_core::error::CpdError;
use cpd_core::git::{Setup, create_lead_worktree};
use cpd_core::home::CpdHome;
use cpd_core::ids::PlanId;
use cpd_core::store::{Store, WriteTxn};
use serde_json::{Value, json};

const PLAN: &str = "pay";

/// O supports efforts 0–5, S 1–5, H 0–3.
const EFFORTS: [(&str, &str, &str); 3] =
    [("O", "p/opus", "[0,1,2,3,4,5]"), ("S", "p/sonnet", "[1,2,3,4,5]"), ("H", "p/haiku", "[0,1,2,3]")];

struct Fx {
    env: TestEnv,
    repo: PathBuf,
    lead: PathBuf,
    /// The Lead's pi session file.
    lead_file: PathBuf,
}

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

/// A repo `pay` with the Lead worktree of Plan `pay` (phase `dispatch`, PR `repo`),
/// a config on this env's tmux server, a fake pi install with its Tier catalog,
/// and the Lead Session `lead1`: `idle`, with a session file whose last turn is
/// finished and quiet. Job `api` is `merged`.
fn fx() -> Fx {
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
            "version = 1\nmachine = \"m1\"\n[tiers]\nO = \"p/opus\"\nS = \"p/sonnet\"\nH = \"p/haiku\"\n[tmux]\nsocket = \"{}\"\n",
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
    for args in [&["new-session", "-d", "-s", "cpd-work", "-n", "watch", "sleep", "600"][..], &["set", "-g", "remain-on-exit", "on"]] {
        assert!(env.tmux(args).status.success(), "tmux {args:?}");
    }
    // The Lead's session file: a finished assistant turn, last written long ago.
    let lead_file = env.home().join(".pi/agent/sessions/lead/2026_lead1.jsonl");
    fs::create_dir_all(lead_file.parent().unwrap()).unwrap();
    fs::write(
        &lead_file,
        "{\"type\":\"session\"}\n{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"stopReason\":\"stop\"}}\n",
    )
    .unwrap();
    File::options().write(true).open(&lead_file).unwrap().set_modified(std::time::SystemTime::UNIX_EPOCH).unwrap();

    let fx = Fx { env, repo, lead, lead_file };
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
         INSERT INTO sessions (id, role, context, plan, name, file, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '{file}', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'lead1', 't', 't');
         INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
           VALUES ('pay', 'repo', '{repo}', 'main', 'dispatch', 'cpd/pay', '{lead}', cpd_txn(), 't', 't');",
        file = fx.lead_file.display(),
        repo = fx.repo.display(),
        lead = fx.lead.display()
    ));
    fx.insert_job("api", "merged");
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

    /// A Job of PR `repo` as the engine leaves it in `status` (no Attempt; a made-up fingerprint).
    fn insert_job(&self, key: &str, status: &str) {
        let (columns, values) = match status {
            "merged" => (", fingerprint, merged_sha", format!(", '{}', 'abc'", "a".repeat(64))),
            "failed" => (", fingerprint, failed_cause", format!(", '{}', 'retries_exhausted'", "a".repeat(64))),
            "cancelled" => (", cancel_reason, cancel_requested_at", ", 'no', 't'".to_owned()),
            _ => (", fingerprint", format!(", '{}'", "a".repeat(64))),
        };
        self.fixture(&format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at{columns})
             VALUES ('pay', 'repo', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', '{status}', 1, cpd_txn(), 't'{values});"
        ));
    }

    /// The fake `pi`: `body` runs in the Worker's pane, cwd the worktree, with `$CPD`
    /// the binary, once the Attempt is `running`.
    fn pi(&self, body: &str) {
        self.env.fake_pi(&format!(
            "CPD='{CPD}'\n\
             until \"$CPD\" attempt show \"$CPD_ATTEMPT\" | grep -q '\"status\":\"running\"'; do sleep 0.1; done\n{body}"
        ));
    }

    fn add(&self, key: &str, extra: &[&str]) -> Run {
        let spec = self.env.root().join("spec.json");
        fs::write(
            &spec,
            json!({"goal": "Review it.", "context": "Because.", "touches": [format!("run/pay/reviews/{key}.md")], "acceptance": ["findings written"], "out_of_scope": []})
                .to_string(),
        )
        .unwrap();
        let title = format!("Title of {key}");
        let mut args = vec!["job", "add", key, "--title", &title, "--tier", "S2", "--spec", spec.to_str().unwrap(), "--plan", PLAN];
        args.extend(extra);
        self.env.cpd(&args)
    }

    fn show(&self, key: &str) -> Value {
        self.env.cpd(&["job", "show", key, "--plan", PLAN]).ok()["job"].clone()
    }

    fn dispatch(&self, key: &str) -> Run {
        self.env.cpd(&["job", "dispatch", key, "--plan", PLAN])
    }

    fn attempt(&self, id: &str) -> Value {
        self.env.cpd(&["attempt", "show", id, "--plan", PLAN]).ok()["attempt"].clone()
    }

    fn wait_status(&self, id: &str, status: &str) -> Value {
        common::wait_some(&format!("Attempt {id} to become {status}"), Duration::from_secs(60), || {
            let attempt = self.attempt(id);
            if attempt["status"] == status { Ok(attempt) } else { Err(format!("{attempt}\nlog: {}", self.log(id))) }
        })
    }

    /// Waits until the Attempt has ended and its driver has exited.
    fn wait_end(&self, id: &str, pid: &Value) -> Value {
        let attempt = self.wait_status(id, "ended");
        let pid = nix::unistd::Pid::from_raw(pid.as_i64().unwrap() as i32);
        common::wait_some(&format!("the driver of {id} to exit"), Duration::from_secs(30), || {
            if nix::sys::signal::kill(pid, None).is_ok() { Err(format!("log: {}", self.log(id))) } else { Ok(()) }
        });
        attempt
    }

    fn log(&self, id: &str) -> String {
        fs::read_to_string(self.env.cpd_home().join(format!("run/pay/logs/attempt-{id}.log"))).unwrap_or_default()
    }

    fn lead_sha(&self) -> String {
        self.env.git(&self.repo, &["rev-parse", "refs/heads/cpd/pay"])
    }

    /// What a Worker's `attempt submit` printed, as the fake pi saved it.
    fn submitted(&self, name: &str) -> Value {
        let path = self.repo.parent().unwrap().join(name);
        let deadline = Instant::now() + Duration::from_secs(60);
        while !path.exists() {
            assert!(Instant::now() < deadline, "the Worker never wrote {name}");
            std::thread::sleep(Duration::from_millis(100));
        }
        let text = fs::read_to_string(&path).unwrap();
        serde_json::from_str(text.trim()).unwrap_or_else(|e| panic!("{name}: {e}: {text}"))
    }
}

/// A review Worker: it submits with no findings, then with a commit and findings, then
/// as it should: findings and no commit. Each answer is saved next to the repos.
const REVIEWER: &str = r#"out="$CPD_HOME/run/pay/reviews/final.md"
"$CPD" attempt submit "$CPD_ATTEMPT" > ../submit-1.json 2>&1
mkdir -p "$(dirname "$out")"
echo "- one finding" > "$out"
echo x > x.txt; git add -A; git commit -qm x
"$CPD" attempt submit "$CPD_ATTEMPT" > ../submit-2.json 2>&1
git reset -q --hard HEAD~1
"$CPD" attempt submit "$CPD_ATTEMPT" > ../submit-3.json 2>&1
sleep 120"#;

// ---------- job add --review ----------

#[test]
fn a_review_job_needs_every_existing_job_of_its_pr_that_is_not_cancelled() {
    let fx = fx();
    fx.insert_job("web", "queued");
    fx.insert_job("old", "cancelled");

    let out = fx.add("final", &["--review"]).ok();

    assert_eq!(out["job"]["needs"], json!(["api", "web"]));
    assert_eq!(out["job"]["review"], true);
    assert_eq!(out["job"]["status"], "queued");
    assert_eq!(out["job"]["appended"], true);
    assert_eq!(fx.show("final")["needs"], json!(["api", "web"]));
    assert_eq!(fx.show("final")["review"], true);
    assert_eq!(fx.show("api")["review"], false);
    let listed = fx.env.cpd(&["job", "list", "--plan", PLAN]).ok()["jobs"].clone();
    let flag = |k: &str| listed.as_array().unwrap().iter().find(|j| j["key"] == k).unwrap()["review"].clone();
    assert_eq!((flag("final"), flag("api")), (json!(true), json!(false)));
    let shown = fx.env.cpd(&["plan", "show", "--plan", PLAN]).ok()["jobs"].clone();
    let flag = |k: &str| shown.as_array().unwrap().iter().find(|j| j["key"] == k).unwrap()["review"].clone();
    assert_eq!((flag("final"), flag("api")), (json!(true), json!(false)));
    assert_eq!(fx.env.query::<i64>("SELECT review FROM jobs WHERE plan = 'pay' AND key = 'final'"), 1);
    assert_eq!(fx.env.query::<i64>("SELECT review FROM jobs WHERE plan = 'pay' AND key = 'api'"), 0);
    let events = fx.env.cpd(&["events", "list", "--kind", "job-added", "--plan", PLAN]).ok()["events"].as_array().unwrap().clone();
    let added = events.iter().find(|e| e["job"] == "final").expect("job-added");
    assert_eq!(added["data"]["review"], true);
    assert_eq!(added["data"]["needs"], json!(["api", "web"]));
}

#[test]
fn a_job_added_later_does_not_become_a_dependency_of_the_review_job() {
    let fx = fx();
    fx.add("final", &["--review"]).ok();

    fx.add("fix", &[]).ok();

    assert_eq!(fx.show("final")["needs"], json!(["api"]));
    assert_eq!(fx.show("fix")["needs"], json!([]));
}

#[test]
fn a_review_add_is_idempotent_until_another_job_is_added() {
    let fx = fx();
    fx.add("final", &["--review"]).ok();
    let before: i64 = fx.env.query("SELECT count(*) FROM txns");

    let again = fx.add("final", &["--review"]).ok();

    assert_eq!(again["already"], true);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM txns"), before, "a rerun writes nothing");
    fx.add("fix", &[]).ok();
    refused(&fx.add("final", &["--review"]), "job_key_exists", 3);
    // The same key without --review is another Job.
    refused(&fx.add("final", &[]), "job_key_exists", 3);
}

#[test]
fn a_review_job_takes_no_needs_and_refuses_a_failed_job_of_its_pr() {
    let fx = fx();
    let run = fx.add("final", &["--review", "--needs", "api"]);
    assert_eq!(run.exit, 2, "{run:?}");

    fx.insert_job("broken", "failed");
    let error = refused(&fx.add("final", &["--review"]), "needs_unavailable", 3);
    assert_eq!(error["details"]["jobs"], json!(["broken"]));
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM jobs WHERE key = 'final'"), 0);
}

// ---------- the Worker ----------

#[test]
fn the_worker_is_a_fork_of_the_lead_that_commits_nothing_and_the_merge_leaves_the_lead_branch() {
    let fx = fx();
    fx.pi(REVIEWER);
    let lead_before = fx.lead_sha();
    fx.add("final", &["--review"]).ok();

    let out = fx.dispatch("final").ok();
    let attempt = fx.wait_end("final-a1", &out["pid"]);

    // The merge: `merged` with no change to the Lead Branch, `merged_sha` its tip.
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("merged"), &json!("merged")));
    assert_eq!(fx.lead_sha(), lead_before);
    assert_eq!(attempt["merged_sha"], lead_before);
    assert_eq!(attempt["check_tail"], Value::Null);
    let job = fx.show("final");
    assert_eq!(job["status"], "merged");
    assert_eq!(job["merged_sha"], lead_before);
    assert!(fx.env.git(&fx.lead, &["status", "--porcelain"]).is_empty());
    // A9: the worktree and the branch are gone.
    assert!(!fx.repo.parent().unwrap().join("pay.pay--final-a1").exists());
    assert_eq!(fx.env.git(&fx.repo, &["branch", "--list", "cpd/pay--final-a1"]), "");

    // Its argv: a Worker's, plus `--fork <the Lead's session file>`.
    let calls = fx.env.calls("pi");
    assert_eq!(calls.len(), 1, "{calls:?}");
    let argv = &calls[0];
    let at = |flag: &str| argv[argv.iter().position(|a| a == flag).unwrap() + 1].clone();
    assert_eq!(&argv[..4], ["--approve", "--no-skills", "--exclude-tools", cpd_core::host::pi::WORKER_EXCLUDED_TOOLS]);
    assert_eq!(at("--fork"), fx.lead_file.to_str().unwrap());
    assert_eq!(at("--model"), "p/sonnet:medium");
    assert_eq!(at("-n"), "pay final a1");

    // Its Session: a Worker's, forked from the Lead.
    let session = attempt["worker_session"].as_str().unwrap().to_owned();
    assert_eq!(at("--session-id"), session);
    let row: (String, Option<String>, Option<String>) = fx
        .store()
        .read(|c| {
            Ok(c.query_row("SELECT role, attempt, forked_from FROM sessions WHERE id = ?1", [&session], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?)
        })
        .unwrap();
    assert_eq!(row, ("worker".to_owned(), Some("final-a1".to_owned()), Some("lead1".to_owned())));
    let events = fx.env.cpd(&["events", "list", "--kind", "session-registered", "--plan", PLAN]).ok()["events"].as_array().unwrap().clone();
    let registered = events.iter().find(|e| e["session"] == session.as_str()).expect("session-registered");
    assert_eq!(registered["data"]["forked_from"], "lead1");

    // Its Spec tells it to review, where to write and to commit nothing.
    let spec = fs::read_to_string(fx.env.cpd_home().join("run/pay/specs/final-a1.md")).unwrap();
    let findings = fx.env.cpd_home().join("run/pay/reviews/final.md");
    assert!(spec.starts_with("# Worker Spec: final, Attempt 1\n"), "{spec}");
    assert!(spec.contains("**review Job**") && spec.contains("commit nothing"), "{spec}");
    assert!(spec.contains("git diff main...cpd/pay"), "{spec}");
    assert!(spec.contains(findings.to_str().unwrap()), "{spec}");
    assert!(spec.contains("`review_commits`") && spec.contains("`review_missing`"), "{spec}");
    assert!(!spec.contains("no_commits") && !spec.contains("quick check"), "{spec}");
    assert!(spec.contains("### Goal\n\nReview it."), "{spec}");
    assert_eq!(fs::read_to_string(&findings).unwrap(), "- one finding\n");
}

#[test]
fn submit_without_commits_is_a_review_jobs_privilege_and_it_needs_its_findings_and_no_commit() {
    let fx = fx();
    fx.pi(REVIEWER);
    fx.add("final", &["--review"]).ok();

    let out = fx.dispatch("final").ok();
    fx.wait_end("final-a1", &out["pid"]);

    let first = fx.submitted("submit-1.json");
    assert_eq!(first["error"]["code"], "review_missing", "{first}");
    assert_eq!(first["error"]["details"]["path"], fx.env.cpd_home().join("run/pay/reviews/final.md").to_str().unwrap());
    let second = fx.submitted("submit-2.json");
    assert_eq!(second["error"]["code"], "review_commits", "{second}");
    let third = fx.submitted("submit-3.json");
    assert_eq!(third["ok"], true, "{third}");
}

#[test]
fn a_normal_job_without_commits_is_still_refused() {
    let fx = fx();
    fx.add("impl", &[]).ok();
    fx.pi(
        r#""$CPD" attempt submit "$CPD_ATTEMPT" > ../submit-1.json 2>&1
echo x > impl.txt; git add -A; git commit -qm x
"$CPD" attempt submit "$CPD_ATTEMPT" > ../submit-2.json 2>&1
sleep 120"#,
    );

    let out = fx.dispatch("impl").ok();
    fx.wait_end("impl-a1", &out["pid"]);

    let first = fx.submitted("submit-1.json");
    assert_eq!(first["error"]["code"], "no_commits", "{first}");
    assert_eq!(fx.submitted("submit-2.json")["ok"], true);
    assert_ne!(fx.lead_sha(), fx.env.git(&fx.repo, &["rev-parse", "main"]));
}

#[test]
fn the_worker_starts_only_once_the_lead_is_idle() {
    let fx = fx();
    fx.pi(REVIEWER);
    fx.fixture("UPDATE sessions SET state = 'working' WHERE id = 'lead1';");
    fx.add("final", &["--review"]).ok();

    let out = fx.dispatch("final").ok();
    wait_until("the driver to wait for the Lead", || fx.log("final-a1").contains("A3: waiting for the Lead to be idle"));
    // time-claim: the driver forks nothing while the Lead works, over a whole 2 s poll of the Lead (LEAD_POLL); no
    // per-poll signal from the driver (its log says only that it waits).
    std::thread::sleep(Duration::from_millis(2500));

    // Still waiting: `starting`, no Worker, no Session, nothing forked.
    assert_eq!(fx.attempt("final-a1")["status"], "starting");
    assert!(fx.env.calls("pi").is_empty());
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM sessions WHERE role = 'worker'"), 0);

    fx.fixture("UPDATE sessions SET state = 'idle' WHERE id = 'lead1';");
    let attempt = fx.wait_end("final-a1", &out["pid"]);
    assert_eq!(attempt["outcome"], "merged");
    assert_eq!(fx.env.calls("pi").len(), 1);
}

#[test]
fn a_cancel_while_the_driver_waits_for_the_lead_ends_the_attempt_cancelled() {
    let fx = fx();
    fx.pi(REVIEWER);
    fx.fixture("UPDATE sessions SET state = 'working' WHERE id = 'lead1';");
    fx.add("final", &["--review"]).ok();
    let out = fx.dispatch("final").ok();
    wait_until("the driver to wait for the Lead", || fx.log("final-a1").contains("A3: waiting for the Lead to be idle"));

    fx.env.cpd(&["job", "cancel", "final", "--reason", "not needed", "--plan", PLAN]).ok();
    let attempt = fx.wait_end("final-a1", &out["pid"]);

    assert_eq!(attempt["outcome"], "cancelled");
    assert!(fx.env.calls("pi").is_empty());
}
