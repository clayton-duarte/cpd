//! Review Jobs (contract §2.2 Review Job, §3.4 "A review Job's Attempt", §7.3,
//! §7.4; commands.md §5.1, §6.1, §16.1): `job add --review`, a Worker that is a
//! fork of the Plan's Lead, findings in `run/<plan>/reviews/<k>.md`, a submit
//! without commits that only a review Job may make, and a merge that ends
//! `merged` and leaves the Lead Branch where it is. A fake pi stands for the Worker.

use crate::common;

use std::fs::{self, File};
use std::path::PathBuf;
use std::time::Duration;

use common::{Run, refused};
use serde_json::{Value, json};

use super::driver::{fixture, store};
use super::driver_open::{Live, live};

const PLAN: &str = "pay";

/// How often a review Job's driver looks at its Lead (`LEAD_POLL`, §3.4): its naps on the `ManualClock`.
const LEAD_POLL: Duration = Duration::from_secs(2);

/// Plan `pay` as `pr start` leaves it, on the fake git, the fake tmux and the `ManualClock` ([`live`]): its Lead worktree and
/// Lead Session `lead1` (`idle`, with a session file whose last turn is finished and quiet), and Job `api` `merged`.
fn fx() -> Live {
    let live = live("");
    // A review Job's Worker is a fork of the Lead: it waits for the Lead's session file to show a finished turn.
    let file = live.fx.env.home().join(".pi/agent/sessions/lead/2026_lead1.jsonl");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, "{\"type\":\"session\"}\n{\"type\":\"message\",\"message\":{\"role\":\"assistant\",\"stopReason\":\"stop\"}}\n").unwrap();
    File::options().write(true).open(&file).unwrap().set_modified(std::time::SystemTime::UNIX_EPOCH).unwrap();
    fixture(
        &live.fx.env,
        &format!(
            "UPDATE sessions SET file = '{}' WHERE id = 'lead1';
             UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND key = 'api';
             UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE plan = 'pay' AND key = 'api';",
            file.display()
        ),
    );
    live
}

/// The Lead's pi session file.
fn lead_file(live: &Live) -> PathBuf {
    live.fx.env.home().join(".pi/agent/sessions/lead/2026_lead1.jsonl")
}

/// A Job of PR `repo` as the engine leaves it in `status` (no Attempt; a made-up fingerprint).
fn insert_job(live: &Live, key: &str, status: &str) {
    let (columns, values) = match status {
        "merged" => (", fingerprint, merged_sha", format!(", '{}', 'abc'", "a".repeat(64))),
        "failed" => (", fingerprint, failed_cause", format!(", '{}', 'retries_exhausted'", "a".repeat(64))),
        "cancelled" => (", cancel_reason, cancel_requested_at", ", 'no', 't'".to_owned()),
        _ => (", fingerprint", format!(", '{}'", "a".repeat(64))),
    };
    fixture(
        &live.fx.env,
        &format!(
            "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, created_txn, created_at{columns})
             VALUES ('pay', 'repo', '{key}', 'Title of {key}', 'S2', 'Do {key}.', 'Because.', '[\"{key}.txt\"]', '[\"it works\"]', '[]', '{status}', 1, cpd_txn(), 't'{values});"
        ),
    );
}

fn add(live: &Live, key: &str, extra: &[&str]) -> Run {
    let spec = live.fx.env.root().join("spec.json");
    fs::write(
        &spec,
        json!({"goal": "Review it.", "context": "Because.", "touches": [format!("run/pay/reviews/{key}.md")], "acceptance": ["findings written"], "out_of_scope": []})
            .to_string(),
    )
    .unwrap();
    let title = format!("Title of {key}");
    let mut args = vec!["job", "add", key, "--title", &title, "--tier", "S2", "--spec", spec.to_str().unwrap(), "--plan", PLAN];
    args.extend(extra);
    live.fx.cpd(&args)
}

fn show(live: &Live, key: &str) -> Value {
    live.fx.cpd(&["job", "show", key, "--plan", PLAN]).ok()["job"].clone()
}

/// What `attempt submit` answered the Worker, run as pi runs it in its window: cwd the worktree, its Plan and its Attempt.
fn submit(live: &Live, id: &str) -> Value {
    live.fx.cpd_with(&live.worktree(id), &["attempt", "submit", id], &[("CPD_PLAN", PLAN), ("CPD_ATTEMPT", id)]).json()
}

/// The argv of every pi the fake tmux was told to start: what follows `--` in a `new-session` or `new-window`, from `pi` on
/// (a Worker's argv has the idle prefix before it).
fn pi_calls(live: &Live) -> Vec<Vec<String>> {
    live.fx
        .calls("tmux")
        .into_iter()
        .filter(|call| call.args.iter().any(|a| a == "new-session" || a == "new-window"))
        .filter_map(|call| {
            let after = call.args.iter().position(|a| a == "--")?;
            let argv = &call.args[after + 1..];
            let pi = argv.iter().position(|a| a == "pi")?;
            Some(argv[pi + 1..].to_vec())
        })
        .collect()
}

fn log(live: &Live, id: &str) -> String {
    fs::read_to_string(live.fx.env.cpd_home().join(format!("run/pay/logs/attempt-{id}.log"))).unwrap_or_default()
}

/// A review Worker, as pi does it in its window: it submits with no findings, then with a commit and findings, then as it
/// should: findings and no commit. The three answers of `attempt submit`.
fn reviewer(live: &Live, id: &str) -> [Value; 3] {
    let worktree = live.worktree(id);
    let findings = live.fx.env.cpd_home().join("run/pay/reviews/final.md");
    let first = submit(live, id);
    fs::create_dir_all(findings.parent().unwrap()).unwrap();
    fs::write(&findings, "- one finding\n").unwrap();
    live.commit(&worktree, "x.txt", "x\n", "x");
    let second = submit(live, id);
    live.fx.git(&worktree, &["reset", "-q", "--hard", "HEAD~1"]);
    let third = submit(live, id);
    [first, second, third]
}

// ---------- job add --review ----------

#[test]
fn a_review_job_needs_every_existing_job_of_its_pr_that_is_not_cancelled() {
    let fx = fx();
    insert_job(&fx, "web", "queued");
    insert_job(&fx, "old", "cancelled");

    let out = add(&fx, "final", &["--review"]).ok();

    assert_eq!(out["job"]["needs"], json!(["api", "web"]));
    assert_eq!(out["job"]["review"], true);
    assert_eq!(out["job"]["status"], "queued");
    assert_eq!(out["job"]["appended"], true);
    assert_eq!(show(&fx, "final")["needs"], json!(["api", "web"]));
    assert_eq!(show(&fx, "final")["review"], true);
    assert_eq!(show(&fx, "api")["review"], false);
    let listed = fx.fx.cpd(&["job", "list", "--plan", PLAN]).ok()["jobs"].clone();
    let flag = |k: &str| listed.as_array().unwrap().iter().find(|j| j["key"] == k).unwrap()["review"].clone();
    assert_eq!((flag("final"), flag("api")), (json!(true), json!(false)));
    let shown = fx.fx.cpd(&["plan", "show", "--plan", PLAN]).ok()["jobs"].clone();
    let flag = |k: &str| shown.as_array().unwrap().iter().find(|j| j["key"] == k).unwrap()["review"].clone();
    assert_eq!((flag("final"), flag("api")), (json!(true), json!(false)));
    assert_eq!(fx.value::<i64>("SELECT review FROM jobs WHERE plan = 'pay' AND key = 'final'"), 1);
    assert_eq!(fx.value::<i64>("SELECT review FROM jobs WHERE plan = 'pay' AND key = 'api'"), 0);
    let events = fx.fx.cpd(&["events", "list", "--kind", "job-added", "--plan", PLAN]).ok()["events"].as_array().unwrap().clone();
    let added = events.iter().find(|e| e["job"] == "final").expect("job-added");
    assert_eq!(added["data"]["review"], true);
    assert_eq!(added["data"]["needs"], json!(["api", "web"]));
}

#[test]
fn a_job_added_later_does_not_become_a_dependency_of_the_review_job() {
    let fx = fx();
    add(&fx, "final", &["--review"]).ok();

    add(&fx, "fix", &[]).ok();

    assert_eq!(show(&fx, "final")["needs"], json!(["api"]));
    assert_eq!(show(&fx, "fix")["needs"], json!([]));
}

#[test]
fn a_review_add_is_idempotent_until_another_job_is_added() {
    let fx = fx();
    add(&fx, "final", &["--review"]).ok();
    let before: i64 = fx.value("SELECT count(*) FROM txns");

    let again = add(&fx, "final", &["--review"]).ok();

    assert_eq!(again["already"], true);
    assert_eq!(fx.value::<i64>("SELECT count(*) FROM txns"), before, "a rerun writes nothing");
    add(&fx, "fix", &[]).ok();
    refused(&add(&fx, "final", &["--review"]), "job_key_exists", 3);
    // The same key without --review is another Job.
    refused(&add(&fx, "final", &[]), "job_key_exists", 3);
}

#[test]
fn a_review_job_takes_no_needs_and_refuses_a_failed_job_of_its_pr() {
    let fx = fx();
    let run = add(&fx, "final", &["--review", "--needs", "api"]);
    assert_eq!(run.exit, 2, "{run:?}");

    insert_job(&fx, "broken", "failed");
    let error = refused(&add(&fx, "final", &["--review"]), "needs_unavailable", 3);
    assert_eq!(error["details"]["jobs"], json!(["broken"]));
    assert_eq!(fx.value::<i64>("SELECT count(*) FROM jobs WHERE key = 'final'"), 0);
}

// ---------- the Worker ----------

#[test]
fn the_worker_is_a_fork_of_the_lead_that_commits_nothing_and_the_merge_leaves_the_lead_branch() {
    let fx = fx();
    let lead_before = fx.lead_sha();
    add(&fx, "final", &["--review"]).ok();

    let mut submitted = None;
    let (id, _) = fx.drive_parallel("final", |live, id, nap| {
        if nap == 1 {
            submitted = Some(reviewer(live, id));
        }
    });
    let attempt = fx.attempt(&id);
    assert_eq!(submitted.expect("the driver napped: the Worker ran")[2]["ok"], true);

    // The merge: `merged` with no change to the Lead Branch, `merged_sha` its tip.
    assert_eq!((&attempt["outcome"], &attempt["cause"]), (&json!("merged"), &json!("merged")), "{}", log(&fx, &id));
    assert_eq!(fx.lead_sha(), lead_before);
    assert_eq!(attempt["merged_sha"], lead_before);
    assert_eq!(attempt["check_tail"], Value::Null);
    let job = show(&fx, "final");
    assert_eq!(job["status"], "merged");
    assert_eq!(job["merged_sha"], lead_before);
    assert!(fx.fx.git(&fx.lead, &["status", "--porcelain"]).is_empty());
    // A9: the worktree and the branch are gone.
    assert!(!fx.worktree("final-a1").exists());
    assert_eq!(fx.fx.git(&fx.repo, &["branch", "--list", "cpd/pay--final-a1"]), "");

    // Its argv: a Worker's, plus `--fork <the Lead's session file>`.
    let calls = pi_calls(&fx);
    assert_eq!(calls.len(), 1, "{calls:?}");
    let argv = &calls[0];
    let at = |flag: &str| argv[argv.iter().position(|a| a == flag).unwrap() + 1].clone();
    assert_eq!(&argv[..4], ["--approve", "--no-skills", "--exclude-tools", cpd_core::host::pi::WORKER_EXCLUDED_TOOLS]);
    assert_eq!(at("--fork"), lead_file(&fx).to_str().unwrap());
    assert_eq!(at("--model"), "p/sonnet:medium");
    assert_eq!(at("-n"), "pay final a1");

    // Its Session: a Worker's, forked from the Lead.
    let session = attempt["worker_session"].as_str().unwrap().to_owned();
    assert_eq!(at("--session-id"), session);
    let row: (String, Option<String>, Option<String>) = store(&fx.fx.env)
        .read(|c| {
            Ok(c.query_row("SELECT role, attempt, forked_from FROM sessions WHERE id = ?1", [&session], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?)
        })
        .unwrap();
    assert_eq!(row, ("worker".to_owned(), Some("final-a1".to_owned()), Some("lead1".to_owned())));
    let events = fx.fx.cpd(&["events", "list", "--kind", "session-registered", "--plan", PLAN]).ok()["events"].as_array().unwrap().clone();
    let registered = events.iter().find(|e| e["session"] == session.as_str()).expect("session-registered");
    assert_eq!(registered["data"]["forked_from"], "lead1");

    // Its Spec tells it to review, where to write and to commit nothing.
    let spec = fs::read_to_string(fx.fx.env.cpd_home().join("run/pay/specs/final-a1.md")).unwrap();
    let findings = fx.fx.env.cpd_home().join("run/pay/reviews/final.md");
    assert!(spec.starts_with("# Worker Spec: final, Attempt 1\n"), "{spec}");
    assert!(spec.contains("**review Job**") && spec.contains("commit nothing"), "{spec}");
    assert!(spec.contains("git diff main...cpd/pay/repo"), "{spec}");
    assert!(spec.contains(findings.to_str().unwrap()), "{spec}");
    assert!(spec.contains("`review_commits`") && spec.contains("`review_missing`"), "{spec}");
    assert!(!spec.contains("no_commits") && !spec.contains("quick check"), "{spec}");
    assert!(spec.contains("### Goal\n\nReview it."), "{spec}");
    assert_eq!(fs::read_to_string(&findings).unwrap(), "- one finding\n");
}

#[test]
fn submit_without_commits_is_a_review_jobs_privilege_and_it_needs_its_findings_and_no_commit() {
    let fx = fx();
    add(&fx, "final", &["--review"]).ok();

    let mut submitted = None;
    fx.drive("final", |live, id, nap| {
        if nap == 1 {
            submitted = Some(reviewer(live, id));
        }
    });
    let [first, second, third] = submitted.expect("the driver napped: the Worker ran");

    assert_eq!(first["error"]["code"], "review_missing", "{first}");
    assert_eq!(first["error"]["details"]["path"], fx.fx.env.cpd_home().join("run/pay/reviews/final.md").to_str().unwrap());
    assert_eq!(second["error"]["code"], "review_commits", "{second}");
    assert_eq!(third["ok"], true, "{third}");
}

#[test]
fn a_normal_job_without_commits_is_still_refused() {
    let fx = fx();
    add(&fx, "impl", &[]).ok();

    let mut submitted = None;
    fx.drive("impl", |live, id, nap| {
        if nap == 1 {
            let first = submit(live, id);
            live.commit(&live.worktree(id), "impl.txt", "x\n", "x");
            submitted = Some((first, submit(live, id)));
        }
    });
    let (first, second) = submitted.expect("the driver napped: the Worker ran");

    assert_eq!(first["error"]["code"], "no_commits", "{first}");
    assert_eq!(second["ok"], true);
    assert_ne!(fx.lead_sha(), fx.fx.git(&fx.repo, &["rev-parse", "main"]));
}

#[test]
fn the_worker_starts_only_once_the_lead_is_idle() {
    let fx = fx();
    fixture(&fx.fx.env, "UPDATE sessions SET state = 'working' WHERE id = 'lead1';");
    add(&fx, "final", &["--review"]).ok();

    // The driver forks nothing while the Lead works, over five polls of the Lead (each a nap of `LEAD_POLL` on the
    // `ManualClock`); the Lead is idle after the fifth, and the Worker acts at the driver's first look at it.
    let mut submitted = None;
    let (id, _) = fx.drive("final", |live, id, nap| {
        if nap <= 5 {
            // Still waiting: `starting`, no Worker, no Session, nothing forked.
            assert_eq!(live.attempt(id)["status"], "starting");
            assert!(pi_calls(live).is_empty());
            assert_eq!(live.value::<i64>("SELECT count(*) FROM sessions WHERE role = 'worker'"), 0);
        }
        if nap == 5 {
            fixture(&live.fx.env, "UPDATE sessions SET state = 'idle' WHERE id = 'lead1';");
        } else if nap == 6 {
            submitted = Some(reviewer(live, id));
        }
    });
    assert_eq!(&fx.fx.fakes.clock.naps()[..5], [LEAD_POLL; 5]);

    let attempt = fx.attempt(&id);
    assert_eq!(submitted.expect("the driver napped: the Worker ran")[2]["ok"], true);
    assert_eq!(attempt["outcome"], "merged", "{}", log(&fx, &id));
    assert_eq!(pi_calls(&fx).len(), 1);
}

#[test]
fn a_cancel_while_the_driver_waits_for_the_lead_ends_the_attempt_cancelled() {
    let fx = fx();
    fixture(&fx.fx.env, "UPDATE sessions SET state = 'working' WHERE id = 'lead1';");
    add(&fx, "final", &["--review"]).ok();

    let (id, _) = fx.drive("final", |live, _, nap| {
        if nap == 1 {
            live.fx.cpd(&["job", "cancel", "final", "--reason", "not needed", "--plan", PLAN]).ok();
        }
    });
    let attempt = fx.attempt(&id);

    assert_eq!(attempt["outcome"], "cancelled");
    assert!(pi_calls(&fx).is_empty());
}
