//! `pr open`, `pr poll` and `pr rebase` (commands.md §9, contract §7.6). All run in-process on `run_with` over the fake ports
//! (the git route, a `gh` handler, a `sh` handler for the Gate's hooks: [`Poll`]), against a Plan written straight into the
//! database; `pr open` only queues the PR's opener, whose merge (`cpd_core::merge::merge`, in this process) runs the Gate,
//! pushes and opens the pull request. `pr open` and `pr rebase` take no lock (hs-d243).

use crate::common;

use std::collections::VecDeque;
use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::fx::Fx as Kit;
use common::{Run, TestEnv, empty_comments};
use cpd_core::config::{Catalog, Config, TierModel};
use cpd_core::git::{Setup, create_attempt_worktree, create_lead_worktree};
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::ids::{AttemptId, PlanId};
use cpd_core::merge::{self, Ctx, End};
use cpd_core::store::Store;
use cpd_tui::read::{self, Checks, PrState, ReviewDecision};
use nix::fcntl::{Flock, FlockArg};
use rusqlite::params;
use serde_json::{Value, json};

const URL: &str = "https://github.com/acme/pay/pull/812";

const WEB_URL: &str = "https://github.com/acme/web/pull/813";

const TITLE: &str = "Add the payment webhook";

/// The stored title of PR `repo`, and of PR `web` (hs-d285): neither is the Plan's title, `Pay the bills`.
const REPO_TITLE: &str = "Charge the card at checkout";
const WEB_PR_TITLE: &str = "Show the receipt";

/// The stored description of PR `repo` and of PR `web` (pro-d93..pro-d96): the pull request body `pr open` gives to GitHub, verbatim.
pub(super) const DESCRIPTION: &str = "## What and why\n\nCharge the card at checkout.\n\n## Acceptance criteria\n\n- [ ] the card is charged\n";

const EMPTY_SEARCH: &str = r#"{"data":{"search":{"issueCount":0,"nodes":[]},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}"#;

pub(super) fn plan() -> PlanId {
    PlanId::parse("pay").unwrap()
}

/// The rows of Plan `pay` in `phase`, written straight into `store`: PR `repo` on Lead Branch `branch` (its Lead worktree
/// `lead`, in `repo`) with its one Job `api`, `job` (`merged` at `merged_sha`, or `cancelled`), and what `pr open` writes of the
/// pull request in the phases after it.
fn seed_pay(
    mut store: Store,
    phase: &str,
    job: &str,
    branch: &str,
    repo: &Path,
    lead: &Path,
    merged_sha: &str,
) {
    let lead = format!("'{}'", lead.display());
    let (locked, status, columns, values) = match phase {
        "plan" => ("NULL", "waiting", "", String::new()),
        "dispatch" => ("'t'", "dispatch", "", String::new()),
        "deploy" => (
            "'t'",
            "open",
            ", pr_number, pr_url, pr_opened_at, pull_request",
            format!(", 812, '{URL}', 'opened', (SELECT id FROM pull_requests)"),
        ),
        "done" => (
            "'t'",
            "merged",
            ", pr_number, pr_url, pr_opened_at, pull_request, merged_at",
            format!(", 812, '{URL}', 'opened', (SELECT id FROM pull_requests), 'finished'"),
        ),
        other => panic!("no phase {other}"),
    };
    let (worktree, done) = match phase {
        "plan" => ("NULL", "NULL"),
        "done" => (&lead[..], "'finished'"),
        _ => (&lead[..], "NULL"),
    };
    // What `pr open` writes: the pull request's row, with the Plan's title, no review, no checks.
    let pull_request = match phase {
        "deploy" => format!("INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                         VALUES ('acme/pay', 812, '{URL}', 'Pay the bills', 0, 'open', 'none', 'none', 'opened', NULL, 'opened');"),
        "done" => format!("INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                           VALUES ('acme/pay', 812, '{URL}', 'Pay the bills', 0, 'merged', 'none', 'none', 'opened', 'finished', 'finished');"),
        _ => String::new(),
    };
    let sql = format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay', 'ctx', 'Pay the bills', '{phase}', 'lead1', 't', {locked}, {done});
         {pull_request}
         INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, lead_worktree, title, description, shape, created_txn, created_at, started_at{columns})
           VALUES ('pay', 'repo', '{repo}', 'pay', 'main', '{status}', '{branch}', {worktree}, '{REPO_TITLE}', '{DESCRIPTION}', 'config', cpd_txn(), 't', {started}{values});",
        started = if locked == "NULL" { "NULL" } else { "'2026-10-27T09:00:00.000Z'" },
        repo = repo.display(),
        branch = branch,
    );
    store
        .write("fixture", |tx| {
            tx.execute_batch(&sql)?;
            tx.execute(
                "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, merged_sha, cancel_reason, cancel_requested_at, created_txn, created_at)
                 VALUES ('pay', 'repo', 'api', 'The | API', 'S2', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', ?4, 0, ?1,
                         iif(?4 = 'merged', ?2, NULL), iif(?4 = 'cancelled', 'not needed', NULL), iif(?4 = 'cancelled', 't', NULL), ?3, 't')",
                params!["f".repeat(64), merged_sha, tx.id(), job],
            )?;
            Ok(())
        })
        .unwrap();
}

/// `(phase, pr_number, pr_url, pr_opened_at, pull_request, done_at)` of the Plan.
type PlanRow = (
    String,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
);

/// What [`Poll`] reads of the database: the Plan `pay` and its PR.
pub(super) trait Rows {
    fn home(&self) -> &CpdHome;

    fn store(&self) -> Store {
        Store::open(self.home(), 5000).unwrap()
    }

    fn plan_row(&self) -> PlanRow {
        self.store()
            .read(|c| {
                Ok(c.query_row(
                    "SELECT p.phase, r.pr_number, r.pr_url, r.pr_opened_at, r.pull_request, p.done_at
                       FROM plans p JOIN workflows r ON r.plan = p.id WHERE p.id = 'pay'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
                )?)
            })
            .unwrap()
    }

    /// The row of `pull_requests` the Plan's PR points to, as `(title, review_decision, checks, state)`.
    fn pr_data(&self) -> (String, String, String, String) {
        self.store()
            .read(|c| {
                Ok(c.query_row(
                    "SELECT q.title, q.review_decision, q.checks, q.state
                       FROM workflows r JOIN pull_requests q ON q.id = r.pull_request WHERE r.plan = 'pay'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )?)
            })
            .unwrap()
    }

    /// How many write transactions have run: the rows of `txns`.
    fn txns(&self) -> i64 {
        self.store()
            .read(|c| Ok(c.query_row("SELECT count(*) FROM txns", [], |r| r.get(0))?))
            .unwrap()
    }

    /// `(text, data)` of the Events of Plan `pay` of `kind`, in order.
    fn events(&self, kind: &str) -> Vec<(String, Value)> {
        self.plan_events("pay", kind)
    }

    /// `(text, data)` of the Events of Plan `plan` of `kind`, in order.
    fn plan_events(&self, plan: &str, kind: &str) -> Vec<(String, Value)> {
        self.store()
            .read(|c| {
                let mut stmt = c.prepare(
                    "SELECT text, data FROM events WHERE plan = ?1 AND kind = ?2 ORDER BY id",
                )?;
                let rows = stmt
                    .query_map([plan, kind], |r| {
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

    /// How many Events there are, of any kind.
    fn event_count(&self) -> i64 {
        self.store()
            .read(|c| Ok(c.query_row("SELECT count(*) FROM events", [], |r| r.get(0))?))
            .unwrap()
    }

    /// `(repo, number)` of every row of `pull_requests`, in order.
    fn pull_requests(&self) -> Vec<(String, i64)> {
        self.store()
            .read(|c| {
                let mut stmt = c.prepare("SELECT repo, number FROM pull_requests ORDER BY id")?;
                Ok(stmt
                    .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<rusqlite::Result<_>>()?)
            })
            .unwrap()
    }

    fn phase(&self) -> String {
        self.plan_row().0
    }

    /// The `pre-merge` hook's output: the log without the Gate's own `gate: ...` lines at its head (the slot's, §7.6).
    fn gate_log(&self) -> String {
        let log = self.pr_log("pre-merge");
        let mut rest = log.as_str();
        while let Some(line) = rest.lines().next().filter(|l| l.starts_with("gate: ")) {
            rest = &rest[line.len() + 1..];
        }
        rest.to_owned()
    }

    /// `run/pay/logs/pr-repo.<hook>.log`.
    fn pr_log(&self, hook: &str) -> String {
        fs::read_to_string(
            self.home()
                .plan_dir(&plan())
                .join(format!("logs/pr-repo.{hook}.log")),
        )
        .unwrap_or_default()
    }

    fn workflow_status(&self, key: &str) -> String {
        self.store()
            .read(|c| {
                Ok(c.query_row(
                    "SELECT status FROM workflows WHERE plan = 'pay' AND key = ?1",
                    [key],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    }
}

/// What the Plan's second PR is for the two-PR tests.
enum Web {
    /// Opened as pull request 813.
    Open,
    /// Not started: it needs `repo`, and its Job is `queued`.
    Waiting,
}

/// The rows of PR `web` of Plan `pay` (repo `repo`, Lead worktree `lead`, its Job `ui` merged at `sha`) as `state` says.
fn seed_web(mut store: Store, repo: &Path, lead: &Path, sha: &str, state: Web) {
    let (status, worktree, started, open_columns, open_values, job, needs) = match state {
        Web::Open => (
            "open",
            format!("'{}'", lead.display()),
            "'t'",
            ", pr_number, pr_url, pr_opened_at, pull_request",
            format!(
                ", 813, '{WEB_URL}', 'opened', (SELECT id FROM pull_requests WHERE number = 813)"
            ),
            "merged",
            "",
        ),
        Web::Waiting => (
            "waiting",
            "NULL".to_owned(),
            "NULL",
            "",
            String::new(),
            "queued",
            "INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('pay', 'web', 'repo');",
        ),
    };
    store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "{row}
                 INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, lead_worktree, title, description, shape, created_txn, created_at, started_at{open_columns})
                   VALUES ('pay', 'web', '{repo}', 'web', 'main', '{status}', 'cpd/pay/web', {worktree}, '{WEB_PR_TITLE}', '{DESCRIPTION}', 'config', cpd_txn(), 't', {started}{open_values});
                 {needs}",
                repo = repo.display(),
                row = if matches!(state, Web::Open) {
                    format!(
                        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                         VALUES ('acme/web', 813, '{WEB_URL}', 'Pay the bills (web)', 0, 'open', 'none', 'none', 'opened', NULL, 'opened');"
                    )
                } else {
                    String::new()
                },
            ))?;
            tx.execute(
                "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, merged_sha, created_txn, created_at)
                 VALUES ('pay', 'web', 'ui', 'The UI', 'S2', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', ?3, 0, ?1,
                         iif(?3 = 'merged', ?2, NULL), ?4, 't')",
                params!["f".repeat(64), sha, job, tx.id()],
            )?;
            Ok(())
        })
        .unwrap();
}

pub(super) fn code(run: &Run) -> (String, i32) {
    (run.error()["code"].as_str().unwrap().to_owned(), run.exit)
}

// ------------------------------------------------------------------ pr open

#[test]
fn open_on_a_done_plan_is_already() {
    let fx = poll_fx("done");
    let out = fx.pr("open").ok();
    assert_eq!(out["phase"], "done");
    assert_eq!(out["already"], true);
    assert_eq!(out["pr"]["pr"]["number"], 812);
}

#[test]
fn open_refuses_a_phase_before_dispatch() {
    let fx = poll_fx("plan");
    let run = fx.pr("open");
    assert_eq!(code(&run), ("illegal_phase".into(), 3));
    assert_eq!(run.error()["details"]["phase"], "plan");
}

// ------------------------------------------------ `pr open` queues the opener (contract §7.6)

/// `(kind, status, tier, appended, fingerprint IS NOT NULL, title)` of Job `key` of Plan `pay`.
fn job_row(fx: &Poll, key: &str) -> (String, String, String, i64, bool, String) {
    fx.store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT kind, status, tier, appended, fingerprint IS NOT NULL, title FROM jobs WHERE plan = 'pay' AND key = ?1",
                [key],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
            )?)
        })
        .unwrap()
}

/// `(goal, context, touches, acceptance, out_of_scope)` of Job `key` of Plan `pay`.
fn job_spec(fx: &Poll, key: &str) -> (String, String, Value, Value, Value) {
    fx.store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT goal, context, touches, acceptance, out_of_scope FROM jobs WHERE plan = 'pay' AND key = ?1",
                [key],
                |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        serde_json::from_str(&r.get::<_, String>(2)?).unwrap(),
                        serde_json::from_str(&r.get::<_, String>(3)?).unwrap(),
                        serde_json::from_str(&r.get::<_, String>(4)?).unwrap(),
                    ))
                },
            )?)
        })
        .unwrap()
}

/// `(key, status)` of every `open` Job of Plan `pay`, by key.
pub(super) fn open_jobs(fx: &Poll) -> Vec<(String, String)> {
    fx.store()
        .read(|c| {
            let mut stmt = c.prepare(
                "SELECT key, status FROM jobs WHERE plan = 'pay' AND kind = 'open' ORDER BY key",
            )?;
            Ok(stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

/// Sets the top-level `worker_tier` of the machine config.
fn set_worker_tier(fx: &Poll, tier: &str) {
    let config = fs::read_to_string(fx.home.config()).unwrap();
    fs::write(
        fx.home.config(),
        config.replacen(
            "machine = \"t\"\n",
            &format!("machine = \"t\"\nworker_tier = \"{tier}\"\n"),
            1,
        ),
    )
    .unwrap();
}

/// cpd pr open queues the PR's opener instead of opening the pull request (contract §7.6, commands.md §9.1): one `open` Job of the
/// PR at `worker_tier` with the fixed Spec and no Dependencies, its key printed as `job` and `status` `queued`.
#[test]
fn pr_open_queues_a_fresh_open_job_at_worker_tier() {
    let fx = poll_fx("dispatch");
    set_worker_tier(&fx, "S3");
    let txns = fx.txns();

    let out = fx.pr("open").ok();

    assert_eq!(
        (&out["plan"], &out["pr"], &out["job"], &out["status"]),
        (
            &json!("pay"),
            &json!("repo"),
            &json!("open-repo"),
            &json!("queued")
        ),
        "{out}"
    );
    assert_eq!(
        open_jobs(&fx),
        [("open-repo".to_owned(), "queued".to_owned())]
    );
    assert_eq!(
        job_row(&fx, "open-repo"),
        (
            "open".to_owned(),
            "queued".to_owned(),
            "S3".to_owned(),
            1,
            true,
            format!("Open the pull request: {REPO_TITLE}")
        )
    );
    let (goal, context, touches, acceptance, out_of_scope) = job_spec(&fx, "open-repo");
    assert!(
        goal.contains(&format!(
            "Make the Lead Branch {branch} of Workflow repo fit its base main",
            branch = fx.branch
        )),
        "{goal}"
    );
    assert!(
        context.contains("You push nothing, open nothing and run no Gate"),
        "{context}"
    );
    for (what, value) in [
        ("touches", &touches),
        ("acceptance", &acceptance),
        ("out_of_scope", &out_of_scope),
    ] {
        assert!(
            value.as_array().is_some_and(|a| !a.is_empty()),
            "{what}: {value}"
        );
    }
    let dependencies: i64 = fx
        .store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM dependencies WHERE job = 'open-repo'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(dependencies, 0, "the opener has no Dependency rows");
    assert_eq!(fx.txns(), txns + 1, "one transaction");
    assert_eq!(
        fx.plan_row().0,
        "dispatch",
        "the Plan stays in dispatch: the opener opens it"
    );
}

/// The opener is added when the PR has no `open` Job at all, whatever other kinds it has (contract §7.6, §3.7 E4).
#[test]
fn pr_open_adds_the_opener_when_the_pr_has_none() {
    let fx = poll_fx("dispatch");
    // An `update` Job is not an `open` Job: the PR still has no opener.
    fx.store()
        .write("fixture", |tx| {
            tx.execute(
                "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, kind, fingerprint, created_txn, created_at)
                 VALUES ('pay', 'repo', 'up-repo', 'Bring the pull request up to date', 'S1', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', 'queued', 1, 'update', ?1, ?2, 't')",
                params!["f".repeat(64), tx.id()],
            )?;
            Ok(())
        })
        .unwrap();
    assert!(open_jobs(&fx).is_empty());

    let out = fx.pr("open").ok();

    assert_eq!(
        (&out["job"], &out["status"]),
        (&json!("open-repo"), &json!("queued")),
        "{out}"
    );
    assert_eq!(
        open_jobs(&fx),
        [("open-repo".to_owned(), "queued".to_owned())]
    );
}

/// A `failed` opener is not live (only `draft`, `queued`, `running` are): cpd adds a fresh one at the next free key and leaves the failed Job
/// as it is (contract §7.6, §3.7 E4).
#[test]
fn pr_open_adds_a_fresh_opener_after_a_failed_one() {
    let fx = poll_fx("dispatch");
    fx.open_job("failed");

    let out = fx.pr("open").ok();

    assert_eq!(
        (&out["job"], &out["status"]),
        (&json!("open-repo-2"), &json!("queued")),
        "{out}"
    );
    assert_eq!(
        open_jobs(&fx),
        [
            ("open-repo".to_owned(), "failed".to_owned()),
            ("open-repo-2".to_owned(), "queued".to_owned())
        ]
    );
    let (status, cause) = fx
        .store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT status, failed_cause FROM jobs WHERE plan = 'pay' AND key = 'open-repo'",
                [],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
            )?)
        })
        .unwrap();
    assert_eq!(
        (status.as_str(), cause.as_deref()),
        ("failed", Some("crashed")),
        "the failed opener is left as it is"
    );
}

/// A live opener (`draft`, `queued` or `running`) refuses the command `open_job_live` (exit 3, `details.job`) with nothing written: two openers
/// would race (contract §7.6, hs-d573).
#[test]
fn pr_open_is_refused_open_job_live_while_the_opener_is_live() {
    for status in ["draft", "queued", "running"] {
        let fx = poll_fx("dispatch");
        fx.open_job(status);
        let txns = fx.txns();

        let run = fx.pr("open");

        assert_eq!(code(&run), ("open_job_live".into(), 3), "{status}: {run:?}");
        assert_eq!(run.error()["details"]["job"], "open-repo", "{status}");
        assert_eq!(
            open_jobs(&fx),
            [("open-repo".to_owned(), status.to_owned())],
            "{status}: nothing was added"
        );
        assert_eq!(
            (fx.plan_row().0, fx.txns()),
            ("dispatch".to_owned(), txns),
            "{status}: nothing was written"
        );
    }
}

/// The kept preconditions refuse before anything runs (contract §7.6): an unmerged Job is `jobs_not_done`, a NULL title is `workflow_title_required`.
#[test]
fn pr_open_refuses_jobs_not_done_and_a_missing_title() {
    // One in-process `Fx` at a time (its lock is process-wide): each block drops its own before the next.
    {
        let fx = poll_fx("dispatch");
        fx.store()
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, fingerprint, created_txn, created_at)
                     VALUES ('pay', 'repo', 'ui', 'UI', 'S2', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', 'queued', 0, ?1, ?2, 't')",
                    params!["f".repeat(64), tx.id()],
                )?;
                Ok(())
            })
            .unwrap();
        let txns = fx.txns();

        let run = fx.pr("open");

        assert_eq!(code(&run), ("jobs_not_done".into(), 3));
        assert_eq!(
            run.error()["details"]["jobs"],
            json!([{"key": "ui", "status": "queued"}])
        );
        assert_eq!(
            (fx.plan_row().0, fx.txns(), open_jobs(&fx)),
            ("dispatch".to_owned(), txns, Vec::<(String, String)>::new())
        );
        assert!(fx.gh_calls().is_empty(), "no gh call");
    }

    let fx = poll_fx("dispatch");
    fx.store()
        .write("fixture", |tx| {
            Ok(tx.execute("UPDATE workflows SET title = NULL", [])?)
        })
        .unwrap();
    let txns = fx.txns();

    let run = fx.pr("open");

    assert_eq!(code(&run), ("workflow_title_required".into(), 3));
    assert_eq!(
        run.error()["details"],
        json!({"plan": "pay", "workflow": "repo"})
    );
    assert_eq!(
        (fx.plan_row().0, fx.txns(), open_jobs(&fx)),
        ("dispatch".to_owned(), txns, Vec::<(String, String)>::new())
    );
    assert!(fx.gh_calls().is_empty(), "no gh call");
}

/// `pr open` queues the opener only: it runs no Gate, calls no `gh`, pushes nothing and opens no pull request (contract §7.6).
#[test]
fn pr_open_does_not_gate_push_or_call_gh() {
    let fx = poll_fx("dispatch");
    let marker = fx.env.root().join("hook-ran");
    fx.hooks(&format!("pre-merge = \"touch {}\"\n", marker.display()));
    // The hook, were it run, leaves the marker.
    let touched = marker.clone();
    fx.sh(move |_| {
        fs::write(&touched, "").unwrap();
        Reply::ok("")
    });
    let before = fx.remote_branch("cpd/pay");
    assert_eq!(before, Some(fx.lock_pushed.clone()));

    let out = fx.pr("open").ok();

    assert_eq!(out["status"], "queued", "{out}");
    assert!(
        !marker.exists() && fx.commands().is_empty(),
        "the Gate must not run"
    );
    assert_eq!(fx.gate_log(), "", "no Gate log");
    assert!(fx.gh_calls().is_empty(), "no gh call: {:?}", fx.gh_calls());
    assert_eq!(fx.remote_branch("cpd/pay"), before, "nothing was pushed");
    assert!(
        fx.events("pr-opened").is_empty(),
        "no pull request was opened"
    );
    assert_eq!(fx.plan_row().0, "dispatch", "the Plan stays in dispatch");
}

/// A fixture of `phase` under policy `pr.gate = "ci"` (cpd's default), whose failing `pre-merge` leaves a marker if it runs.
fn ci_fx_with_failing_hook(phase: &str) -> (Poll, PathBuf) {
    let fx = rebase_fx(phase);
    fx.env.owner_policy("acme", "[pr]\ngate = \"ci\"\n");
    let marker = fx.env.root().join("hook-ran");
    fx.hooks(&format!(
        "pre-merge = \"touch {}; exit 3\"\n",
        marker.display()
    ));
    let touched = marker.clone();
    fx.sh(move |_| {
        fs::write(&touched, "").unwrap();
        Reply::exit(3, "", "")
    });
    (fx, marker)
}

#[test]
fn rebase_with_gate_ci_runs_no_gate_and_pushes() {
    let (fx, marker) = ci_fx_with_failing_hook("deploy");
    fx.git(&fx.lead, &["push", "-q", "origin", "cpd/pay"]);
    fx.advance_base("base.txt", "base\n");

    let out = fx.pr("rebase").ok();

    assert_eq!(out["rebased"], true);
    assert!(!marker.exists(), "the pre-merge hook ran under pr.gate ci");
    assert_eq!(
        fx.remote_branch("cpd/pay"),
        Some(fx.lead_sha()),
        "the rebased branch was pushed"
    );
}

// ------------------------------------------------------------------ the opener's merge (restored, in-process)
//
// `pr open` queues the PR's `open` Job; its Attempt's merge (contract §7.4/§7.7, `cpd_core::merge::merge`, run here by
// `Poll::merge`) is what runs the Gate, pushes the Lead Branch and opens or adopts the pull request. Git, `gh` and the
// Gate's `sh` are fakes: the Gate's hooks answer by [`Poll::sh`].

/// `gh pr list` answering one pull request of the Lead Branch in `state`.
fn list_of(state: &str) -> Reply {
    list_created(812, state, "2026-10-28T10:00:00Z")
}

/// `gh pr list` answering one pull request `number` of the Lead Branch in `state`, created at `created` (the PR row started 2026-10-27).
fn list_created(number: i64, state: &str, created: &str) -> Reply {
    let url = format!("https://github.com/acme/pay/pull/{number}");
    Reply::ok(&format!(
        "[{{\"number\": {number}, \"url\": \"{url}\", \"state\": \"{state}\", \"createdAt\": \"{created}\"}}]\n"
    ))
}

/// A fixture of phase `dispatch` whose `pre-merge` hook leaves a marker file when it runs; and that marker.
fn poll_fx_with_marked_hook() -> (Poll, PathBuf) {
    let fx = poll_fx("dispatch");
    let marker = fx.env.root().join("hook-ran");
    fx.hooks(&format!("pre-merge = \"touch {}\"\n", marker.display()));
    let touched = marker.clone();
    fx.sh(move |_| {
        fs::write(&touched, "").unwrap();
        Reply::ok("")
    });
    (fx, marker)
}

/// `git <args>` in `dir`, through the fake ports of a test: for a hook that does what a shell command would.
fn git_in_hook(ports: &cpd_core::ports::Ports, dir: &Path, args: &[&str]) {
    git_out_in_hook(ports, dir, args);
}

/// [`git_in_hook`], and what git printed, without its last newline.
fn git_out_in_hook(ports: &cpd_core::ports::Ports, dir: &Path, args: &[&str]) -> String {
    let call = Call {
        tool: "git".to_owned(),
        program: "git".to_owned(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        envs: Vec::new(),
        cwd: Some(dir.to_path_buf()),
        deadline: Duration::from_secs(60),
        stdin: None,
        stderr_to: None,
    };
    let out = ports.runner.run(&call).unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .expect("git prints UTF-8")
        .trim_end_matches('\n')
        .to_owned()
}

#[test]
fn open_gates_pushes_creates_the_pr_and_records_it() {
    let fx = poll_fx("dispatch");
    fx.hooks("pre-merge = \"echo gate ran on {{ branch }} in $(basename $PWD) at $(git rev-parse HEAD)\"\n");
    let (attempt, worktree, tip) = fx.submitted_opener();
    let at = tip.clone();
    fx.sh(move |call| {
        Reply::ok(&format!(
            "gate ran on {OPENER_BRANCH} in {} at {at}\n",
            basename(call)
        ))
    });

    let end = fx.merge(&attempt);

    assert_eq!(end, End::Merged { sha: tip.clone() });
    // The Gate ran once, in the Attempt's worktree at the tip (`branch` is the Attempt's branch), and wrote its log.
    assert_eq!(
        fx.commands(),
        [format!(
            "echo gate ran on {OPENER_BRANCH} in $(basename $PWD) at $(git rev-parse HEAD)"
        )]
    );
    assert_eq!(fx.calls("sh")[0].cwd.as_deref(), Some(worktree.as_path()));
    assert_eq!(
        fx.open_gate_log(),
        format!(
            "gate ran on {OPENER_BRANCH} in {} at {tip}\n",
            worktree.file_name().unwrap().to_string_lossy()
        )
    );
    // Pushed, by rule P: no upstream written to the repo's config (G1).
    assert_eq!(fx.remote_branch("cpd/pay"), Some(tip.clone()));
    let config = fx.git_output(&fx.repo, &["config", "--get-regexp", "^branch\\."]);
    assert!(
        config.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&config.stdout)
    );
    // gh: look for an existing PR (twice: before the Gate's push, and to open it), then create it with the stored description as its body.
    let calls = fx.gh_calls();
    let list = [
        "pr",
        "list",
        "--head",
        "cpd/pay",
        "--state",
        "all",
        "--json",
        "number,url,state,createdAt",
    ];
    assert_eq!(calls[0], list);
    assert_eq!(calls[1], list);
    assert_eq!(
        calls[2][..8],
        [
            "pr", "create", "--base", "main", "--head", "cpd/pay", "--title", REPO_TITLE
        ]
    );
    assert_eq!(calls[2][8..], ["--body", DESCRIPTION]);
    // Rows and the Event, in one transaction.
    let row = fx.plan_row();
    assert_eq!(
        (row.0.as_str(), row.1, row.2.as_deref()),
        ("deploy", Some(812), Some(URL))
    );
    assert!(row.3.is_some(), "the PR has its opening time");
    // The pull request's row (D6), from the URL `gh pr create` printed, and the PR points to it.
    assert!(row.4.is_some(), "prs.pull_request is set");
    assert_eq!(
        fx.shown_pull_request(),
        json!({"repo": "acme/pay", "number": 812, "url": URL, "title": REPO_TITLE, "draft": false, "state": "open",
               "review_decision": "none", "checks": "none", "mergeable": "unknown", "merge_state": "unknown",
               "created_at": "opened", "merged_at": null})
    );
    let opened = fx.events("pr-opened");
    assert_eq!(opened.len(), 1);
    assert_eq!(
        opened[0].1,
        json!({"workflow": "repo", "number": 812, "url": URL})
    );
    let events = fx.events("plan-phase");
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].1,
        json!({"from": "dispatch", "to": "deploy", "workflows": [{"key": "repo", "number": 812, "url": URL}]})
    );
    assert!(
        events[0].0.contains("cpd pr poll --plan pay"),
        "{}",
        events[0].0
    );
}

#[test]
fn open_fails_loud_when_gh_cannot_find_the_pull_request_it_created() {
    let fx = poll_fx("dispatch");
    fx.lookup(&[None]);
    let (attempt, _, _) = fx.submitted_opener();

    let end = fx.merge(&attempt);

    let End::Failed { detail, .. } = end else {
        panic!("{end:?}")
    };
    assert!(detail.contains("gh_failed"), "{detail}");
    assert_eq!(fx.plan_row().0, "dispatch");
}

/// The pull request #265 of the Lead Branch, created 2026-10-22, before the PR row started (2026-10-27): not the row's.
fn old_pull_request(state: &str) -> Reply {
    list_created(265, state, "2026-10-22T08:00:00Z")
}

#[test]
fn open_ignores_a_merged_pull_request_created_before_the_pr_started() {
    let fx = poll_fx("dispatch");
    fx.gh.list.set(old_pull_request("MERGED"));
    let (attempt, _, tip) = fx.submitted_opener();

    let end = fx.merge(&attempt);

    assert_eq!(end, End::Merged { sha: tip });
    assert_eq!(
        fx.gh_calls().iter().filter(|c| c[1] == "create").count(),
        1,
        "{:?}",
        fx.gh_calls()
    );
    let row = fx.plan_row();
    assert_eq!((row.0.as_str(), row.1), ("deploy", Some(812)));
    let status: String = fx
        .store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT status FROM workflows WHERE plan = 'pay' AND key = 'repo'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(status, "open");
    assert!(fx.events("pr-merged").is_empty());
}

#[test]
fn open_ignores_a_closed_pull_request_created_before_the_pr_started() {
    let fx = poll_fx("dispatch");
    fx.gh.list.set(old_pull_request("CLOSED"));
    let (attempt, _, tip) = fx.submitted_opener();

    let end = fx.merge(&attempt);

    assert_eq!(end, End::Merged { sha: tip });
    assert_eq!(
        fx.gh_calls().iter().filter(|c| c[1] == "create").count(),
        1,
        "{:?}",
        fx.gh_calls()
    );
    assert_eq!(fx.plan_row().1, Some(812));
    assert!(fx.events("pr-closed").is_empty());
}

#[test]
fn open_does_not_adopt_an_open_pull_request_created_before_the_pr_started() {
    let fx = poll_fx("dispatch");
    fx.gh.list.set(old_pull_request("OPEN"));
    fx.gh.create.set(Reply::exit(
        1,
        "",
        "a pull request for branch cpd/pay already exists\n",
    ));
    let (attempt, _, _) = fx.submitted_opener();

    let end = fx.merge(&attempt);

    let End::Failed { detail, .. } = end else {
        panic!("{end:?}")
    };
    assert!(detail.contains("gh_failed"), "{detail}");
    assert!(
        fx.gh_calls().iter().any(|c| c[1] == "create"),
        "{:?}",
        fx.gh_calls()
    );
    let row = fx.plan_row();
    assert_eq!((row.0.as_str(), row.1), ("dispatch", None));
}

#[test]
fn open_adopts_a_pr_an_earlier_run_created() {
    let fx = poll_fx("dispatch");
    fx.gh.list.set(list_of("OPEN"));
    let (attempt, _, tip) = fx.submitted_opener();

    let end = fx.merge(&attempt);

    assert_eq!(end, End::Merged { sha: tip });
    assert_eq!(fx.plan_row().1, Some(812));
    assert!(
        fx.gh_calls().iter().all(|c| c[1] != "create"),
        "{:?}",
        fx.gh_calls()
    );
    assert_eq!(fx.plan_row().0, "deploy");
}

#[test]
fn open_adopts_a_merged_pull_request_without_the_gate_and_the_plan_is_done() {
    let (fx, marker) = poll_fx_with_marked_hook();
    fx.gh.list.set(list_of("MERGED"));
    fx.lookup(&[Some(pay("MERGED", None, None))]);
    let remote_before = fx.remote_branch("cpd/pay");
    let (attempt, _, tip) = fx.submitted_opener();

    let end = fx.merge(&attempt);

    assert_eq!(end, End::Merged { sha: tip });
    // The opener's merge runs its Gate before it finds the pull request merged (contract §7.7, hs-d340: only steps 5 and 8
    // are skipped), where the `pr open` of this test's old form skipped it; no push, no pull request created.
    assert!(marker.exists(), "the pre-merge hook ran");
    assert_eq!(fx.commands().len(), 1);
    assert_eq!(fx.remote_branch("cpd/pay"), remote_before);
    assert!(
        fx.gh_calls().iter().all(|c| c[1] != "create"),
        "{:?}",
        fx.gh_calls()
    );
    // The PR and the Plan, as `pr poll` leaves them.
    let row = fx.plan_row();
    assert_eq!(
        (row.0.as_str(), row.1, row.2.as_deref()),
        ("done", Some(812), Some(URL))
    );
    assert!(row.4.is_some() && row.5.is_some(), "{row:?}");
    assert_eq!(fx.workflow_status("repo"), "merged");
    assert_eq!(
        fx.pr_data(),
        (
            "Pay the bills".into(),
            "none".into(),
            "none".into(),
            "merged".into()
        )
    );
    assert_eq!(fx.shown_pull_request()["merged_at"], "2026-10-14T10:00:00Z");
    let merged = fx.events("pr-merged");
    assert_eq!(merged.len(), 1);
    assert_eq!(
        merged[0].1,
        json!({"workflow": "repo", "number": 812, "url": URL, "merged_at": "2026-10-14T10:00:00Z"})
    );
    assert!(fx.events("pr-opened").is_empty());
    assert_eq!(fx.events("plan-phase")[0].1["to"], "deploy");
    let done = fx.events("plan-done");
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].1["workflows"][0]["key"], "repo");

    // Again: already, nothing more.
    let txns = fx.txns();
    let again = fx.pr("open").ok();
    assert_eq!(
        (again["already"].as_bool(), again["phase"].as_str()),
        (Some(true), Some("done"))
    );
    assert_eq!((fx.txns(), fx.events("plan-done").len()), (txns, 1));
}

#[test]
fn open_adopting_a_merged_pull_request_supersedes_the_pending_reviews() {
    let (fx, marker) = poll_fx_with_marked_hook();
    fx.gh.list.set(list_of("MERGED"));
    fx.lookup(&[Some(pay("MERGED", None, None))]);
    insert_pending_reviews(&fx.env, "pay");
    let (attempt, _, tip) = fx.submitted_opener();

    let end = fx.merge(&attempt);

    assert_eq!(end, End::Merged { sha: tip });
    assert!(marker.exists(), "the Gate of the opener's merge ran (§7.7)");
    assert_reviews_superseded_before_done(&fx.env, "pay");
}

#[test]
fn open_adopting_a_merged_pull_request_keeps_the_plan_in_dispatch_while_another_pr_is_not_open() {
    let fx = poll_fx("dispatch");
    fx.add_waiting_web();
    fx.gh.list.set(list_of("MERGED"));
    fx.lookup(&[Some(pay("MERGED", None, None))]);
    let (attempt, _, tip) = fx.submitted_opener();

    let end = fx.merge(&attempt);

    assert_eq!(end, End::Merged { sha: tip });
    assert_eq!(
        (fx.workflow_status("repo").as_str(), fx.phase().as_str()),
        ("merged", "dispatch")
    );
    assert_eq!(fx.workflow_status("web"), "waiting");
    assert_eq!(fx.events("pr-merged").len(), 1);
    assert!(fx.events("plan-phase").is_empty() && fx.events("plan-done").is_empty());
}

#[test]
fn open_adopting_a_merged_pull_request_updates_the_row_the_poll_stored_open() {
    let fx = poll_fx("dispatch");
    fx.store()
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                 VALUES ('acme/pay', 812, '{URL}', 'As the poll wrote it', 0, 'open', 'approved', 'passed', 'github time', NULL, 'poll time')"
            ))?;
            Ok(())
        })
        .unwrap();
    fx.gh.list.set(list_of("MERGED"));
    fx.lookup(&[Some(pay("MERGED", Some("APPROVED"), Some("SUCCESS")))]);
    let (attempt, _, _) = fx.submitted_opener();

    fx.merge(&attempt);

    assert_eq!(fx.pr_data().3, "merged");
    assert_eq!(fx.workflow_status("repo"), "merged");
    let rows: i64 = fx
        .store()
        .read(|c| Ok(c.query_row("SELECT count(*) FROM pull_requests", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(rows, 1);
}

#[test]
fn open_adopts_an_open_pull_request_of_a_lead_branch_that_is_behind_its_base() {
    let (fx, marker) = poll_fx_with_marked_hook();
    fx.gh.list.set(list_of("OPEN"));
    fx.advance_base("base.txt", "base\n");
    let (attempt, _, tip) = fx.submitted_opener();

    let end = fx.merge(&attempt);

    assert_eq!(end, End::Merged { sha: tip.clone() });
    assert!(marker.exists(), "the Gate ran");
    assert_eq!(
        (
            fx.plan_row().0.as_str(),
            fx.workflow_status("repo").as_str()
        ),
        ("deploy", "open")
    );
    assert_eq!(fx.remote_branch("cpd/pay"), Some(tip));
}

#[test]
fn open_adopts_the_pull_request_a_concurrent_open_created_when_its_create_fails() {
    let fx = poll_fx("dispatch");
    // `gh pr create` fails because another opener of the PR created it: the lookup now finds it.
    fx.gh.list.then(Reply::ok("[]\n"));
    fx.gh.list.then(list_of("OPEN"));
    fx.gh.create.set(Reply::exit(
        1,
        "",
        "a pull request for branch cpd/pay already exists\n",
    ));
    let (attempt, _, tip) = fx.submitted_opener();

    let end = fx.merge(&attempt);

    assert_eq!(end, End::Merged { sha: tip });
    assert_eq!(fx.plan_row().1, Some(812));
    let calls: Vec<String> = fx.gh_calls().iter().map(|c| c[1].clone()).collect();
    assert_eq!(calls, ["list", "list", "create", "list", "graphql"]);
    assert_eq!(fx.plan_row().0, "deploy");
}

#[test]
fn open_after_another_open_opened_the_pr_during_its_gate_is_already_and_pushes_nothing() {
    let fx = poll_fx("dispatch");
    fx.hooks("pre-merge = \"echo gate\"\n");
    let (attempt, _, tip) = fx.submitted_opener();
    // Another opener commits meanwhile: its rows and the pull request GitHub now lists.
    let (home, gh) = (fx.home.clone(), fx.gh.clone());
    fx.sh(move |_| {
        Store::open(&home, 5000)
            .unwrap()
            .write("pr open", |tx| {
                tx.execute_batch(&format!(
                    "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                       VALUES ('acme/pay', 812, '{URL}', 'Pay the bills', 0, 'open', 'none', 'none', 'opened', NULL, 'opened');
                     UPDATE workflows SET status = 'open', pr_number = 812, pr_url = '{URL}', pr_opened_at = 'opened',
                            pull_request = (SELECT id FROM pull_requests) WHERE plan = 'pay';"
                ))?;
                Ok(())
            })
            .unwrap();
        gh.list.set(list_of("OPEN"));
        Reply::ok("")
    });
    assert_eq!(fx.remote_branch("cpd/pay"), Some(fx.lock_pushed.clone()));

    let end = fx.merge(&attempt);

    // The merge ends all the same, adopting what the other opener recorded: no `gh pr create`, nothing recorded twice.
    assert_eq!(end, End::Merged { sha: tip.clone() });
    assert_eq!(
        fx.plan_row().0,
        "dispatch",
        "the other opener's rows stay as they are: its own poll moves the Plan"
    );
    assert_eq!(fx.workflow_status("repo"), "open");
    assert!(
        fx.events("pr-opened").is_empty() && fx.events("plan-phase").is_empty(),
        "written once, by the other opener"
    );
    assert!(
        fx.gh_calls().iter().all(|c| c[1] != "create"),
        "{:?}",
        fx.gh_calls()
    );
}

#[test]
fn open_again_is_already_and_touches_nothing() {
    let fx = poll_fx("dispatch");
    let (attempt, _, _) = fx.submitted_opener();
    fx.merge(&attempt);
    let first = fx.plan_row();
    let calls = fx.gh_calls().len();
    let txns = fx.txns();

    let again = fx.pr("open").ok();

    assert_eq!(
        (
            again["already"].as_bool(),
            again["phase"].as_str(),
            again["pr"]["status"].as_str()
        ),
        (Some(true), Some("deploy"), Some("open"))
    );
    assert_eq!(again["pr"]["pr"]["number"], 812);
    assert_eq!((fx.gh_calls().len(), fx.txns()), (calls, txns));
    assert_eq!(fx.plan_row(), first);
    assert_eq!(fx.events("plan-phase").len(), 1);
}

#[test]
fn open_by_hand_goes_on_when_the_open_job_failed_was_cancelled_or_merged_and_counts_it_for_nothing()
{
    for status in ["failed", "cancelled", "merged"] {
        let fx = poll_fx("dispatch");
        fx.open_job(status);

        let out = fx.pr("open").ok();

        assert_eq!(
            (out["job"].as_str(), out["status"].as_str()),
            (Some("open-repo-2"), Some("queued")),
            "{status}: {out}"
        );
        assert_eq!(
            open_jobs(&fx),
            [
                ("open-repo".to_owned(), status.to_owned()),
                ("open-repo-2".to_owned(), "queued".to_owned())
            ],
            "{status}"
        );
        assert_eq!(fx.plan_row().0, "dispatch", "{status}");
    }
}

#[test]
fn open_does_not_fetch_the_base_again_after_the_gate_so_a_base_that_moves_then_refuses_nothing() {
    let fx = poll_fx("dispatch");
    fx.hooks("pre-merge = \"move the base\"\n");
    let (attempt, _, tip) = fx.submitted_opener();
    // The base moves, in a conflicting way, while the Gate runs: B was read once, before it (hs-d326).
    let (ports, main) = (fx.fakes.ports.clone(), fx.repo.clone());
    fx.sh(move |_| {
        fs::write(main.join("api.txt"), "late\n").unwrap();
        git_in_hook(&ports, &main, &["add", "-A"]);
        git_in_hook(&ports, &main, &["commit", "-q", "-m", "late"]);
        git_in_hook(&ports, &main, &["push", "-q", "origin", "main"]);
        Reply::ok("")
    });

    let end = fx.merge(&attempt);

    assert_eq!(end, End::Merged { sha: tip.clone() });
    assert_eq!(fx.plan_row().0, "deploy");
    assert_eq!(fx.remote_branch("cpd/pay"), Some(tip));
}

#[test]
fn both_hooks_see_no_cpd_variable() {
    let fx = poll_fx("dispatch");
    // `cpd` is run with CPD_HOME and more in its environment, and none of it reaches a hook.
    fx.hooks("pre-commit = \"env\"\npre-merge = \"env\"\n");
    fx.sh(|_| Reply::ok(""));
    fx.export("CPD_SOMETHING", "x");
    let (attempt, _, _) = fx.submitted_opener();

    fx.merge(&attempt);

    let hooks = fx.calls("sh");
    assert_eq!(hooks.len(), 2, "pre-commit and pre-merge");
    for call in hooks {
        let set: Vec<&String> = call
            .envs
            .iter()
            .filter(|(k, v)| k.starts_with("CPD_") && v.is_some())
            .map(|(k, _)| k)
            .collect();
        assert!(set.is_empty(), "{set:?}");
        assert!(
            call.envs
                .iter()
                .any(|(k, v)| k == "CPD_HOME" && v.is_none()),
            "CPD_HOME is withheld: {:?}",
            call.envs
        );
    }
}

#[test]
fn open_an_unknown_plan_is_plan_not_found() {
    let fx = poll_fx("dispatch");
    let run = fx.cpd(&["pr", "open", "--plan", "nope"]);
    assert_eq!(code(&run), ("plan_not_found".into(), 4));
}

#[test]
fn open_push_and_rebase_run_no_r_for_another_prs_orphaned_merge() {
    // Nobody but E2 and guard U runs R (hs-d243), and guard U only for its own PR's
    // Attempts: this pi has no install, so asking it for a catalog would fail.
    for command in ["open", "push", "rebase"] {
        let fx = poll_fx(if command == "open" {
            "dispatch"
        } else {
            "deploy"
        });
        fx.fake_pi(Reply::exit(1, "", ""));
        // The fake git has no `ls-files` (the Gate hides the Lead worktree's untracked files with it): no file is untracked.
        let _ = (*fx.fakes.runner).clone().on(
            "git",
            |call| call.args.iter().any(|a| a == "ls-files"),
            Reply::ok(""),
        );
        orphaned_merge(&fx);

        let run = fx.pr(command);
        assert_eq!(run.exit, 0, "{command}: {}", run.stdout);

        let status: String = fx
            .store()
            .read(|c| {
                Ok(
                    c.query_row("SELECT status FROM attempts WHERE id = 'x-a1'", [], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap();
        assert_eq!(status, "merging", "{command}");
    }
}

/// Plan `oth` on the same repo whose driver died right after its merge's compare-and-swap (§7.4 step 8): the Attempt is
/// `merging`, its commit is on `cpd/oth`, and the Ledger knows nothing of it.
fn orphaned_merge(fx: &Poll) {
    let oth = PlanId::parse("oth").unwrap();
    let setup = Setup {
        timeout: Duration::from_secs(30),
        log: fx.env.root().join("oth.log"),
    };
    let (lead, made, tip) = fx.scoped(|| {
        let lead = create_lead_worktree(&fx.repo, &oth, "main", &setup)
            .unwrap()
            .path;
        let attempt = AttemptId::parse("x-a1").unwrap();
        let made = create_attempt_worktree(&fx.repo, &lead, &oth, &attempt, &setup).unwrap();
        (lead, made.clone(), made.path)
    });
    fs::write(tip.join("x.txt"), "x\n").unwrap();
    fx.git(&tip, &["add", "-A"]);
    fx.git(&tip, &["commit", "-q", "-m", "x"]);
    let sha = fx.git(&tip, &["rev-parse", "HEAD"]);
    fx.store()
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead2', 'lead', 'ctx', 'oth', 'Lead', '/', 'O3', 'idle', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                   VALUES ('oth', 'ctx', 'Other', 'dispatch', 'lead2', 't', 't');
                 INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
                   VALUES ('oth', 'repo', '{repo}', 'main', 'dispatch', 'cpd/oth', '{lead}', cpd_txn(), 't', 't');",
                repo = fx.repo.display(),
                lead = lead.display()
            ))?;
            tx.execute(
                "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, created_txn, created_at)
                 VALUES ('oth', 'repo', 'x', 'X', 'S2', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', 'running', 0, ?1, ?2, 't')",
                params!["f".repeat(64), tx.id()],
            )?;
            tx.execute(
                "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha,
                                       submitted_sha, submitted_at, started_at)
                 VALUES ('oth', 'x-a1', 'x', 1, 'S2', 'ladder', 'merging', ?1, ?2, ?3, ?4, 't', 't')",
                params![made.branch, made.path.to_str().unwrap(), made.base_sha, sha],
            )?;
            Ok(())
        })
        .unwrap();
    fx.git(
        &fx.repo,
        &["update-ref", "refs/heads/cpd/oth", &sha, &made.base_sha],
    );
}

#[test]
fn open_without_a_selected_plan_is_plan_required() {
    let fx = poll_fx("dispatch");
    let run = fx.cpd(&["pr", "open"]);
    assert_eq!(code(&run), ("plan_required".into(), 2));
}

// -------------------------------------------------------------- R before the Gate

const LEVELS: &str = r#"{"O":["off","low","medium","high","xhigh","max"],"S":["off","low","medium"],"H":["off","low","medium"],"G":["off"]}"#;

/// A pi install whose catalog module a fake `node` answers: R reads pi's catalog.
fn install_pi(fx: &Poll) {
    fx.install_pi();
    fx.fake_pi(Reply::ok(""));
    fx.fake_node(Reply::ok(&format!("{LEVELS}\n")));
}

/// Job `x` of PR `repo` with Attempt `x-a1` in `merging` whose merge's compare-and-swap
/// happened (§7.4 step 8) but not its step 10: its commit (`x.txt`) is on `cpd/pay`, the
/// Lead worktree is stale, and the Ledger knows nothing of it. Returns its worktree and tip.
fn unrecorded_merge(fx: &Poll) -> (PathBuf, String) {
    let attempt = AttemptId::parse("x-a1").unwrap();
    let setup = Setup {
        timeout: Duration::from_secs(30),
        log: fx.env.root().join("x.log"),
    };
    let made = fx
        .scoped(|| create_attempt_worktree(&fx.repo, &fx.lead, &plan(), &attempt, &setup))
        .unwrap();
    fs::write(made.path.join("x.txt"), "x\n").unwrap();
    fx.git(&made.path, &["add", "-A"]);
    fx.git(&made.path, &["commit", "-q", "-m", "x"]);
    let tip = fx.git(&made.path, &["rev-parse", "HEAD"]);
    fx.store()
        .write("fixture", |tx| {
            tx.execute(
                "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, created_txn, created_at)
                 VALUES ('pay', 'repo', 'x', 'X', 'S2', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', 'running', 0, ?1, ?2, 't')",
                params!["f".repeat(64), tx.id()],
            )?;
            tx.execute(
                "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha,
                                       submitted_sha, submitted_at, started_at)
                 VALUES ('pay', 'x-a1', 'x', 1, 'S2', 'ladder', 'merging', ?1, ?2, ?3, ?4, 't', 't')",
                params![made.branch, made.path.to_str().unwrap(), made.base_sha, tip],
            )?;
            Ok(())
        })
        .unwrap();
    fx.git(
        &fx.repo,
        &["update-ref", "refs/heads/cpd/pay", &tip, &made.base_sha],
    );
    (made.path, tip)
}

/// `(status, outcome, merged_sha)` of Attempt `x-a1` of Plan `pay`, and its Job's status.
fn attempt_x(fx: &Poll) -> ((String, Option<String>, Option<String>), String) {
    fx.store()
        .read(|c| {
            let attempt = c.query_row("SELECT status, outcome, merged_sha FROM attempts WHERE plan = 'pay' AND id = 'x-a1'", [], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?;
            let job = c.query_row("SELECT status FROM jobs WHERE plan = 'pay' AND key = 'x'", [], |r| r.get(0))?;
            Ok((attempt, job))
        })
        .unwrap()
}

#[test]
fn rebase_runs_r_first_for_a_merge_of_its_pr_whose_driver_died() {
    let fx = rebase_fx("deploy");
    install_pi(&fx);
    let (worktree, tip) = unrecorded_merge(&fx);
    let base = fx.advance_base("base.txt", "base\n");

    let out = fx.pr("rebase").ok();

    // Guard U: R ended the Attempt `merged` before the rewrite, not `crashed`.
    assert_eq!(
        attempt_x(&fx),
        (
            (
                "ended".to_owned(),
                Some("merged".to_owned()),
                Some(tip.clone())
            ),
            "merged".to_owned()
        )
    );
    let source: String = fx
        .store()
        .read(|c| Ok(c.query_row("SELECT t.command FROM events e JOIN txns t ON t.id = e.txn WHERE e.kind = 'job-merged'", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(source, "pr rebase");
    assert!(
        !worktree.exists(),
        "R's rest of A9 removed the Attempt's worktree"
    );
    // Then the rebase of S, which holds the merge, onto the base; pushed despite R's own push.
    let new = fx.lead_sha();
    assert_eq!(
        out,
        json!({"ok": true, "plan": "pay", "pr": "repo", "rebased": true, "old": tip, "new": new, "pushed": true})
    );
    assert!(
        fx.git(&fx.repo, &["merge-base", "--is-ancestor", &base, &new])
            .is_empty()
    );
    assert_eq!(fx.remote_branch("cpd/pay"), Some(new));
    // Rule S: the Lead worktree holds both, clean.
    assert_eq!(fs::read_to_string(fx.lead.join("x.txt")).unwrap(), "x\n");
    assert_eq!(
        fs::read_to_string(fx.lead.join("base.txt")).unwrap(),
        "base\n"
    );
    assert_eq!(fx.git(&fx.lead, &["status", "--porcelain"]), "");
    assert!(fx.events("error").is_empty(), "{:?}", fx.events("error"));
}

#[test]
fn rebase_waits_for_a_live_driver_to_record_its_merge() {
    let mut fx = rebase_fx("deploy");
    fx.fake_pi(Reply::exit(1, "", ""));
    let (_, tip) = unrecorded_merge(&fx);
    fx.advance_base("base.txt", "base\n");
    let attempt = AttemptId::parse("x-a1").unwrap();
    let driver = cpd_core::merge::take_free_driver_lock(&fx.home, &plan(), &attempt)
        .unwrap()
        .expect("the driver lock is free");

    // What the rebase waits on is the clock: at its first nap (guard U reads the store every 200 ms) the Lead Branch is
    // unrewritten and the Attempt `merging`; then the driver's step 10 is committed, with its lock still held.
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (ports, repo, home, recorded) = (
        fx.fakes.ports.clone(),
        fx.repo.clone(),
        fx.home.clone(),
        tip.clone(),
    );
    let during = seen.clone();
    fx.fx.on_nap(move |_| {
        let lead = git_out_in_hook(&ports, &repo, &["rev-parse", "refs/heads/cpd/pay"]);
        let mut store = Store::open(&home, 5000).unwrap();
        let status: String = store.read(|c| Ok(c.query_row("SELECT status FROM attempts WHERE plan = 'pay' AND id = 'x-a1'", [], |r| r.get(0))?)).unwrap();
        during.lock().unwrap().push((lead, status));
        store
            .write("attempt run", |tx| {
                tx.execute_batch(&format!(
                    "UPDATE attempts SET status = 'ended', outcome = 'merged', cause = 'merged', merged_sha = '{recorded}', ended_at = 't'
                      WHERE plan = 'pay' AND id = 'x-a1';
                     UPDATE jobs SET status = 'merged', merged_sha = '{recorded}' WHERE plan = 'pay' AND key = 'x';"
                ))?;
                Ok(())
            })
            .unwrap();
    });

    let out = fx.pr("rebase").ok();
    drop(driver);

    // Guard U waited: the Lead Branch was not rewritten while the merge was unrecorded.
    assert_eq!(
        *seen.lock().unwrap(),
        [(tip.clone(), "merging".to_owned())],
        "one nap, the merge unrecorded"
    );
    assert_eq!(fx.fakes.clock.naps(), [Duration::from_millis(200)]);
    let new = fx.lead_sha();
    assert_eq!(
        out,
        json!({"ok": true, "plan": "pay", "pr": "repo", "rebased": true, "old": tip, "new": new, "pushed": true})
    );
    assert_eq!(fx.remote_branch("cpd/pay"), Some(new));
    assert!(
        fx.events("job-merged").is_empty(),
        "R did not run: the live driver recorded it"
    );
}

// ------------------------------------------------------------------ pr poll
//
// In-process: `cpd pr poll` runs on `run_with` over the fake ports (the git route, the clock, a `gh` handler), against a
// Plan written straight into the database. The old tests drove the built binary against real git and a shell `gh`.

/// One scripted answer of the fake `gh`.
struct Slot(Mutex<Reply>);

impl Slot {
    fn new(reply: Reply) -> Slot {
        Slot(Mutex::new(reply))
    }

    fn set(&self, reply: Reply) {
        *self.0.lock().unwrap() = reply;
    }

    fn get(&self) -> Reply {
        self.0.lock().unwrap().clone()
    }

    /// The answer as it is, but `gh` exits `code` after printing it (the lookup does when one alias is NOT_FOUND).
    fn exits(&self, code: i32) {
        if let Reply::Exit { code: was, .. } = &mut *self.0.lock().unwrap() {
            *was = code;
        }
    }
}

/// The in-process `gh` of the poll tests: the search, the lookup of the pull requests no search lists, each answers its
/// [`Slot`]; `fail`, when set, makes every call fail with its text on stderr; any other call exits 9.
pub(super) struct Gh {
    search: Slot,
    lookup: Slot,
    /// `gh pr list --head <branch>`: the pull requests of the Lead Branch.
    list: Seq,
    /// `gh pr create`.
    create: Seq,
    /// `gh api -X PATCH repos/<repo>/pulls/<n> --input -` (`pr describe` of an open pull request).
    update: Seq,
    /// The test environment's root: the walk looking for a `.git` stops there.
    root: PathBuf,
    fail: Mutex<Option<String>>,
    /// The comments read (pr-comments): what it answers; None is an empty read of every pull request the query names.
    pub(super) comments: Mutex<Option<String>>,
}

/// Answers of one `gh` call in order: each is given once, the last one for ever.
struct Seq(Mutex<VecDeque<Reply>>);

impl Seq {
    fn new(reply: Reply) -> Seq {
        Seq(Mutex::new(VecDeque::from([reply])))
    }

    /// From now on `reply` is the only answer.
    fn set(&self, reply: Reply) {
        *self.0.lock().unwrap() = VecDeque::from([reply]);
    }

    /// After the answers so far, `reply`.
    fn then(&self, reply: Reply) {
        self.0.lock().unwrap().push_back(reply);
    }

    fn get(&self) -> Reply {
        let mut queue = self.0.lock().unwrap();
        if queue.len() > 1 {
            queue.pop_front().unwrap()
        } else {
            queue[0].clone()
        }
    }
}

/// Whether `cwd` or one of its ancestors below `root` holds a `.git` (the walk stops at `root`: the temp dir can sit in a checkout).
pub(super) fn in_git_repo(cwd: &Path, root: &Path) -> bool {
    cwd.ancestors()
        .take_while(|dir| *dir != root)
        .any(|dir| dir.join(".git").exists())
}

impl Handler for Gh {
    fn handle(&self, call: &Call) -> Reply {
        // Like gh: a call that resolves its repo from the cwd fails outside a git repository.
        let first = call.args.first().map(String::as_str);
        let named = call.args.iter().any(|a| a == "--repo" || a == "-R")
            || call.envs.iter().any(|(k, v)| k == "GH_REPO" && v.is_some());
        let placeholder = first == Some("api")
            && call
                .args
                .iter()
                .any(|a| a.contains("{owner}") || a.contains("{repo}"));
        if ((first == Some("pr") && !named) || placeholder)
            && call
                .cwd
                .as_deref()
                .is_some_and(|cwd| !in_git_repo(cwd, &self.root))
        {
            return Reply::exit(
                1,
                "",
                "failed to run git: fatal: not a git repository (or any of the parent directories): .git\n",
            );
        }
        if let Some(text) = self.fail.lock().unwrap().as_ref() {
            return Reply::exit(1, "", &format!("{text}\n"));
        }
        let args: Vec<&str> = call.args.iter().map(String::as_str).collect();
        match args[..] {
            ["pr", "list", ..] => self.list.get(),
            ["pr", "create", ..] => self.create.get(),
            ["api", "-X", "PATCH", _, "--input", "-"] => self.update.get(),
            ["api", "graphql", "-f", query, ..] if query.contains("search(type: ISSUE") => {
                self.search.get()
            }
            ["api", "graphql", "-f", query, ..] if query.contains("comments(last:") => {
                match &*self.comments.lock().unwrap() {
                    Some(text) => Reply::ok(&format!("{text}\n")),
                    None => Reply::ok(&format!("{}\n", empty_comments(query))),
                }
            }
            // The checks read (poll step 3b): by the rollup state the search reports for each pull request: PENDING or EXPECTED
            // is one running check, anything else one failing check, so a FAILURE rollup is confirmed; it carries no label (a test that
            // needs labels answers the read itself). `pr view` is answered by the `pr_view` mock, whose labels are settable.
            ["api", "graphql", "-f", query, ..] if query.contains("contexts(first: 100") => {
                let searched = match self.search.get() {
                    Reply::Exit { stdout, .. } => String::from_utf8_lossy(&stdout).into_owned(),
                    _ => String::new(),
                };
                let mut data = serde_json::Map::new();
                for (i, part) in query.split(": repository(").skip(1).enumerate() {
                    let number = part
                        .split("pullRequest(number: ")
                        .nth(1)
                        .and_then(|t| t.split(')').next())
                        .unwrap_or_default();
                    let rollup = searched
                        .split(&format!(r#""number":{number},"#))
                        .nth(1)
                        .and_then(|t| t.split(r#""statusCheckRollup":"#).nth(1))
                        .unwrap_or_default();
                    let running = rollup.starts_with(r#"{"state":"PENDING""#)
                        || rollup.starts_with(r#"{"state":"EXPECTED""#);
                    let run = if running {
                        serde_json::json!({"__typename": "CheckRun", "name": "ci", "status": "IN_PROGRESS", "conclusion": null})
                    } else {
                        serde_json::json!({"__typename": "CheckRun", "name": "ci", "status": "COMPLETED", "conclusion": "FAILURE"})
                    };
                    let page = serde_json::json!({"pageInfo": {"hasNextPage": false, "endCursor": null}, "nodes": [run]});
                    let pull = serde_json::json!({"labels": {"nodes": []}, "commits": {"nodes": [{"commit": {"statusCheckRollup": {"contexts": page}}}]}});
                    data.insert(format!("p{i}"), serde_json::json!({"pullRequest": pull}));
                }
                Reply::ok(&format!("{}\n", serde_json::json!({"data": data})))
            }
            ["api", "graphql", "-f", query, ..] if query.contains("p0: repository") => {
                self.lookup.get()
            }
            ["api", "graphql", "-f", query, ..] => {
                Reply::exit(9, "", &format!("unexpected graphql query: {query}\n"))
            }
            _ => Reply::exit(9, "", &format!("unexpected gh call: {}\n", args.join(" "))),
        }
    }
}

/// The in-process env of the poll tests: fake ports (the git route, the `gh` above), repo `pay` with a bare `origin`, the
/// Lead worktree of Plan `pay`, and the Plan in the database. It derefs to the [`Kit`] (`cpd`, `git`, `env`, `calls`, ...).
pub(super) struct Poll {
    fx: Kit,
    home: CpdHome,
    repo: PathBuf,
    origin: PathBuf,
    pub(super) lead: PathBuf,
    merged_sha: String,
    /// What the lock pushed to the Lead Branch on origin (R35, §7.2): the base.
    lock_pushed: String,
    branch: String,
    pub(super) gh: Arc<Gh>,
}

impl Deref for Poll {
    type Target = Kit;

    fn deref(&self) -> &Kit {
        &self.fx
    }
}

impl Rows for Poll {
    fn home(&self) -> &CpdHome {
        &self.home
    }
}

pub(super) fn poll_fx(phase: &str) -> Poll {
    poll_fx_with(phase, "")
}

fn poll_fx_with(phase: &str, config_extra: &str) -> Poll {
    poll_fx_full(phase, config_extra, "merged")
}

/// `job` is the status of the Plan's one Job, `merged` or `cancelled`.
fn poll_fx_full(phase: &str, config_extra: &str, job: &str) -> Poll {
    poll_fx_branch(phase, config_extra, job, "cpd/pay")
}

/// A Plan whose PR `repo` has the Lead Branch `cpd/pay/repo`: git cannot hold `cpd/pay` and `cpd/pay/web` both.
fn poll_fx_stacked(phase: &str) -> Poll {
    poll_fx_branch(phase, "", "merged", "cpd/pay/repo")
}

fn poll_fx_branch(phase: &str, config_extra: &str, job: &str, branch: &str) -> Poll {
    let fx = Kit::with_ports();
    fx.env.owner_policy("acme", "[pr]\ngate = \"local\"\n");
    let repo = fx.keyed_repo("pay").canonicalize().unwrap();
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
    fx.git(&repo, &["push", "-q", "origin", "main"]);
    let setup = Setup {
        timeout: Duration::from_secs(30),
        log: fx.env.root().join("lead.log"),
    };
    let (lead, lock_pushed) = fx.scoped(|| {
        if branch == "cpd/pay" {
            let created = create_lead_worktree(&repo, &plan(), "main", &setup).unwrap();
            (created.path, created.base_sha)
        } else {
            let path = repo.parent().unwrap().join("pay.pay.repo");
            let prepared = cpd_core::git::worktree::prepare_lead_worktree(
                &repo,
                &plan(),
                &path,
                branch,
                "main",
                &setup,
            )
            .map_err(|f| f.error)
            .unwrap();
            prepared.push().unwrap();
            (prepared.path, prepared.base_sha)
        }
    });
    fx.git(&lead, &["config", "user.name", "cpd test"]);
    fx.git(&lead, &["config", "user.email", "test@cpd.invalid"]);
    fx.commit_file(&lead, "api.txt", "api\n", "api");
    let merged_sha = fx.git(&lead, &["rev-parse", "HEAD"]);

    fx.env.write_config(&format!(
        "version = 1\nmachine = \"t\"\n[timeouts]\n{config_extra}\n[tmux]\nsocket = \"{}\"\n",
        fx.env.tmux_socket()
    ));
    let gh = Arc::new(Gh {
        search: Slot::new(Reply::ok(&format!("{EMPTY_SEARCH}\n"))),
        // `pr open` reads `createdAt` of the pull request it just opened (store-W5).
        list: Seq::new(Reply::ok("[]\n")),
        create: Seq::new(Reply::ok(&format!("{URL}\n"))),
        update: Seq::new(Reply::ok("")),
        root: fx.env.root().to_path_buf(),
        lookup: Slot::new(if phase == "dispatch" {
            Reply::ok(&format!(
                "{}\n",
                lookup_answer(&[Some(pay("OPEN", None, None))])
            ))
        } else {
            Reply::ok("{\"data\":{}}\n")
        }),
        fail: Mutex::new(None),
        comments: Mutex::new(None),
    });
    let _ = (*fx.fakes.runner).clone().route("gh", gh.clone());
    let home = CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
    // The PR's row before the Plan's: `fx.git` of the Lead worktree is all it needs of the repo.
    let poll = Poll {
        fx,
        home,
        repo,
        origin,
        lead,
        merged_sha,
        lock_pushed,
        branch: branch.to_owned(),
        gh,
    };
    seed_pay(
        poll.store(),
        phase,
        job,
        branch,
        &poll.repo,
        &poll.lead,
        &poll.merged_sha,
    );
    // The poll reads the catalog to floor the `update` Job's Tier to a valid rung.
    poll.install_catalog();
    poll
}

impl Poll {
    /// `cpd pr <command> --plan pay`.
    pub(super) fn pr(&self, command: &str) -> Run {
        self.cpd(&["pr", command, "--plan", "pay"])
    }

    /// What the fake `gh` answers to the search, as the text of its stdout.
    pub(super) fn search_raw(&self, text: &str) {
        self.gh.search.set(Reply::ok(&format!("{text}\n")));
    }

    /// What the fake `gh` answers to the lookup, as the text of its stdout.
    fn lookup_raw(&self, text: &str) {
        self.gh.lookup.set(Reply::ok(&format!("{text}\n")));
    }

    /// From now on the `gh api -X PATCH` update of a pull request fails with `text` on stderr.
    pub(super) fn fail_update(&self, text: &str) {
        self.gh.update.set(Reply::exit(1, "", &format!("{text}\n")));
    }

    /// Makes every call of the fake `gh` fail with `text` on stderr.
    pub(super) fn fail(&self, text: &str) {
        *self.gh.fail.lock().unwrap() = Some(text.to_owned());
    }

    /// The `pull_request` of the Plan's PR, as `plan show` prints it.
    fn shown_pull_request(&self) -> Value {
        self.cpd(&["plan", "show", "--plan", "pay"]).ok()["workflows"][0]["pull_request"].clone()
    }

    /// The argv, after `gh`, of every call of the fake `gh` so far.
    pub(super) fn gh_calls(&self) -> Vec<Vec<String>> {
        self.calls("gh").into_iter().map(|call| call.args).collect()
    }

    /// `cpd pr <command> --workflow <key> --plan pay`.
    fn pr_cmd(&self, command: &str, key: &str) -> Run {
        self.cpd(&["pr", command, "--workflow", key, "--plan", "pay"])
    }

    /// The keyed repo `web`, with a bare `origin` that has `main`.
    fn web_repo(&self) -> PathBuf {
        let repo = self.keyed_repo("web").canonicalize().unwrap();
        let origin = self.env.root().join("repos/web-origin.git");
        self.git(
            self.env.root(),
            &[
                "init",
                "--bare",
                "-q",
                "-b",
                "main",
                origin.to_str().unwrap(),
            ],
        );
        self.local_origin(&repo, &origin);
        self.git(&repo, &["push", "-q", "origin", "main"]);
        repo
    }

    /// A second PR `web` of Plan `pay`, waiting on PR `repo`, in its own repo: its rows and nothing else.
    fn add_waiting_web(&self) {
        let repo = self.web_repo();
        seed_web(self.store(), &repo, &repo, "", Web::Waiting);
    }

    /// [`Poll::add_waiting_web`], with PR `web` open as pull request 813 on its pushed Lead Branch `cpd/pay/web`, its Job `ui` merged.
    /// Returns PR `web`'s Lead worktree.
    fn add_open_web(&self) -> PathBuf {
        let repo = self.web_repo();
        let lead = repo.parent().unwrap().join("web.pay.web");
        self.git(
            &repo,
            &[
                "worktree",
                "add",
                "-q",
                "-b",
                "cpd/pay/web",
                lead.to_str().unwrap(),
                "main",
            ],
        );
        self.git(&lead, &["config", "user.name", "cpd test"]);
        self.git(&lead, &["config", "user.email", "test@cpd.invalid"]);
        self.commit_file(&lead, "ui.txt", "ui\n", "ui");
        let sha = self.git(&lead, &["rev-parse", "HEAD"]);
        self.git(&lead, &["push", "-q", "-u", "origin", "cpd/pay/web"]);
        seed_web(self.store(), &repo, &lead, &sha, Web::Open);
        lead
    }

    /// PR `key` of Plan `pay` open as pull request `number` of `acme/pay`, with the pull request's row `pr open` writes: the
    /// Plan's title, no review, no checks.
    fn mark_opened(&self, key: &str, number: i64) {
        let url = format!("https://github.com/acme/pay/pull/{number}");
        self.store()
            .write("fixture", |tx| {
                tx.execute_batch(&format!(
                    "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
                       VALUES ('acme/pay', {number}, '{url}', 'Pay the bills', 0, 'open', 'none', 'none', 'opened', NULL, 'opened');
                     UPDATE workflows SET status = 'open', pr_number = {number}, pr_url = '{url}', pr_opened_at = 'opened',
                            pull_request = (SELECT id FROM pull_requests WHERE number = {number})
                      WHERE plan = 'pay' AND key = '{key}';"
                ))?;
                Ok(())
            })
            .unwrap();
    }

    /// The `base` of PR `web` of Plan `pay`.
    fn web_base(&self) -> String {
        self.store()
            .read(|c| {
                Ok(c.query_row(
                    "SELECT base FROM workflows WHERE plan = 'pay' AND key = 'web'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    }

    fn web_status(&self) -> String {
        self.store()
            .read(|c| {
                Ok(c.query_row(
                    "SELECT status FROM workflows WHERE plan = 'pay' AND key = 'web'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    }

    /// The Lead Branch tip.
    pub(super) fn lead_sha(&self) -> String {
        self.git(
            &self.repo,
            &["rev-parse", &format!("refs/heads/{}", self.branch)],
        )
    }

    /// Sets the `[hooks]` of the repo `acme/pay`'s policy file to `toml` (`pre-merge = "cmd"` lines), and commits an
    /// empty commit on the Lead Branch.
    pub(super) fn hooks(&self, toml: &str) {
        self.env
            .repo_policy("acme", "pay", &format!("[hooks]\n{toml}"));
        self.git(
            &self.lead,
            &["commit", "-q", "--allow-empty", "-m", "hooks"],
        );
    }

    /// The tip of `branch` on `origin`, if it has one.
    pub(super) fn remote_branch(&self, branch: &str) -> Option<String> {
        let out = self.git(
            &self.origin,
            &[
                "for-each-ref",
                "--format=%(objectname)",
                &format!("refs/heads/{branch}"),
            ],
        );
        (!out.is_empty()).then_some(out)
    }

    /// Moves `main` on `origin` (through the main checkout) with a commit writing `path`.
    pub(super) fn advance_base(&self, path: &str, content: &str) -> String {
        self.commit_file(&self.repo, path, content, &format!("base {path}"));
        self.git(&self.repo, &["push", "-q", "origin", "main"]);
        self.git(&self.repo, &["rev-parse", "HEAD"])
    }

    /// The Gate's hooks (`sh -c <command>`) answer by `script`: what a shell would print and exit with (the Gate is
    /// external software: nothing runs it).
    fn sh(&self, script: impl Fn(&Call) -> Reply + Send + Sync + 'static) {
        let _ = (*self.fakes.runner)
            .clone()
            .route("sh", Arc::new(Sh(Box::new(script))));
    }

    /// The commands the Gate's hooks were run with (`sh -c <command>`), in order.
    fn commands(&self) -> Vec<String> {
        self.calls("sh")
            .iter()
            .map(|call| call.args[1].clone())
            .collect()
    }

    /// The PR's `open` Job `open-repo` in `status`, as engine E4 adds it (contract §3.7 E4: appended, no Dependencies).
    fn open_job(&self, status: &str) {
        self.store()
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, kind, fingerprint, merged_sha, failed_cause, cancel_reason, cancel_requested_at, created_txn, created_at)
                     VALUES ('pay', 'repo', 'open-repo', 'Open the pull request', 'S2', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', ?1, 1, 'open', iif(?1 = 'draft', NULL, ?2),
                             iif(?1 = 'merged', 'abc', NULL), iif(?1 = 'failed', 'crashed', NULL),
                             iif(?1 = 'cancelled', 'start over', NULL), iif(?1 = 'cancelled', 't', NULL), ?3, 't')",
                    params![status, "f".repeat(64), tx.id()],
                )?;
                Ok(())
            })
            .unwrap();
    }

    /// The PR's opener running, its Attempt `open-repo-a1` `merging` as `attempt submit` leaves it: its worktree is cut from the
    /// Lead Branch and the opener makes no commit (the branch fits its base as it is), so it submits the Lead tip. Returns the
    /// Attempt, its worktree and that tip.
    pub(super) fn submitted_opener(&self) -> (AttemptId, PathBuf, String) {
        let attempt = AttemptId::parse(OPENER_ATTEMPT).unwrap();
        let setup = Setup {
            timeout: Duration::from_secs(30),
            log: self.env.root().join("opener.log"),
        };
        let made = self
            .scoped(|| create_attempt_worktree(&self.repo, &self.lead, &plan(), &attempt, &setup))
            .unwrap();
        let sha = self.git(&made.path, &["rev-parse", "HEAD"]);
        self.open_job("running");
        self.store()
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha,
                                           submitted_sha, submitted_at, started_at)
                     VALUES ('pay', ?1, 'open-repo', 1, 'S2', 'ladder', 'merging', ?2, ?3, ?4, ?5, 't', 't')",
                    params![OPENER_ATTEMPT, made.branch, made.path.to_str().unwrap(), made.base_sha, sha],
                )?;
                Ok(())
            })
            .unwrap();
        (attempt, made.path, sha)
    }

    /// Driver step A8 for `attempt`, in this process, as `attempt run` runs it: the opener's Gate, push and pull request.
    pub(super) fn merge(&self, attempt: &AttemptId) -> End {
        let config = Config::load(&self.home).unwrap();
        let model = |m: &str| TierModel {
            model: m.to_owned(),
            efforts: vec![0, 1, 2, 3, 4, 5],
        };
        let catalog = Catalog {
            pi_version: "1.2.0".to_owned(),
            o: model("p/opus"),
            s: model("p/sonnet"),
            h: model("p/haiku"),
            g: TierModel {
                model: "p/gemini".to_owned(),
                efforts: vec![],
            },
        };
        let ctx = Ctx {
            home: &self.home,
            config: &config,
            catalog: &catalog,
            source: "attempt run",
            metrics: &|_| None,
            classifier: cpd_core::merge::no_verdict_classifier(),
        };
        self.scoped(|| merge::merge(&mut self.store(), &ctx, &plan(), attempt))
            .unwrap()
            .end
    }

    /// `run/pay/logs/open-repo-a1.<hook>.log`, the output of the opener's hook.
    fn open_log(&self, hook: &str) -> String {
        fs::read_to_string(
            self.home
                .plan_dir(&plan())
                .join(format!("logs/{OPENER_ATTEMPT}.{hook}.log")),
        )
        .unwrap_or_default()
    }

    /// The `pre-merge` output of the opener's Gate: [`Poll::open_log`] without the Gate's own `gate: ...` lines at its head.
    fn open_gate_log(&self) -> String {
        let log = self.open_log("pre-merge");
        let mut rest = log.as_str();
        while let Some(line) = rest.lines().next().filter(|l| l.starts_with("gate: ")) {
            rest = &rest[line.len() + 1..];
        }
        rest.to_owned()
    }
}

const OPENER_ATTEMPT: &str = "open-repo-a1";
/// The branch of Attempt `open-repo-a1`, what `{{ branch }}` is in the Gate of its merge.
const OPENER_BRANCH: &str = "cpd/pay--open-repo-a1";

/// The `sh` of the Gate's hooks, answering by a closure.
struct Sh(Box<dyn Fn(&Call) -> Reply + Send + Sync>);

impl Handler for Sh {
    fn handle(&self, call: &Call) -> Reply {
        (self.0)(call)
    }
}

/// The name of the directory a hook runs in: what `$(basename $PWD)` prints.
fn basename(call: &Call) -> String {
    call.cwd
        .as_ref()
        .expect("a hook runs in a directory")
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned()
}

/// A pull request as the search and the lookup print it. Its title and creation time are what the
/// fixtures stored for 812 and 813, so that only the other fields can differ.
pub(super) fn node(
    repo: &str,
    number: i64,
    state: &str,
    review: Option<&str>,
    rollup: Option<&str>,
) -> String {
    let title = if number == 813 {
        "Pay the bills (web)"
    } else {
        "Pay the bills"
    };
    let merged = if state == "MERGED" {
        r#""2026-10-14T10:00:00Z""#
    } else {
        "null"
    };
    let reviewers = if review == Some("APPROVED") {
        r#""latestOpinionatedReviews":{"nodes":[{"state":"APPROVED","author":{"__typename":"User","login":"amy"}}]},"#
    } else {
        ""
    };
    let review = review.map_or("null".to_owned(), |r| format!("\"{r}\""));
    let rollup = rollup.map_or("null".to_owned(), |s| format!(r#"{{"state":"{s}"}}"#));
    format!(
        r#"{{"repository":{{"nameWithOwner":"{repo}","isArchived":false}},"number":{number},"url":"https://github.com/{repo}/pull/{number}","title":"{title}",
        "isDraft":false,"state":"{state}","createdAt":"opened","updatedAt":"U1","reviewThreads":{{"nodes":[]}},"mergedAt":{merged},"reviewDecision":{review},{reviewers}
        "mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN","viewerDidAuthor":true,"viewerLatestReviewRequest":null,
        "commits":{{"nodes":[{{"commit":{{"statusCheckRollup":{rollup}}}}}]}}}}"#
    )
}

/// `node` with `latestOpinionatedReviews` exactly `reviewers`, each `(__typename, login, state)`: it replaces the field when the
/// node has it (an APPROVED node has `amy`), else inserts it right after `"reviewDecision":...,`.
pub(super) fn reviewed(node: String, reviewers: &[(&str, &str, &str)]) -> String {
    let nodes: Vec<String> = reviewers
        .iter()
        .map(|(kind, login, state)| {
            format!(r#"{{"state":"{state}","author":{{"__typename":"{kind}","login":"{login}"}}}}"#)
        })
        .collect();
    let field = format!(
        r#""latestOpinionatedReviews":{{"nodes":[{}]}},"#,
        nodes.join(",")
    );
    const KEY: &str = r#""latestOpinionatedReviews":"#;
    if let Some(start) = node.find(KEY) {
        let end = start + node[start..].find("]}").expect("the field ends") + 3;
        return format!("{}{field}{}", &node[..start], &node[end..]);
    }
    let at = node
        .find(r#""reviewDecision":"#)
        .expect("a node has a reviewDecision");
    let after = at + node[at..].find(',').expect("a comma follows") + 1;
    format!("{}{field}{}", &node[..after], &node[after..])
}

/// `node`, as GitHub reports its `mergeable` and `mergeStateStatus`.
fn merge_state(node: String, mergeable: &str, merge_state: &str) -> String {
    node.replace(
        r#""mergeable":"UNKNOWN","mergeStateStatus":"UNKNOWN""#,
        &format!(r#""mergeable":"{mergeable}","mergeStateStatus":"{merge_state}""#),
    )
}

/// `node`, as a direct read (the lookup) answers it for the viewer: whether the viewer `did_author` it and whether a review
/// is `requested` of them (`viewerLatestReviewRequest` is `{"id":"RR_1"}` or null). The search ignores both fields.
pub(super) fn viewer(node: String, did_author: bool, requested: bool) -> String {
    let request = if requested {
        r#"{"id":"RR_1"}"#
    } else {
        "null"
    };
    node.replace(
        r#""viewerDidAuthor":true,"viewerLatestReviewRequest":null"#,
        &format!(r#""viewerDidAuthor":{did_author},"viewerLatestReviewRequest":{request}"#),
    )
}

/// 812, the Plan's pull request.
pub(super) fn pay(state: &str, review: Option<&str>, rollup: Option<&str>) -> String {
    node("acme/pay", 812, state, review, rollup)
}

const RESET: &str = "2026-10-20T11:00:00Z";

/// The search's answer: `nodes` of the User's own pull requests, `requested` of those that ask for their
/// review, and a rate limit with `remaining` requests.
fn search_answer_with(nodes: &[String], requested: &[String], remaining: u32) -> String {
    format!(
        r#"{{"data":{{"search":{{"issueCount":{},"nodes":[{}]}},"reviewRequests":{{"issueCount":{},"nodes":[{}]}},"rateLimit":{{"cost":1,"remaining":{remaining},"resetAt":"{RESET}"}}}}}}"#,
        nodes.len(),
        nodes.join(","),
        requested.len(),
        requested.join(",")
    )
}

/// The search's answer with no review request.
fn search_answer(nodes: &[String], remaining: u32) -> String {
    search_answer_with(nodes, &[], remaining)
}

/// The lookup's answer, one alias per entry; `None` is an alias GitHub cannot find.
fn lookup_answer(nodes: &[Option<String>]) -> String {
    let data: Vec<String> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            format!(
                r#""p{i}":{{"pullRequest":{}}}"#,
                n.as_deref().unwrap_or("null")
            )
        })
        .collect();
    let errors: Vec<String> = nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.is_none())
        .map(|(i, _)| format!(r#"{{"type":"NOT_FOUND","path":["p{i}","pullRequest"],"message":"Could not resolve to a PullRequest"}}"#))
        .collect();
    match errors.is_empty() {
        true => format!(r#"{{"data":{{{}}}}}"#, data.join(",")),
        false => format!(
            r#"{{"data":{{{}}},"errors":[{}]}}"#,
            data.join(","),
            errors.join(",")
        ),
    }
}

/// `pr-feedback`'s data for PR `repo`.
pub(super) fn feedback(state: &str, checks: &str, decision: &str, reasons: &[&str]) -> Value {
    feedback_merge(state, checks, decision, "unknown", "unknown", reasons)
}

/// `pr-feedback`'s data for PR `repo`, with the row's `mergeable` and `merge_state`; `conflict` follows `reasons`.
fn feedback_merge(
    state: &str,
    checks: &str,
    decision: &str,
    mergeable: &str,
    merge_state: &str,
    reasons: &[&str],
) -> Value {
    json!({"workflow": "repo", "number": 812, "url": URL, "state": state, "checks": checks, "review_decision": decision,
           "mergeable": mergeable, "merge_state": merge_state, "conflict": reasons.contains(&"conflict"), "reasons": reasons})
}

impl Poll {
    /// What the fake `gh`'s search answers.
    pub(super) fn search(&self, nodes: &[String]) {
        self.search_raw(&search_answer(nodes, 4987));
    }

    /// What the fake `gh`'s search answers: `authored` in the `author:@me` search, `requested` in the
    /// `review-requested:@me` one.
    fn search_both(&self, authored: &[String], requested: &[String]) {
        self.search_raw(&search_answer_with(authored, requested, 4987));
    }

    /// `(repo, number, authored, review_requested)` of every row of `pull_requests`, in order.
    fn memberships(&self) -> Vec<(String, i64, bool, bool)> {
        self.store()
            .read(|c| {
                let mut stmt = c.prepare("SELECT repo, number, authored, review_requested FROM pull_requests ORDER BY id")?;
                Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<_>>()?)
            })
            .unwrap()
    }

    /// What the fake `gh`'s lookup answers.
    pub(super) fn lookup(&self, nodes: &[Option<String>]) {
        self.lookup_raw(&lookup_answer(nodes));
    }

    /// `pr poll --plan pay`.
    pub(super) fn poll(&self) -> Value {
        self.pr("poll").ok()
    }

    /// `pr poll`, as engine E5 runs it.
    fn poll_all(&self) -> Value {
        self.cpd(&["pr", "poll"]).ok()
    }
}

fn is_search(call: &[String]) -> bool {
    call[..3] == ["api", "graphql", "-f"]
        && call[3].starts_with(
            "query=query { search(type: ISSUE, query: \"is:pr is:open author:@me archived:false",
        )
}

fn is_lookup(call: &[String]) -> bool {
    call[..3] == ["api", "graphql", "-f"]
        && call[3].starts_with("query=query { p0: repository(owner: ")
        && !call[3].contains("contexts(first: 100")
}

#[test]
fn the_first_poll_inserts_every_pull_request_and_appends_nothing_for_unowned_ones() {
    let fx = poll_fx("dispatch");
    fx.search(&[
        node("acme/pay", 901, "OPEN", Some("APPROVED"), Some("SUCCESS")),
        node("acme/web", 902, "OPEN", None, None),
    ]);
    let events = fx.event_count();

    let out = fx.poll_all();

    assert_eq!(
        out,
        json!({"ok": true, "polled": 2,
               "changed": [
                   {"repo": "acme/pay", "number": 901, "state": "open", "draft": false, "checks": "passed", "review_decision": "approved", "mergeable": "unknown", "merge_state": "unknown"},
                   {"repo": "acme/web", "number": 902, "state": "open", "draft": false, "checks": "none", "review_decision": "none", "mergeable": "unknown", "merge_state": "unknown"}],
               "base_changed": [], "feedback": [], "update_jobs": [], "merged": [], "done": [],
               "rate_limit": {"cost": 1, "remaining": 4987, "reset_at": RESET}})
    );
    assert_eq!(
        fx.pull_requests(),
        [("acme/pay".to_owned(), 901), ("acme/web".to_owned(), 902)]
    );
    assert_eq!(fx.event_count(), events, "no PR owns them: no Event");
    // One search call, from `$CPD_HOME`; no lookup, since no PR is open.
    let calls = fx.gh_calls();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert!(is_search(&calls[0]), "{calls:?}");
    assert!(
        calls[0][3].ends_with("rateLimit { cost remaining resetAt } }"),
        "{}",
        calls[0][3]
    );
}

// ------------------------------------------------------------------ pr poll: the base follow (hs-d325, hs-d326)

/// `node`, with the base branch GitHub reports for the pull request (`baseRefName`).
fn based(node: String, base: &str) -> String {
    assert_eq!(node.matches(r#""mergedAt":"#).count(), 1);
    node.replace(
        r#""mergedAt":"#,
        &format!(r#""baseRefName":"{base}","mergedAt":"#),
    )
}

/// 814, the pull request of PR `up`.
fn up(state: &str) -> String {
    node("acme/pay", 814, state, None, None)
}

impl Poll {
    /// Adds PR `up` of Plan `pay`, `open` with its pull request 814, stacked on PR `repo` (its base is `cpd/pay`).
    fn stack_up(&self) {
        let sql = format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
               VALUES ('acme/pay', 814, 'https://github.com/acme/pay/pull/814', 'Pay the bills', 0, 'open', 'none', 'none', 'opened', NULL, 'opened');
             INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at,
                              pr_number, pr_url, pr_opened_at, pull_request)
               VALUES ('pay', 'up', '{}', 'cpd/pay', 'open', 'cpd/pay/up', '/nonexistent/up', cpd_txn(), 't', 't',
                       814, 'https://github.com/acme/pay/pull/814', 'opened', (SELECT id FROM pull_requests WHERE number = 814));",
            self.repo.display()
        );
        self.store()
            .write("fixture", |tx| Ok(tx.execute_batch(&sql)?))
            .unwrap();
    }

    /// `(status, base)` of PR `key` of Plan `pay`.
    fn base_of(&self, key: &str) -> (String, String) {
        self.store()
            .read(|c| {
                Ok(c.query_row(
                    "SELECT status, base FROM workflows WHERE plan = 'pay' AND key = ?1",
                    [key],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap()
    }

    /// `(draft, state)` of the pull request `number`, as the poll stored it.
    fn stored_pull_request(&self, number: i64) -> (bool, String) {
        self.store()
            .read(|c| {
                Ok(c.query_row(
                    "SELECT draft, state FROM pull_requests WHERE number = ?1",
                    [number],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?)
            })
            .unwrap()
    }
}

/// PR `web` of Plan `pay`, stacked on its PR `repo` (its Lead Branch `fx.branch`, whose
/// Lead worktree is `fx.lead`): started from that branch, one Merged Job's commit on it, its Lead Branch pushed.
fn poll_stacked_pr(fx: &Poll) {
    let path = fx.repo.parent().unwrap().join("pay.pay.web");
    let setup = Setup {
        timeout: Duration::from_secs(30),
        log: fx.env.root().join("web.log"),
    };
    let prepared = fx.scoped(|| {
        cpd_core::git::worktree::prepare_stacked_lead_worktree(
            &fx.repo,
            &plan(),
            &path,
            "cpd/pay/web",
            fx.branch.as_str(),
            &setup,
        )
        .map_err(|f| f.error)
        .unwrap()
    });
    assert_eq!(
        prepared.base_sha,
        fx.lead_sha(),
        "cut from the base PR's Lead Branch head"
    );
    fx.scoped(|| prepared.push().unwrap());
    fx.git(&path, &["config", "user.name", "cpd test"]);
    fx.git(&path, &["config", "user.email", "test@cpd.invalid"]);
    fx.commit_file(&path, "web.txt", "web\n", "web");
    let merged = fx.git(&path, &["rev-parse", "HEAD"]);
    fx.store()
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "UPDATE plans SET phase = 'dispatch', done_at = NULL WHERE id = 'pay' AND phase = 'done';
                 INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, lead_worktree, title, created_txn, created_at, started_at)
                   VALUES ('pay', 'web', '{repo}', 'pay', '{base}', 'dispatch', 'cpd/pay/web', '{path}', 'Ship the web', cpd_txn(), 't', 't');",
                repo = fx.repo.display(),
                base = fx.branch,
                path = path.display()
            ))?;
            tx.execute(
                "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, merged_sha, created_txn, created_at)
                 VALUES ('pay', 'web', 'ui', 'The UI', 'S2', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', 'merged', 0, ?1, ?2, ?3, 't')",
                params!["f".repeat(64), merged, tx.id()],
            )?;
            Ok(())
        })
        .unwrap();
}

/// Plan `pay`: PR `repo` (812, base `main`) and PR `up` (814) stacked on it, both open, as a poll stored them.
fn stacked() -> Poll {
    let fx = poll_fx("deploy");
    // The base PR's Lead Branch is pushed, clean: the clean-up of its merge removes the worktree, which unpushed work would keep.
    fx.git(&fx.lead, &["push", "-q", "origin", "cpd/pay"]);
    fx.stack_up();
    fx.search(&[
        based(pay("OPEN", None, None), "main"),
        based(up("OPEN"), "cpd/pay"),
    ]);
    let out = fx.poll();
    assert_eq!(
        out["base_changed"],
        json!([]),
        "GitHub agrees with cpd: nothing to follow"
    );
    assert_eq!(fx.base_of("up"), ("open".to_owned(), "cpd/pay".to_owned()));
    fx
}

#[test]
fn poll_follows_github_repointing_a_stacked_pr_when_its_base_pr_merges_and_keeps_it_ready() {
    let fx = stacked();
    let events = fx.event_count();
    // The base PR merged: GitHub deleted its branch and repointed 814 to `main`. 812 left the open search.
    fx.search(&[based(up("OPEN"), "main")]);
    fx.lookup(&[Some(pay("MERGED", None, None))]);

    let out = fx.poll();

    assert_eq!(
        out["base_changed"],
        json!([{"plan": "pay", "pr": "up", "from": "cpd/pay", "to": "main"}])
    );
    assert_eq!(out["merged"], json!([{"plan": "pay", "pr": "repo"}]));
    assert_eq!(out["feedback"], json!([]));
    assert_eq!(out["done"], json!([]), "up is still open");
    // The PR's base is `main` and nothing else about it moved: still open, its pull request still ready.
    assert_eq!(fx.base_of("up"), ("open".to_owned(), "main".to_owned()));
    assert_eq!(fx.stored_pull_request(814), (false, "open".to_owned()));
    assert_eq!(fx.plan_row().0, "deploy");
    // One informational Event for the Plan, once; no `pr-feedback`.
    let told = fx.events("workflow-base-changed");
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(
        told[0].0,
        "Workflow up of Plan pay now targets main (was cpd/pay): its base Workflow pay/repo merged and GitHub repointed the pull request. \
         Its tree is unchanged: no new Gate is needed and it stays ready. Read it with cpd plan show --plan pay."
    );
    assert_eq!(
        told[0].1,
        json!({"workflow": "up", "from": "cpd/pay", "to": "main", "from_workflow": "pay/repo", "to_workflow": null, "by": "poll",
               "number": 814, "url": "https://github.com/acme/pay/pull/814"})
    );
    assert!(fx.events("pr-feedback").is_empty());
    assert_eq!(
        fx.event_count() - events,
        2,
        "pr-merged and workflow-base-changed, nothing else"
    );
    // cpd changes no base on GitHub, runs no Gate and opens nothing: it only asked GitHub (graphql).
    assert!(
        fx.gh_calls().iter().all(|c| c[0] == "api"),
        "{:?}",
        fx.gh_calls()
    );
    assert!(
        !fx.home
            .plan_dir(&plan())
            .join("logs/pr-up.pre-merge.log")
            .exists(),
        "no Gate ran for up"
    );
    assert_eq!(fx.gate_log(), "", "no Gate ran for repo");

    // A second poll changes nothing.
    let (txns, events) = (fx.txns(), fx.event_count());
    let again = fx.poll();
    assert_eq!(again["base_changed"], json!([]));
    assert_eq!(again["changed"], json!([]));
    assert_eq!(
        (fx.txns(), fx.event_count()),
        (txns, events),
        "no write transaction, no Event"
    );
    assert_eq!(fx.base_of("up"), ("open".to_owned(), "main".to_owned()));
}

#[test]
fn poll_follows_a_base_retargeted_by_hand_on_any_open_pr() {
    let fx = stacked();
    fx.search(&[
        based(pay("OPEN", None, None), "develop"),
        based(up("OPEN"), "cpd/pay"),
    ]);

    let out = fx.poll();

    assert_eq!(
        out["base_changed"],
        json!([{"plan": "pay", "pr": "repo", "from": "main", "to": "develop"}])
    );
    assert_eq!(
        fx.base_of("repo"),
        ("open".to_owned(), "develop".to_owned())
    );
    assert_eq!(
        fx.base_of("up"),
        ("open".to_owned(), "cpd/pay".to_owned()),
        "814 still targets 812's branch, as GitHub says"
    );
    let told = fx.events("workflow-base-changed");
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(
        told[0].0,
        "Workflow repo of Plan pay now targets develop (was main): the base changed on GitHub. \
         Its tree is unchanged: no new Gate is needed and it stays ready. Read it with cpd plan show --plan pay."
    );
    assert_eq!(told[0].1["from_pr"], Value::Null);
    assert_eq!(told[0].1["to_pr"], Value::Null);

    let events = fx.event_count();
    assert_eq!(fx.poll()["base_changed"], json!([]));
    assert_eq!(fx.event_count(), events);
}

#[test]
fn poll_changes_nothing_for_a_pr_stacked_above_a_base_pr_closed_unmerged() {
    let fx = stacked();
    // GitHub deletes the branch of a merged pull request only: 812 is closed, 814 still targets `cpd/pay`.
    fx.search(&[based(up("OPEN"), "cpd/pay")]);
    fx.lookup(&[Some(pay("CLOSED", None, None))]);

    let out = fx.poll();

    assert_eq!(out["base_changed"], json!([]));
    assert_eq!(
        out["feedback"],
        json!([{"plan": "pay", "pr": "repo"}]),
        "the closed pull request is the poll's usual pr-feedback"
    );
    assert_eq!(fx.base_of("up"), ("open".to_owned(), "cpd/pay".to_owned()));
    assert_eq!(fx.stored_pull_request(814), (false, "open".to_owned()));
    assert!(fx.events("workflow-base-changed").is_empty());

    // The same answers again: still nothing, and the base PR is not cancelled out from under the stacked PR.
    let (txns, events) = (fx.txns(), fx.event_count());
    assert_eq!(fx.poll()["base_changed"], json!([]));
    assert_eq!((fx.txns(), fx.event_count()), (txns, events));
    let run = fx.cpd(&["workflow", "cancel", "repo", "--plan", "pay"]);
    let error = common::refused(&run, "workflow_in_use", 3);
    assert_eq!(error["details"]["workflows"], json!(["up"]), "{error}");
    assert_eq!(fx.base_of("repo").0, "open");
    assert_eq!(fx.base_of("up"), ("open".to_owned(), "cpd/pay".to_owned()));
}

/// What PR `web` of Plan `pay` is told when its base PR `pay/repo` merged before `web` had a pull request (hs-d301): `by` is who
/// unstacked it, `number` and `url` are the poll's, both null.
fn web_unstacked(from: &str, by: &str) -> (String, Value) {
    let data = match by {
        "poll" => {
            json!({"workflow": "web", "from": from, "to": "main", "from_workflow": "pay/repo", "to_workflow": null, "by": "poll", "number": null, "url": null})
        }
        _ => {
            json!({"workflow": "web", "from": from, "to": "main", "from_workflow": "pay/repo", "to_workflow": null, "by": by})
        }
    };
    (
        format!(
            "Workflow web of Plan pay now targets main (was {from}): its base Workflow pay/repo merged before its pull request existed. \
             Its tree is unchanged: no new Gate is needed and it stays ready. Read it with cpd plan show --plan pay."
        ),
        data,
    )
}

#[test]
fn poll_unstacks_a_dispatch_pr_in_the_transaction_that_records_its_base_prs_merge() {
    let fx = poll_fx_stacked("deploy");
    poll_stacked_pr(&fx);
    assert_eq!(fx.web_base(), fx.branch);
    // pay/repo merged (a pull request the search no longer lists); web has no pull request.
    fx.search(&[]);
    fx.lookup(&[Some(pay("MERGED", None, None))]);

    let out = fx.poll();

    assert_eq!(out["merged"], json!([{"plan": "pay", "pr": "repo"}]));
    assert_eq!(fx.web_base(), "main");
    assert_eq!(
        fx.plan_events("pay", "workflow-base-changed"),
        vec![web_unstacked(&fx.branch, "poll")],
        "once"
    );
    assert_eq!(fx.plan_events("pay", "pr-feedback"), vec![]);
    assert_eq!(
        fx.web_status(),
        "dispatch",
        "nothing else about web changed: no Gate, no start, no pull request"
    );
    assert!(
        fx.gh_calls().iter().all(|c| c[0] == "api"),
        "{:?}",
        fx.gh_calls()
    );

    // A second poll changes nothing.
    let (txns, events) = (fx.txns(), fx.event_count());
    fx.poll();
    assert_eq!((fx.txns(), fx.event_count()), (txns, events));
}

#[test]
fn poll_does_not_tell_the_plan_of_failed_checks_when_the_policy_says_none() {
    let fx = poll_fx("deploy");
    fx.env
        .owner_policy("acme", "[pr.feedback]\nchecks = \"none\"\n");
    fx.search(&[pay("OPEN", None, Some("FAILURE"))]);

    let out = fx.poll();

    assert_eq!(out["feedback"], json!([]));
    assert_eq!(fx.pr_data().2, "failed", "the row still records the checks");
    assert!(fx.events("pr-feedback").is_empty());
}

#[test]
fn poll_tells_the_plan_once_that_the_checks_turned_failed() {
    let fx = poll_fx("deploy");
    fx.search(&[pay("OPEN", None, Some("FAILURE"))]);

    let out = fx.poll();

    assert_eq!(out["feedback"], json!([{"plan": "pay", "pr": "repo"}]));
    assert_eq!(
        out["changed"],
        json!([{"repo": "acme/pay", "number": 812, "state": "open", "draft": false, "checks": "failed", "review_decision": "none", "mergeable": "unknown", "merge_state": "unknown"}])
    );
    assert_eq!(
        fx.pr_data(),
        (
            "Pay the bills".into(),
            "none".into(),
            "failed".into(),
            "open".into()
        )
    );
    let events = fx.events("pr-feedback");
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].1,
        feedback("open", "failed", "none", &["checks_failed"])
    );
    assert_eq!(
        events[0].0,
        "Workflow repo (#812) of Plan pay needs attention: checks failed. Read the details with cpd pr view --plan pay --workflow repo."
    );
    assert_eq!(fx.plan_row().0, "deploy");

    // Still failing: nothing new. `ERROR` is `failed` too.
    fx.search(&[pay("OPEN", None, Some("ERROR"))]);
    assert_eq!(fx.poll()["feedback"], json!([]));
    // Pending, then failing again: from anything else, so told again.
    fx.search(&[pay("OPEN", None, Some("PENDING"))]);
    assert_eq!(fx.poll()["feedback"], json!([]));
    fx.search(&[pay("OPEN", None, Some("FAILURE"))]);
    assert_eq!(
        fx.poll()["feedback"],
        json!([{"plan": "pay", "pr": "repo"}])
    );
    assert_eq!(fx.events("pr-feedback").len(), 2);
}

impl Poll {
    /// `(key, status)` of every `update` Job of Plan `pay`, by key.
    fn update_jobs(&self) -> Vec<(String, String)> {
        self.store()
            .read(|c| {
                let mut stmt = c.prepare("SELECT key, status FROM jobs WHERE plan = 'pay' AND kind = 'update' ORDER BY key")?;
                Ok(stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?)
            })
            .unwrap()
    }

    /// GitHub says pull request 812 is `mergeable` / `merge_state`; one poll of Plan `pay`.
    fn polled(&self, mergeable: &str, merge_state_: &str) -> Value {
        self.search(&[merge_state(
            pay("OPEN", None, None),
            mergeable,
            merge_state_,
        )]);
        self.poll()
    }
}

/// The poll queues the PR's `update` Job in the transaction that stores `conflicting` (contract §7.6 step 6, §3.7 E5): `up-repo`, Tier
/// `worker_tier`, `queued`, `appended`, no Dependencies, and `job-added` with `kind` `update`, `by` `poll`, `reason` `conflict`. The conflict
/// is no `pr-feedback`, and the PR stays `open`. The same conflict seen again writes nothing.
#[test]
fn poll_queues_one_update_job_when_the_pull_request_becomes_conflicting() {
    let fx = poll_fx("deploy");
    fx.polled("MERGEABLE", "CLEAN");
    assert_eq!(fx.update_jobs(), Vec::<(String, String)>::new());

    let out = fx.polled("CONFLICTING", "DIRTY");

    assert_eq!(
        fx.update_jobs(),
        [("up-repo".to_owned(), "queued".to_owned())]
    );
    assert_eq!(
        out["feedback"],
        json!([]),
        "a conflict is no pr-feedback reason"
    );
    assert!(fx.events("pr-feedback").is_empty());
    let row: (String, String, i64, bool) = fx
        .store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT title, tier, appended, fingerprint IS NOT NULL FROM jobs WHERE plan = 'pay' AND key = 'up-repo'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?)
        })
        .unwrap();
    assert_eq!(
        row,
        (
            format!("Bring the pull request up to date: {REPO_TITLE}"),
            "H3".into(),
            1,
            true
        )
    );
    let dependencies: i64 = fx
        .store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM dependencies WHERE job = 'up-repo'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(dependencies, 0);
    let added = fx.events("job-added");
    assert_eq!(added.len(), 1, "{added:?}");
    let data = &added[0].1;
    assert_eq!(
        (
            &data["kind"],
            &data["by"],
            &data["reason"],
            &data["workflow"]
        ),
        (
            &json!("update"),
            &json!("poll"),
            &json!("conflict"),
            &json!("repo")
        )
    );
    assert_eq!(fx.workflow_status("repo"), "open");

    let txns = fx.txns();
    fx.polled("CONFLICTING", "DIRTY");
    assert_eq!(
        (fx.txns(), fx.update_jobs().len()),
        (txns, 1),
        "the same conflict again writes nothing"
    );
}

/// A base that merely moved queues nothing (hs-d497: no `behind` detection): `merge_state` `behind` is stored as GitHub gives it and is
/// no cause, whatever `mergeable` says. The conflict that follows does queue the Job.
#[test]
fn poll_queues_no_update_job_for_a_base_that_merely_moved_and_one_for_the_conflict_that_follows() {
    let fx = poll_fx("deploy");
    fx.polled("MERGEABLE", "CLEAN");

    for (mergeable, state) in [
        ("MERGEABLE", "BEHIND"),
        ("UNKNOWN", "BEHIND"),
        ("MERGEABLE", "BLOCKED"),
        ("MERGEABLE", "UNSTABLE"),
    ] {
        fx.polled(mergeable, state);
        assert_eq!(
            fx.update_jobs(),
            Vec::<(String, String)>::new(),
            "{mergeable}/{state}"
        );
        assert_eq!(
            fx.shown_pull_request()["merge_state"],
            state.to_lowercase(),
            "{mergeable}/{state}: stored as GitHub says"
        );
    }
    assert!(fx.events("job-added").is_empty());

    fx.polled("CONFLICTING", "DIRTY");

    assert_eq!(
        fx.update_jobs(),
        [("up-repo".to_owned(), "queued".to_owned())]
    );
}

/// "Once per conflict": no second `update` Job while one is `queued` or `running`, however often the conflict is seen again, and a conflict
/// that comes after the Job ended (`merged`) is a new cause, a new Job, `up-repo-2`.
#[test]
fn poll_queues_no_second_update_job_while_one_is_live_and_a_new_one_for_a_conflict_after_it_ended()
{
    let fx = poll_fx("deploy");
    fx.polled("CONFLICTING", "DIRTY");
    assert_eq!(
        fx.update_jobs(),
        [("up-repo".to_owned(), "queued".to_owned())]
    );

    // GitHub recomputes after the base moved (`unknown`) and the conflict is seen as a transition again: the Job is still queued.
    fx.polled("UNKNOWN", "UNKNOWN");
    fx.polled("CONFLICTING", "DIRTY");
    assert_eq!(
        fx.update_jobs(),
        [("up-repo".to_owned(), "queued".to_owned())]
    );

    // Still no second one while it runs.
    fx.store()
        .write("fixture", |tx| {
            Ok(tx.execute_batch("UPDATE jobs SET status = 'running' WHERE key = 'up-repo';")?)
        })
        .unwrap();
    fx.polled("UNKNOWN", "UNKNOWN");
    fx.polled("CONFLICTING", "DIRTY");
    assert_eq!(
        fx.update_jobs(),
        [("up-repo".to_owned(), "running".to_owned())]
    );

    // It merged: the next conflict is a new transition and a new Job.
    fx.store()
        .write("fixture", |tx| {
            Ok(tx.execute_batch(
                "UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE key = 'up-repo';",
            )?)
        })
        .unwrap();
    fx.polled("UNKNOWN", "UNKNOWN");
    fx.polled("CONFLICTING", "DIRTY");
    assert_eq!(
        fx.update_jobs(),
        [
            ("up-repo".to_owned(), "merged".to_owned()),
            ("up-repo-2".to_owned(), "queued".to_owned())
        ]
    );
}

#[test]
fn a_conflicting_pull_request_with_failed_checks_is_told_only_the_checks() {
    let fx = poll_fx("deploy");
    fx.search(&[merge_state(
        pay("OPEN", None, Some("FAILURE")),
        "CONFLICTING",
        "DIRTY",
    )]);

    fx.poll();

    let events = fx.events("pr-feedback");
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].1,
        feedback_merge(
            "open",
            "failed",
            "none",
            "conflicting",
            "dirty",
            &["checks_failed"]
        )
    );
    assert!(
        events[0].0.contains("needs attention: checks failed. Read"),
        "{}",
        events[0].0
    );
}

#[test]
fn a_conflicting_pull_request_no_plan_owns_gets_no_event() {
    let fx = poll_fx("deploy");
    fx.search(&[
        pay("OPEN", None, None),
        merge_state(
            node("acme/web", 902, "OPEN", None, None),
            "CONFLICTING",
            "DIRTY",
        ),
    ]);
    let events = fx.event_count();

    let out = fx.poll();

    assert_eq!(out["feedback"], json!([]));
    assert_eq!(out["changed"][0]["mergeable"], "conflicting");
    assert_eq!(fx.event_count(), events);
    let stored: (String, String) = fx
        .store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT mergeable, merge_state FROM pull_requests WHERE number = 902",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    assert_eq!(stored, ("conflicting".to_owned(), "dirty".to_owned()));
}

#[test]
fn poll_tells_the_plan_once_that_changes_were_requested() {
    let fx = poll_fx("deploy");
    fx.search(&[pay("OPEN", Some("CHANGES_REQUESTED"), None)]);

    assert_eq!(
        fx.poll()["feedback"],
        json!([{"plan": "pay", "pr": "repo"}])
    );
    let events = fx.events("pr-feedback");
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].1,
        feedback("open", "none", "changes_requested", &["changes_requested"])
    );
    assert!(
        events[0]
            .0
            .contains("needs attention: changes requested. Read"),
        "{}",
        events[0].0
    );

    let txns = fx.txns();
    assert_eq!(fx.poll()["feedback"], json!([]));
    assert_eq!((fx.txns(), fx.events("pr-feedback").len()), (txns, 1));

    // Approved, which tells nobody, then changes requested again.
    fx.search(&[pay("OPEN", Some("APPROVED"), None)]);
    assert_eq!(fx.poll()["feedback"], json!([]));
    fx.search(&[pay("OPEN", Some("CHANGES_REQUESTED"), None)]);
    assert_eq!(
        fx.poll()["feedback"],
        json!([{"plan": "pay", "pr": "repo"}])
    );
    assert_eq!(fx.events("pr-feedback").len(), 2);
}

#[test]
fn poll_maps_gh_values_to_the_stored_ones() {
    let fx = poll_fx("deploy");
    let cases = [
        // (reviewDecision, rollup state, stored decision, stored checks)
        (Some("APPROVED"), None, "approved", "none"),
        (
            Some("CHANGES_REQUESTED"),
            Some("SUCCESS"),
            "changes_requested",
            "passed",
        ),
        (Some("REVIEW_REQUIRED"), Some("PENDING"), "none", "pending"),
        (Some(""), Some("EXPECTED"), "none", "pending"),
        (None, Some("FAILURE"), "none", "failed"),
        (Some("APPROVED"), Some("ERROR"), "approved", "failed"),
    ];
    for (review, rollup, want_review, want_checks) in cases {
        fx.search(&[pay("OPEN", review, rollup)]);
        fx.poll();
        assert_eq!(
            fx.pr_data(),
            (
                "Pay the bills".into(),
                want_review.into(),
                want_checks.into(),
                "open".into()
            ),
            "{review:?} {rollup:?}"
        );
    }
}

#[test]
fn a_draft_is_stored_and_printed() {
    let fx = poll_fx("deploy");
    fx.search(&[pay("OPEN", None, None).replace(r#""isDraft":false"#, r#""isDraft":true"#)]);
    let out = fx.poll();
    assert_eq!(out["changed"][0]["draft"], true);
    assert_eq!(out["workflows"][0]["pull_request"]["draft"], true);
}

impl Poll {
    /// `(id, state, authored, review_requested, merged_at)` of the row of `pull_requests` for pull request `number`.
    fn pull_request_row(&self, number: i64) -> Option<(i64, String, bool, bool, Option<String>)> {
        self.store()
            .read(|c| {
                Ok(rusqlite::OptionalExtension::optional(c.query_row(
                    "SELECT id, state, authored, review_requested, merged_at FROM pull_requests WHERE number = ?1",
                    [number],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                ))?)
            })
            .unwrap()
    }

    /// How many rows of `history` are `op` writes of `pull_requests` (of pull request `number` when given).
    fn pull_request_history(&self, op: &str, number: Option<i64>) -> i64 {
        self.store()
            .read(|c| {
                Ok(c.query_row(
                    "SELECT count(*) FROM history WHERE tbl = 'pull_requests' AND op = ?1
                        AND (?2 IS NULL OR json_extract(coalesce(after, before), '$.number') = ?2)",
                    params![op, number],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    }

    /// `(repo, number)` of the open pull requests the User authored, oldest first: the rows that become the Inbox's `pr-ready` items.
    fn widget_prs(&self) -> Vec<(String, i64)> {
        self.store()
            .read(|c| {
                let mut stmt = c.prepare("SELECT repo, number FROM pull_requests WHERE state = 'open' AND authored = 1 ORDER BY created_at, repo, number")?;
                let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .unwrap()
    }

    /// Adds a stored pull request `number` of `acme/<repo>`, no PR points to it.
    fn add_row(&self, repo: &str, number: i64, state: &str, authored: bool, requested: bool) {
        let sql = format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at, authored, review_requested)
               VALUES ('acme/{repo}', {number}, 'https://github.com/acme/{repo}/pull/{number}', 'Pay the bills', 0, '{state}', 'none', 'none', 'opened', NULL, 'opened', {}, {});",
            authored as i32, requested as i32
        );
        self.store()
            .write("fixture", |tx| Ok(tx.execute_batch(&sql)?))
            .unwrap();
    }

    /// Marks the comments of pull request `number` as read at `updatedAt` `U1` (what [`node`] lists) with none open:
    /// the poll then makes no comments read for it (pr-comments).
    fn mark_read(&self, number: i64) {
        let sql = format!(
            "UPDATE pull_requests SET updated_at = 'U1', feedback = '[]' WHERE number = {number}"
        );
        self.store()
            .write("fixture", |tx| Ok(tx.execute_batch(&sql)?))
            .unwrap();
    }

    /// What the fake `gh` is asked for each call of the lookup: the text of its query.
    fn lookup_texts(&self) -> Vec<String> {
        self.gh_calls()
            .into_iter()
            .filter(|c| is_lookup(c))
            .map(|c| c[3].clone())
            .collect()
    }
}

#[test]
fn a_flag_a_search_misses_clears_only_when_the_direct_read_says_so() {
    let fx = poll_fx("dispatch");
    let a = node("acme/pay", 901, "OPEN", None, None);
    fx.search_both(std::slice::from_ref(&a), std::slice::from_ref(&a));
    fx.poll_all();
    assert_eq!(fx.memberships(), [("acme/pay".to_owned(), 901, true, true)]);
    let id = fx
        .pull_request_row(901)
        .expect("stored by the first poll")
        .0;
    let numbers = |out: &Value| -> Vec<i64> {
        out["changed"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["number"].as_i64().unwrap())
            .collect()
    };

    // Poll A: no search lists it; the read says the viewer wrote it and no request is left.
    fx.search_both(&[], &[]);
    fx.lookup(&[Some(viewer(a.clone(), true, false))]);
    let out = fx.poll_all();
    assert_eq!(
        fx.memberships(),
        [("acme/pay".to_owned(), 901, true, false)]
    );
    assert_eq!(numbers(&out), [901]);
    assert_eq!(fx.pull_request_row(901).map(|r| r.0), Some(id));
    assert_eq!(fx.widget_prs(), [("acme/pay".to_owned(), 901)]);

    // Poll B: the viewer did not write it (another gh account) and no request: both flags clear, the row stays.
    fx.lookup(&[Some(viewer(a.clone(), false, false))]);
    fx.poll_all();
    assert_eq!(
        fx.memberships(),
        [("acme/pay".to_owned(), 901, false, false)]
    );
    assert_eq!(
        fx.pull_request_row(901),
        Some((id, "open".to_owned(), false, false, None))
    );
    assert_eq!(fx.widget_prs(), []);
    assert_eq!(fx.pull_request_history("delete", None), 0);

    // Poll C: no flag and no owner: not a candidate, no lookup, one call, no write.
    let (calls, txns) = (fx.gh_calls().len(), fx.txns());
    let out = fx.poll_all();
    assert_eq!(out["changed"], json!([]));
    assert_eq!(fx.gh_calls().len(), calls + 1, "one call: the search");
    assert_eq!(fx.txns(), txns, "no write transaction");
    assert_eq!(
        fx.memberships(),
        [("acme/pay".to_owned(), 901, false, false)]
    );

    // Poll D: the author search lists it again.
    fx.search_both(&[a], &[]);
    fx.poll_all();
    assert_eq!(
        fx.memberships(),
        [("acme/pay".to_owned(), 901, true, false)]
    );
    assert_eq!(fx.widget_prs(), [("acme/pay".to_owned(), 901)]);
}

#[test]
fn a_direct_read_that_says_merged_or_closed_moves_an_unowned_row_and_keeps_it() {
    let fx = poll_fx("dispatch");
    let (a, b) = (
        node("acme/pay", 901, "OPEN", None, None),
        node("acme/web", 902, "OPEN", None, None),
    );
    fx.search(&[a.clone(), b.clone()]);
    fx.poll_all();
    let ids = (
        fx.pull_request_row(901).unwrap().0,
        fx.pull_request_row(902).unwrap().0,
    );

    fx.search(&[]);
    fx.lookup(&[
        Some(node("acme/pay", 901, "MERGED", None, None)),
        Some(node("acme/web", 902, "CLOSED", None, None)),
    ]);
    let out = fx.poll_all();

    let mut changed: Vec<(i64, String)> = out["changed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["number"].as_i64().unwrap(),
                c["state"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    changed.sort();
    assert_eq!(
        changed,
        [(901, "merged".to_owned()), (902, "closed".to_owned())],
        "{out}"
    );
    let (a_row, b_row) = (
        fx.pull_request_row(901).expect("901 is kept"),
        fx.pull_request_row(902).expect("902 is kept"),
    );
    assert_eq!(
        (a_row.0, a_row.1.as_str(), a_row.2, a_row.3),
        (ids.0, "merged", false, false)
    );
    assert_eq!(a_row.4.as_deref(), Some("2026-10-14T10:00:00Z"));
    assert_eq!(b_row, (ids.1, "closed".to_owned(), false, false, None));
    assert_eq!(fx.widget_prs(), []);
    assert_eq!(fx.pull_request_history("delete", None), 0);

    // A lagging search lists 901 as open: merged is final, and a merged row is not read again.
    let (txns, calls) = (fx.txns(), fx.gh_calls().len());
    fx.search(&[a]);
    let out = fx.poll_all();
    assert_eq!(out["changed"], json!([]));
    assert_eq!(fx.pull_request_row(901), Some(a_row));
    assert_eq!(fx.txns(), txns);
    assert_eq!(fx.gh_calls().len(), calls + 1, "one call: the search");
}

#[test]
fn the_direct_read_is_one_batched_call_and_only_when_a_stored_open_pr_is_missing() {
    let fx = poll_fx("deploy");
    fx.mark_read(812);
    let (p812, p901, p902, p903) = (
        pay("OPEN", None, None),
        node("acme/pay", 901, "OPEN", None, None),
        node("acme/web", 902, "OPEN", None, None),
        node("acme/shop", 903, "OPEN", None, None),
    );
    // 812 is owned; 901 and 902 are authored; 903 is review-requested only.
    fx.search_both(
        &[p812.clone(), p901.clone(), p902.clone()],
        std::slice::from_ref(&p903),
    );
    fx.poll_all();
    assert_eq!(
        fx.memberships(),
        [
            ("acme/pay".to_owned(), 812, true, false),
            ("acme/pay".to_owned(), 901, true, false),
            ("acme/web".to_owned(), 902, true, false),
            ("acme/shop".to_owned(), 903, false, true)
        ]
    );
    assert_eq!(fx.gh_calls().len(), 1);
    // Every flag listed: exactly one call, a search.
    fx.poll_all();
    assert_eq!(fx.gh_calls().len(), 2);
    assert!(fx.lookup_texts().is_empty());

    // The searches list 812 only: 901, 902 (a) and 903 (c) are read, in id order, in one call; 812 is not.
    fx.search_both(std::slice::from_ref(&p812), &[]);
    fx.lookup(&[
        Some(viewer(p901.clone(), true, false)),
        Some(viewer(p902.clone(), true, false)),
        Some(viewer(p903.clone(), false, true)),
    ]);
    fx.poll_all();
    let calls = fx.gh_calls();
    assert_eq!(
        calls.len(),
        4,
        "that poll made a search and one lookup: {calls:?}"
    );
    assert!(is_search(&calls[2]) && is_lookup(&calls[3]), "{calls:?}");
    let text = &calls[3][3];
    for (i, (repo, name, number)) in [
        ("acme", "pay", 901),
        ("acme", "web", 902),
        ("acme", "shop", 903),
    ]
    .into_iter()
    .enumerate()
    {
        let want = format!(
            r#"p{i}: repository(owner: "{repo}", name: "{name}") {{ pullRequest(number: {number}) {{"#
        );
        assert!(text.contains(&want), "{want} in {text}");
    }
    assert!(
        !text.contains("p3:") && !text.contains("number: 812"),
        "{text}"
    );
    assert_eq!(
        text.matches(
            "viewerDidAuthor viewerLatestReviewRequest { id } repository { isArchived } }"
        )
        .count(),
        3,
        "{text}"
    );
    assert_eq!(
        fx.memberships()[1..],
        [
            ("acme/pay".to_owned(), 901, true, false),
            ("acme/web".to_owned(), 902, true, false),
            ("acme/shop".to_owned(), 903, false, true)
        ]
    );

    // A stored closed row and a stored open row with no flag and no owner are never named.
    fx.add_row("pay", 904, "closed", true, false);
    fx.add_row("pay", 905, "open", false, false);
    fx.poll_all();
    let texts = fx.lookup_texts();
    assert_eq!(texts.len(), 2, "the same three are read again");
    assert!(
        !texts[1].contains("number: 904")
            && !texts[1].contains("number: 905")
            && !texts[1].contains("p3:"),
        "{}",
        texts[1]
    );

    // 903 is listed by the author search only while its stored flag is `review_requested`: (c) makes it the only candidate.
    fx.search_both(&[p812, p901, p902, p903.clone()], &[]);
    fx.lookup(&[Some(viewer(p903, true, true))]);
    fx.poll_all();
    let texts = fx.lookup_texts();
    assert_eq!(texts.len(), 3);
    assert!(
        texts[2]
            .contains("p0: repository(owner: \"acme\", name: \"shop\") { pullRequest(number: 903)")
            && !texts[2].contains("p1:"),
        "{}",
        texts[2]
    );
}

#[test]
fn a_failed_direct_read_fails_the_poll_and_writes_nothing() {
    let fx = poll_fx("dispatch");
    fx.search(&[node("acme/pay", 901, "OPEN", None, None)]);
    fx.poll_all();
    let (txns, events, rows) = (fx.txns(), fx.event_count(), fx.memberships());

    fx.search(&[]);
    fx.gh
        .lookup
        .set(stderr_exit(1, "gh: HTTP 502: Bad Gateway"));
    let run = fx.cpd(&["pr", "poll"]);

    assert_eq!(
        run.exit, 1,
        "the poll reads 901 directly and fails: {run:?}"
    );
    assert_eq!(code(&run), ("gh_failed".into(), 1));
    let details = &run.error()["details"];
    assert_eq!(
        (&details["call"], &details["transient"]),
        (&json!("lookup"), &json!(true)),
        "{details}"
    );
    assert_eq!(
        (fx.txns(), fx.event_count(), fx.memberships()),
        (txns, events, rows)
    );
    assert_eq!(fx.pull_request_history("delete", None), 0);
    assert_eq!(fx.widget_prs(), [("acme/pay".to_owned(), 901)]);
}

#[test]
fn with_a_plan_the_output_adds_its_prs_and_their_rows() {
    let fx = poll_fx("deploy");
    fx.add_waiting_web();
    fx.search(&[pay("OPEN", None, Some("PENDING"))]);

    let without = fx.poll_all();
    assert!(without.get("workflows").is_none(), "{without}");
    let out = fx.poll();

    assert_eq!(out["polled"], 1);
    assert_eq!(out["workflows"].as_array().unwrap().len(), 2);
    assert_eq!(
        out["workflows"][0],
        json!({"key": "repo", "status": "open", "pull_request": fx.shown_pull_request()})
    );
    assert_eq!(out["workflows"][0]["pull_request"]["checks"], "pending");
    assert_eq!(
        out["workflows"][1],
        json!({"key": "web", "status": "waiting", "pull_request": null})
    );
}

#[test]
fn poll_has_no_phase_precondition_and_knows_its_plan() {
    let fx = poll_fx("plan");
    fx.search(&[]);
    let out = fx.poll();
    assert_eq!(
        out["workflows"],
        json!([{"key": "repo", "status": "waiting", "pull_request": null}])
    );

    let run = fx.cpd(&["pr", "poll", "--plan", "nope"]);
    assert_eq!(code(&run), ("plan_not_found".into(), 4));
    assert_eq!(
        fx.gh_calls().iter().filter(|c| is_search(c)).count(),
        1,
        "an unknown Plan polls nothing"
    );
}

#[test]
fn poll_has_no_pr_flag() {
    let fx = poll_fx("deploy");
    let run = fx.pr_cmd("poll", "repo");
    assert_eq!(code(&run), ("usage".into(), 2));
    assert!(fx.gh_calls().is_empty());
}

#[test]
fn a_second_poll_waits_for_the_lock() {
    let mut fx = poll_fx("deploy");
    fx.mark_read(812);
    fx.search(&[pay("OPEN", None, None)]);
    let path = fx.home.pr_poll_lock();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .unwrap();
    // Another poll's lock, a real `flock` on a file of this env: the fixture holds it, until the nap below or the end of the test.
    let held = Arc::new(Mutex::new(Some(
        Flock::lock(file, FlockArg::LockExclusiveNonblock)
            .map_err(|(_, e)| e)
            .unwrap(),
    )));

    // The poll waits on the clock: at each nap it has made no `gh` call. The first nap is the other poll finishing.
    let (runner, gh_calls_at_nap, release) = (
        fx.fakes.runner.clone(),
        Arc::new(Mutex::new(Vec::new())),
        held.clone(),
    );
    let seen = gh_calls_at_nap.clone();
    fx.fx.on_nap(move |_| {
        seen.lock().unwrap().push(
            runner
                .calls()
                .iter()
                .filter(|call| call.tool == "gh")
                .count(),
        );
        release.lock().unwrap().take();
    });

    let out = fx.cpd(&["pr", "poll"]).ok();

    assert_eq!(
        *gh_calls_at_nap.lock().unwrap(),
        [0],
        "one nap, with the lock held and no gh call made"
    );
    assert_eq!(out["polled"], 1);
    assert_eq!(fx.gh_calls().len(), 1);
    assert!(
        held.lock().unwrap().is_none(),
        "the lock was released at the nap"
    );
}

#[test]
fn poll_reports_a_failing_gh_and_changes_nothing() {
    let fx = poll_fx("deploy");
    let (txns, events) = (fx.txns(), fx.event_count());
    fx.fail("rate limited");

    let run = fx.pr("poll");

    assert_eq!(code(&run), ("gh_failed".into(), 1));
    let details = &run.error()["details"];
    assert!(
        details["stderr"].as_str().unwrap().contains("rate limited"),
        "{details}"
    );
    assert_eq!(details["exit"], 1);
    assert_eq!(
        details["argv"].as_array().unwrap()[..3],
        ["gh", "api", "graphql"]
    );
    assert_eq!((fx.txns(), fx.event_count()), (txns, events));
}

// ---------- `pr poll`: a `gh` HTTP 5xx or timeout is a transient `gh_failed` (contract §7.6 step 4, hs-d354) ----------

/// The call of the poll a failure is scripted on.
#[derive(Clone, Copy)]
enum On {
    /// The first call, the search of the User's own and review-requested pull requests.
    Search,
    /// The direct read of a stored open pull request no search lists (812, with the fixture's empty search).
    Lookup,
}

/// `pr poll` against a `gh` that answers `reply` to the call `on`; the `gh_failed` it exits 1 with, its `details`.
/// Nothing is written.
fn poll_failure(fx: &Poll, on: On, reply: Reply) -> Value {
    let (txns, events) = (fx.txns(), fx.event_count());
    match on {
        On::Search => fx.gh.search.set(reply.clone()),
        On::Lookup => fx.gh.lookup.set(reply.clone()),
    }
    let run = fx.cpd(&["pr", "poll", "--plan", "pay"]);
    assert_eq!(code(&run), ("gh_failed".into(), 1), "{reply:?}");
    assert_eq!((fx.txns(), fx.event_count()), (txns, events), "{reply:?}");
    run.error()["details"].clone()
}

/// `gh` exits `code` with `stderr` and a line end, and nothing on stdout.
fn stderr_exit(code: i32, stderr: &str) -> Reply {
    Reply::exit(code, "", &format!("{stderr}\n"))
}

const NGINX: &str = "<!DOCTYPE html><html><head><title>502 Bad Gateway</title></head><body><center><h1>502 Bad Gateway</h1></center><hr><center>nginx</center></body></html>";

#[test]
fn poll_says_an_http_5xx_of_the_search_is_transient() {
    let fx = poll_fx("deploy");
    for (reply, reason) in [
        (
            stderr_exit(
                1,
                "gh: HTTP 502: Bad Gateway (https://api.github.com/graphql)",
            ),
            "HTTP 502",
        ),
        // nginx's page with the status in the message: the same case.
        (
            Reply::exit(
                1,
                &format!("{NGINX}\n"),
                "gh: HTTP 502: Bad Gateway (https://api.github.com/graphql)\n",
            ),
            "HTTP 502",
        ),
        (
            stderr_exit(1, "gh: Service Unavailable (HTTP 503)"),
            "HTTP 503",
        ),
        // The status in stdout when stderr is empty.
        (
            Reply::exit(1, "gh: HTTP 504: Gateway Timeout\n", ""),
            "HTTP 504",
        ),
    ] {
        let details = poll_failure(&fx, On::Search, reply.clone());
        assert_eq!(
            (
                &details["transient"],
                &details["call"],
                &details["reason"],
                &details["exit"]
            ),
            (&json!(true), &json!("search"), &json!(reason), &json!(1)),
            "{reply:?}: {details}"
        );
        assert_eq!(
            details["argv"].as_array().unwrap()[..3],
            ["gh", "api", "graphql"],
            "{details}"
        );
    }
}

#[test]
fn poll_says_an_http_5xx_of_the_lookup_is_transient_and_other_lookup_failures_are_not() {
    let fx = poll_fx("deploy");
    let details = poll_failure(&fx, On::Lookup, stderr_exit(1, "gh: HTTP 502: Bad Gateway"));
    assert_eq!(
        (&details["transient"], &details["call"], &details["reason"]),
        (&json!(true), &json!("lookup"), &json!("HTTP 502")),
        "{details}"
    );
    for reply in [
        stderr_exit(1, "gh: HTTP 401: Bad credentials"),
        Reply::exit(1, "{\"errors\":[{\"message\":\"Bad credentials\"}]}\n", ""),
    ] {
        let details = poll_failure(&fx, On::Lookup, reply.clone());
        assert_eq!(
            (&details["transient"], &details["call"], &details["reason"]),
            (&json!(false), &json!("lookup"), &Value::Null),
            "{reply:?}: {details}"
        );
    }
}

/// `gh`'s own failure to parse a response that ended early (hs-d354, E5 at 2026-10-20 16:43:13Z): exit 1, this on stderr, nothing on stdout.
fn gh_cut() -> Reply {
    stderr_exit(1, "unexpected end of JSON input")
}

#[test]
fn poll_says_a_cut_response_of_the_search_is_transient() {
    let fx = poll_fx("deploy");
    for reply in [
        gh_cut(),
        // A body that stopped mid-way holds no complete JSON.
        Reply::exit(
            1,
            "{\"data\":{\"search\":{\"issueCount\":1,\"nodes\":[{\"repository\":\n",
            "unexpected end of JSON input\n",
        ),
    ] {
        let details = poll_failure(&fx, On::Search, reply.clone());
        assert_eq!(
            (
                &details["transient"],
                &details["call"],
                &details["reason"],
                &details["exit"]
            ),
            (
                &json!(true),
                &json!("search"),
                &json!("unexpected end of JSON input"),
                &json!(1)
            ),
            "{reply:?}: {details}"
        );
        assert!(
            details["stderr"]
                .as_str()
                .unwrap()
                .contains("unexpected end of JSON input"),
            "{details}"
        );
    }
}

#[test]
fn poll_says_a_cut_response_of_the_lookup_is_transient() {
    let fx = poll_fx("deploy");
    let details = poll_failure(&fx, On::Lookup, gh_cut());
    assert_eq!(
        (&details["transient"], &details["call"], &details["reason"]),
        (
            &json!(true),
            &json!("lookup"),
            &json!("unexpected end of JSON input")
        ),
        "{details}"
    );
}

#[test]
fn poll_says_other_json_failures_of_gh_are_not_the_cut_response() {
    let fx = poll_fx("deploy");
    for reply in [
        // A body that arrived whole and is wrong: other `gh` parse messages.
        stderr_exit(1, "invalid character '<' looking for beginning of value"),
        stderr_exit(
            1,
            "json: cannot unmarshal string into Go value of type map[string]interface {}",
        ),
        // `gh` exit 0 with an empty body is the poll's own check, never transient, whatever stderr says.
        stderr_exit(0, "unexpected end of JSON input"),
    ] {
        let details = poll_failure(&fx, On::Search, reply.clone());
        assert_eq!(
            (&details["transient"], &details["call"], &details["reason"]),
            (&json!(false), &json!("search"), &Value::Null),
            "{reply:?}: {details}"
        );
    }
}

#[test]
fn poll_says_every_other_gh_failure_is_not_transient() {
    let fx = poll_fx("deploy");
    for reply in [
        stderr_exit(1, "gh: HTTP 401: Bad credentials"),
        stderr_exit(1, "gh: HTTP 403: Resource not accessible"),
        stderr_exit(1, "gh: HTTP 404: Not Found"),
        stderr_exit(1, "gh: HTTP 429: Too Many Requests"),
        // An HTML page with no 5xx status in the message is not transient, whatever the exit.
        Reply::exit(1, &format!("{NGINX}\n"), "gh: exit status 1\n"),
        Reply::ok(&format!("{NGINX}\n")),
        // A 2xx answer with a bad or empty body.
        Reply::ok(""),
        Reply::ok("not json\n"),
        // GraphQL's own `errors`, a RATE_LIMITED among them.
        Reply::exit(
            1,
            "{\"errors\":[{\"type\":\"RATE_LIMITED\",\"message\":\"API rate limit exceeded\"}]}\n",
            "gh: API rate limit exceeded\n",
        ),
        // `HTTP 502` in a body that is a 2xx answer is not a status.
        Reply::ok("{\"data\":null,\"errors\":[{\"message\":\"upstream said HTTP 502\"}]}\n"),
    ] {
        let details = poll_failure(&fx, On::Search, reply.clone());
        assert_eq!(
            (&details["transient"], &details["call"], &details["reason"]),
            (&json!(false), &json!("search"), &Value::Null),
            "{reply:?}: {details}"
        );
        assert!(
            details["argv"].is_array()
                && details.get("exit").is_some()
                && details.get("stderr").is_some(),
            "{details}"
        );
    }
}

#[test]
fn poll_refuses_a_gh_answer_that_is_not_an_answer_and_writes_nothing() {
    let fx = poll_fx("deploy");
    let (txns, events) = (fx.txns(), fx.event_count());
    let failing = pay("OPEN", None, Some("FAILURE"));
    for (name, body) in [
        ("empty", String::new()),
        ("not JSON", "<html>Unicorn!</html>".to_owned()),
        // The nodes are there, but an `errors` makes the search no answer: never "no PRs", never partial.
        (
            "errors",
            search_answer(&[failing], 4987).replace("}}}", r#"}},"errors":[{"message":"boom"}]}"#),
        ),
    ] {
        fx.search_raw(&body);
        let run = fx.pr("poll");
        assert_eq!(code(&run), ("gh_failed".into(), 1), "{name}");
    }
    // The search answers; the lookup has no `data`: the whole poll fails and the search's changes are not written.
    fx.search(&[node("acme/web", 902, "OPEN", None, None)]);
    fx.lookup_raw(r#"{"errors":[{"message":"Bad credentials"}]}"#);
    assert_eq!(code(&fx.pr("poll")), ("gh_failed".into(), 1));
    assert_eq!(
        (fx.txns(), fx.event_count(), fx.pull_requests().len()),
        (txns, events, 1)
    );
}

#[test]
fn poll_of_the_last_merged_pull_request_supersedes_the_pending_reviews() {
    let fx = poll_fx("deploy");
    kept_attempt(&fx);
    fx.search(&[]);
    fx.lookup(&[Some(
        pay("MERGED", Some("APPROVED"), Some("SUCCESS")).replace("Pay the bills", TITLE),
    )]);
    insert_pending_reviews(&fx.env, "pay");

    let out = fx.poll();

    assert_eq!(out["merged"], json!([{"plan": "pay", "pr": "repo"}]));
    assert_eq!(
        (
            out["done"].clone(),
            out["feedback"].clone(),
            out["polled"].clone()
        ),
        (json!(["pay"]), json!([]), json!(0))
    );
    assert_reviews_superseded_before_done(&fx.env, "pay");
}

/// The Dispatch card: label, then (number, state, (checks, decision, title)) when a PR is open.
type DispatchCard = (
    String,
    Option<(i64, PrState, Option<(String, ReviewDecision, Checks)>)>,
);

/// The Dispatch column's read of the Plan: label, number, state, checks, decision, title.
fn dispatch_card(fx: &Poll) -> DispatchCard {
    let widgets = read::widgets(&mut fx.store(), 4, 10).unwrap();
    assert_eq!(widgets.plans.len(), 1);
    let card = widgets.plans.into_iter().next().unwrap();
    let pr = card.workflows.into_iter().next().map(|pr| {
        (
            pr.number,
            pr.state,
            pr.polled.map(|p| (p.title, p.decision, p.checks)),
        )
    });
    (card.label, pr)
}

#[test]
fn a_plan_without_a_pr_has_a_label_and_no_pr_row() {
    let fx = poll_fx("dispatch");
    assert_eq!(dispatch_card(&fx), ("Lead".into(), None));
}

/// Attempt `b-a1` of a cancelled Job `b`, ended, with its worktree and branch kept for
/// debugging (§3.4 A9), and a tmux window for it.
fn kept_attempt(fx: &Poll) -> (PathBuf, String) {
    let attempt = AttemptId::parse("b-a1").unwrap();
    let setup = Setup {
        timeout: Duration::from_secs(30),
        log: fx.env.root().join("attempt.log"),
    };
    let made = fx
        .scoped(|| create_attempt_worktree(&fx.repo, &fx.lead, &plan(), &attempt, &setup).unwrap());
    fx.store()
        .write("fixture", |tx| {
            tx.execute(
                "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, cancel_reason, cancel_requested_at, created_txn, created_at)
                 VALUES ('pay', 'repo', 'b', 'B', 'S2', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', 'cancelled', 0, ?1, 'no', 't', ?2, 't')",
                params!["f".repeat(64), tx.id()],
            )?;
            tx.execute(
                "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, branch, worktree, base_sha,
                                       started_at, ended_at)
                 VALUES ('pay', 'b-a1', 'b', 1, 'S2', 'ladder', 'ended', 'cancelled', 'cancel_requested', ?1, ?2, ?3, 't', 't')",
                params![made.branch, made.path.to_str().unwrap(), made.base_sha],
            )?;
            Ok(())
        })
        .unwrap();
    (made.path, made.branch)
}

const GONE_URL: &str = "https://github.com/gone/shop/pull/20";

impl Poll {
    /// A second Plan `bob` in Context `ctx2` of owner `gone`, its PR `shop` open as pull request 20 of `gone/shop`.
    /// The owner file of `gone` is written, then deleted: the one owner this machine has no file for.
    fn add_bob_without_an_owner_file(&self) {
        self.env.owner_policy("gone", "");
        fs::remove_file(self.env.cpd_home().join("policy/gone/owner.toml")).unwrap();
        self.store()
            .write("fixture", |tx| {
                tx.execute_batch(&format!(
                    "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx2', 'Ctx2', 'gone', 't', 't');
                     INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                       VALUES ('lead2', 'lead', 'ctx2', 'bob', 'Lead', '/', 'O3', 'idle', 't', 't');
                     INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
                       VALUES ('bob', 'ctx2', 'Bob', 'deploy', 'lead2', 't', 't');
                     INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at)
                       VALUES ('gone/shop', 20, '{GONE_URL}', 'Pay the bills', 0, 'open', 'none', 'none', 'opened', 'opened');
                     INSERT INTO workflows (plan, key, repo, base, name, status, lead_branch, lead_worktree, created_txn, created_at, started_at,
                                      pr_number, pr_url, pr_opened_at, pull_request)
                       VALUES ('bob', 'shop', '/r', 'main', 'shop', 'open', 'cpd/bob/shop', '/w', cpd_txn(), 't', 't',
                               20, '{GONE_URL}', 'opened', (SELECT id FROM pull_requests WHERE number = 20));"
                ))?;
                Ok(())
            })
            .unwrap();
    }

    /// `(plan, data)` of every `error` Event of source `pr poll` that is not a clean-up failure.
    fn policy_errors(&self) -> Vec<(String, Value)> {
        self.store()
            .read(|c| {
                let mut stmt = c.prepare(
                    "SELECT plan, data FROM events WHERE kind = 'error' AND json_extract(data, '$.source') = 'pr poll'
                        AND json_extract(data, '$.code') <> 'cleanup_failed' ORDER BY id",
                )?;
                Ok(stmt
                    .query_map([], |r| Ok((r.get::<_, String>(0)?, serde_json::from_str(&r.get::<_, String>(1)?).unwrap())))?
                    .collect::<rusqlite::Result<_>>()?)
            })
            .unwrap()
    }

    /// `session:window` of every window of the fake tmux server.
    fn window_names(&self) -> String {
        let out = self.tmux(&["list-windows", "-a", "-F", "#{session_name}:#{window_name}"]);
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

/// `deploy`, polled once with a search that lists nothing and a read that cannot find 812; the txns did not move.
fn poll_fx_owned_not_found() -> Poll {
    let fx = poll_fx("deploy");
    fx.search(&[]);
    fx.lookup(&[None]);
    let txns = fx.txns();
    fx.poll_all();
    assert_eq!(
        fx.txns(),
        txns,
        "an owned row GitHub cannot find is unchanged"
    );
    fx
}

#[test]
fn a_lookup_with_a_not_found_alias_and_exit_1_still_applies_the_others() {
    let fx = poll_fx("deploy");
    fx.add_open_web();
    fx.search(&[]);
    // `gh` exits 1 when one alias is NOT_FOUND although `data` holds the others.
    fx.lookup(&[Some(pay("MERGED", None, None)), None]);
    fx.gh.lookup.exits(1);

    let out = fx.poll();

    assert_eq!(out["merged"], json!([{"plan": "pay", "pr": "repo"}]));
    assert_eq!(out["done"], json!([]));
    assert_eq!(
        (fx.workflow_status("repo"), fx.workflow_status("web")),
        ("merged".to_owned(), "open".to_owned())
    );
    assert_eq!(fx.phase(), "deploy");
    let calls = fx.gh_calls();
    assert!(
        calls[1][3].contains(r#"p0: repository(owner: "acme", name: "pay")"#)
            && calls[1][3].contains(r#"p1: repository(owner: "acme", name: "web")"#),
        "{}",
        calls[1][3]
    );
    assert_eq!(out["workflows"][1]["pull_request"]["state"], "open");
}

#[test]
fn a_merged_pr_makes_its_dependent_startable_but_poll_does_not_start_it() {
    let fx = poll_fx("dispatch");
    fx.add_waiting_web();
    // `pr open` only queues the PR's opener Job now: the open pull request is written straight, as the Lead's opener leaves it.
    fx.mark_opened("repo", 812);
    assert_eq!(
        code(&fx.cpd(&["workflow", "start", "web", "--plan", "pay"])),
        ("workflow_not_ready".into(), 3)
    );

    fx.search(&[]);
    fx.lookup(&[Some(pay("MERGED", None, None))]);
    let out = fx.poll();

    assert_eq!(out["merged"], json!([{"plan": "pay", "pr": "repo"}]));
    assert_eq!(
        (
            fx.workflow_status("repo").as_str(),
            fx.workflow_status("web").as_str(),
            fx.phase().as_str()
        ),
        ("merged", "waiting", "dispatch")
    );
    assert_eq!(fx.events("pr-merged").len(), 1);
    assert!(fx.events("plan-done").is_empty());
    assert!(
        fx.events("workflow-started").is_empty(),
        "pr poll never starts a PR"
    );
    // Its need is merged now: it can start.
    let started = fx.cpd(&["workflow", "start", "web", "--plan", "pay"]).ok();
    assert_eq!(started["workflow"]["status"], "dispatch");
    assert_eq!(fx.workflow_status("web"), "dispatch");
}

#[test]
fn a_poll_reply_without_the_review_request_search_is_gh_failed_and_writes_nothing() {
    let fx = poll_fx("deploy");
    let (txns, events) = (fx.txns(), fx.event_count());
    fx.search_raw(
        &search_answer(&[pay("OPEN", None, Some("FAILURE"))], 4987)
            .replace(r#""reviewRequests":{"issueCount":0,"nodes":[]},"#, ""),
    );

    let run = fx.pr("poll");

    assert_eq!(code(&run), ("gh_failed".into(), 1));
    assert!(
        run.error()["message"]
            .as_str()
            .unwrap()
            .contains("reviewRequests"),
        "{}",
        run.error()
    );
    assert_eq!((fx.txns(), fx.event_count()), (txns, events));
}

#[test]
fn a_pr_opened_by_cpd_is_authored_and_a_row_a_pr_points_to_stays_while_in_no_search() {
    let fx = poll_fx("deploy");
    assert_eq!(
        fx.memberships(),
        [("acme/pay".to_owned(), 812, true, false)]
    );
    fx.search_both(&[], &[]);
    fx.lookup(&[Some(pay("OPEN", None, None))]);
    fx.poll();
    assert_eq!(
        fx.memberships(),
        [("acme/pay".to_owned(), 812, true, false)],
        "a lookup answering open changes nothing"
    );
}

#[test]
fn a_pr_whose_owner_file_is_gone_is_reported_and_the_other_plans_pr_still_merges() {
    let fx = poll_fx("deploy");
    fx.add_bob_without_an_owner_file();
    // bob's checks turned failed (it would be told, were its policy known); pay's PR merged, so it is looked up.
    fx.search(&[node("gone/shop", 20, "OPEN", None, Some("FAILURE"))]);
    fx.lookup(&[Some(pay("MERGED", Some("APPROVED"), Some("SUCCESS")))]);

    let out = fx.poll_all();

    assert_eq!(out["merged"], json!([{"plan": "pay", "pr": "repo"}]));
    assert_eq!(
        (out["done"].clone(), out["feedback"].clone()),
        (json!(["pay"]), json!([]))
    );
    assert_eq!(
        (fx.plan_row().0, fx.workflow_status("repo")),
        ("done".to_owned(), "merged".to_owned())
    );
    assert_eq!(fx.events("pr-merged").len(), 1);
    // bob's row is written as usual, and bob is told nothing.
    let bob: String = fx
        .store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT checks FROM pull_requests WHERE number = 20",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(bob, "failed");
    let told: i64 = fx
        .store()
        .read(|c| {
            Ok(c.query_row(
                "SELECT count(*) FROM events WHERE kind = 'pr-feedback'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(told, 0);
    // One `error` Event names the plan, the PR and the code.
    let errors = fx.policy_errors();
    assert_eq!(errors.len(), 1, "{errors:?}");
    assert_eq!(errors[0].0, "bob");
    assert_eq!(errors[0].1["code"], "owner_unconfirmed");
    assert_eq!(errors[0].1["details"]["plan"], "bob");
    assert_eq!(errors[0].1["details"]["pr"], "shop");
    assert_eq!(errors[0].1["details"]["owner"], "gone");
    assert!(
        errors[0].1["message"]
            .as_str()
            .unwrap()
            .contains("Workflow shop (#20) of Plan bob"),
        "{}",
        errors[0].1["message"]
    );

    // Once per episode: the next polls, still failing the same way, append nothing and write nothing.
    let events = fx.event_count();
    fx.poll_all();
    fx.poll_all();
    assert_eq!((fx.policy_errors().len(), fx.event_count()), (1, events));
}

#[test]
fn a_pull_request_closed_without_a_merge_is_not_looked_up_again() {
    let fx = poll_fx("deploy");
    fx.search(&[]);
    fx.lookup(&[Some(pay("CLOSED", None, None))]);
    fx.poll();
    assert_eq!(fx.pr_data().3, "closed");
    let lookups = || fx.gh_calls().iter().filter(|c| is_lookup(c)).count();
    assert_eq!(lookups(), 1);

    for _ in 0..3 {
        fx.poll();
    }

    assert_eq!(lookups(), 1, "three more ticks looked it up again");
    assert_eq!(
        (fx.plan_row().0, fx.workflow_status("repo")),
        ("deploy".to_owned(), "open".to_owned())
    );
}

#[test]
fn a_pull_request_github_cannot_find_loses_its_flags_and_is_not_read_again() {
    let fx = poll_fx("dispatch");
    fx.search(&[node("acme/pay", 901, "OPEN", None, None)]);
    fx.poll_all();
    let id = fx
        .pull_request_row(901)
        .expect("stored by the first poll")
        .0;

    fx.search(&[]);
    fx.lookup(&[None]);
    fx.poll_all();

    assert_eq!(
        fx.pull_request_row(901),
        Some((id, "open".to_owned(), false, false, None))
    );
    assert_eq!(fx.widget_prs(), []);
    assert_eq!(fx.pull_request_history("delete", None), 0);
    let (calls, txns) = (fx.gh_calls().len(), fx.txns());
    fx.poll_all();
    assert_eq!(fx.gh_calls().len(), calls + 1, "one call: the search");
    assert_eq!(fx.txns(), txns);

    // An owned row GitHub cannot find is unchanged. One `Fx` at a time in a process: the first is dropped.
    drop(fx);
    let fx = poll_fx_owned_not_found();
    assert_eq!(
        fx.memberships(),
        [("acme/pay".to_owned(), 812, true, false)]
    );
    assert_eq!(fx.pr_data().3, "open");
}

#[test]
fn a_pull_request_in_both_searches_is_authored_and_review_requested_and_each_flag_clears_only_when_a_read_says_so()
 {
    let fx = poll_fx("dispatch");
    let mine = node("acme/pay", 901, "OPEN", None, None);
    fx.search_both(std::slice::from_ref(&mine), std::slice::from_ref(&mine));
    let out = fx.poll_all();
    assert_eq!(
        (
            out["polled"].clone(),
            out["changed"].as_array().unwrap().len()
        ),
        (json!(1), 1)
    );
    assert_eq!(fx.memberships(), [("acme/pay".to_owned(), 901, true, true)]);

    // Left the author search only: the read says the viewer did not write it, so `authored` clears; the row is reported as changed.
    fx.search_both(&[], std::slice::from_ref(&mine));
    fx.lookup(&[Some(viewer(mine.clone(), false, true))]);
    let out = fx.poll_all();
    assert_eq!(out["changed"].as_array().unwrap().len(), 1);
    assert!(out.get("removed").is_none(), "nothing is removed: {out}");
    assert_eq!(
        fx.memberships(),
        [("acme/pay".to_owned(), 901, false, true)]
    );

    // Back in the author search only: the read shows no request, so `review_requested` clears.
    fx.search_both(std::slice::from_ref(&mine), &[]);
    fx.lookup(&[Some(viewer(mine.clone(), true, false))]);
    fx.poll_all();
    assert_eq!(
        fx.memberships(),
        [("acme/pay".to_owned(), 901, true, false)]
    );

    // In neither: the read answers, both flags clear and the row is kept, never deleted.
    fx.search_both(&[], &[]);
    fx.lookup(&[Some(viewer(mine.clone(), false, false))]);
    let out = fx.poll_all();
    assert!(out.get("removed").is_none(), "nothing is removed: {out}");
    assert_eq!(
        fx.memberships(),
        [("acme/pay".to_owned(), 901, false, false)]
    );
}

#[test]
fn a_pull_request_one_search_misses_keeps_its_row_and_the_open_list() {
    let fx = poll_fx("dispatch");
    let (a, b) = (
        node("acme/pay", 901, "OPEN", None, None),
        node("acme/web", 902, "OPEN", None, None),
    );
    fx.search(&[a.clone(), b.clone()]);
    fx.poll_all();
    let id = fx
        .pull_request_row(901)
        .expect("stored by the first poll")
        .0;
    let (txns, events) = (fx.txns(), fx.event_count());
    let list = fx.widget_prs();
    assert_eq!(
        list,
        [("acme/pay".to_owned(), 901), ("acme/web".to_owned(), 902)]
    );
    assert_eq!(fx.gh_calls().len(), 1);

    let check = |when: &str| {
        assert_eq!(
            fx.pull_request_row(901),
            Some((id, "open".to_owned(), true, false, None)),
            "{when}"
        );
        assert_eq!(
            (fx.txns(), fx.event_count()),
            (txns, events),
            "{when}: nothing written, no Event"
        );
        assert_eq!(
            fx.pull_request_history("delete", None),
            0,
            "{when}: no row is ever deleted"
        );
        assert_eq!(
            fx.pull_request_history("insert", Some(901)),
            1,
            "{when}: 901 was inserted once"
        );
        assert_eq!(
            fx.widget_prs(),
            list,
            "{when}: the widget's list does not flicker"
        );
    };

    // Poll 2: one search misses 901; the direct read says it is open, and the viewer's.
    fx.search(std::slice::from_ref(&b));
    fx.lookup(&[Some(viewer(a.clone(), true, false))]);
    let out = fx.poll_all();
    assert_eq!(out["changed"], json!([]));
    check("poll 2");
    let calls = fx.gh_calls();
    assert_eq!(
        calls.len(),
        3,
        "poll 2 made a search and a lookup: {calls:?}"
    );
    assert!(is_search(&calls[1]) && is_lookup(&calls[2]), "{calls:?}");
    assert!(
        calls[2][3].contains("number: 901") && !calls[2][3].contains("number: 902"),
        "{}",
        calls[2][3]
    );

    // Poll 3: the search lists both again.
    fx.search(&[a, b]);
    let out = fx.poll_all();
    assert_eq!(out["changed"], json!([]));
    check("poll 3");
    assert_eq!(fx.gh_calls().len(), 4, "poll 3 made one call");
}

#[test]
fn a_reopened_plan_whose_new_pr_merges_is_done_again_with_a_second_plan_done() {
    let fx = poll_fx("deploy");
    fx.search(&[]);
    fx.lookup(&[Some(pay("MERGED", None, None))]);
    assert_eq!(fx.poll()["done"], json!(["pay"]));
    assert_eq!(fx.phase(), "done");
    assert_eq!(fx.events("plan-done").len(), 1);

    // `pr add` moves the legacy branch and reopens the Plan.
    // The PR is stacked on an unmerged PR, `side`, so that it waits: a PR that starts at once is refused its stub drafts after the lock
    // (`draft_parked`). `side` is cancelled once the PR is added, so that it does not keep the Plan from `done`.
    fx.env.sql(&format!(
        "INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at)
           VALUES ('pay', 'side', '{}', 'pay', 'main', 'dispatch', 'cpd/pay/side', '/w/side', 'Side', 'config', cpd_txn(), 't', 't');",
        fx.repo.display()
    ));
    let spec = json!({"goal": "Do it.", "context": "Because.", "touches": ["src/a.rs"], "acceptance": ["cargo test"], "out_of_scope": ["Everything else."]});
    let jobs = fx.env.root().join("fix-jobs.json");
    fs::write(
        &jobs,
        json!({"config": {"tier": "S2", "spec": spec}}).to_string(),
    )
    .unwrap();
    fx.install_catalog();
    fx.cpd(&[
        "workflow",
        "add",
        "fix",
        "--repo",
        fx.repo.to_str().unwrap(),
        "--title",
        "Change the PR",
        "--shape",
        "config",
        "--jobs",
        jobs.to_str().unwrap(),
        "--base",
        "pay/side",
        "--plan",
        "pay",
    ])
    .ok();
    fx.env.sql("UPDATE workflows SET status = 'cancelled', lead_worktree = NULL, started_at = NULL WHERE plan = 'pay' AND key = 'side';");
    assert_eq!(fx.phase(), "dispatch");
    // The engine would start the PR and the Lead open it: written straight, as the other tests do,
    // with the pull request's row `pr open` writes.
    fx.store()
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "UPDATE workflows SET status = 'dispatch', lead_worktree = '{lead}', started_at = 't' WHERE plan = 'pay' AND key = 'fix';
                 UPDATE plans SET phase = 'deploy' WHERE id = 'pay';
                 UPDATE jobs SET status = 'cancelled', cancel_reason = 'fixture: the PR is merged', cancel_requested_at = 't' WHERE plan = 'pay' AND status NOT IN ('merged', 'cancelled');",
                lead = fx.lead.display()
            ))?;
            Ok(())
        })
        .unwrap();
    fx.mark_opened("fix", 900);

    fx.lookup(&[Some(node("acme/pay", 900, "MERGED", None, None))]);
    let out = fx.poll();
    assert_eq!(
        (out["merged"].clone(), out["done"].clone()),
        (json!([{"plan": "pay", "pr": "fix"}]), json!(["pay"]))
    );
    assert_eq!(
        (fx.phase().as_str(), fx.workflow_status("fix").as_str()),
        ("done", "merged")
    );
    let done = fx.events("plan-done");
    assert_eq!(
        done.len(),
        2,
        "plan-done once each time the Plan reaches done"
    );
    let listed: Vec<&str> = done[1].1["workflows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["key"].as_str().unwrap())
        .collect();
    assert_eq!(listed, ["fix", "repo"]);
}

#[test]
fn a_review_request_is_stored_as_not_authored_and_kept_until_a_read_says_it_is_gone() {
    let fx = poll_fx("dispatch");
    fx.search_both(
        &[node("acme/pay", 901, "OPEN", None, None)],
        &[node(
            "acme/web",
            902,
            "OPEN",
            Some("APPROVED"),
            Some("FAILURE"),
        )],
    );
    let events = fx.event_count();

    let out = fx.poll_all();

    assert_eq!(
        (
            out["polled"].clone(),
            out["changed"].as_array().unwrap().len()
        ),
        (json!(2), 2)
    );
    assert!(out.get("removed").is_none(), "nothing is removed: {out}");
    assert_eq!(
        fx.memberships(),
        [
            ("acme/pay".to_owned(), 901, true, false),
            ("acme/web".to_owned(), 902, false, true)
        ]
    );
    assert_eq!(
        fx.event_count(),
        events,
        "no PR owns either: no Event, not even for a review request with failed checks"
    );
    let calls = fx.gh_calls();
    assert_eq!(calls.len(), 1, "both searches in one gh call: {calls:?}");
    assert!(
        is_search(&calls[0])
            && calls[0][3].contains("review-requested:@me is:pr is:open archived:false"),
        "{calls:?}"
    );

    // The same answer: nothing is written.
    let txns = fx.txns();
    assert_eq!(fx.poll_all()["changed"], json!([]));
    assert_eq!(fx.txns(), txns);

    // It leaves the review-request search: its row is kept, read directly, and `review_requested` clears (the viewer did not
    // write it and no request is left); the authored one stays.
    fx.search_both(&[node("acme/pay", 901, "OPEN", None, None)], &[]);
    fx.lookup(&[Some(viewer(
        node("acme/web", 902, "OPEN", Some("APPROVED"), Some("FAILURE")),
        false,
        false,
    ))]);
    let out = fx.poll_all();
    assert!(out.get("removed").is_none(), "nothing is removed: {out}");
    assert_eq!(
        out["changed"].as_array().unwrap().len(),
        1,
        "the flag moved: {out}"
    );
    assert_eq!(
        fx.memberships(),
        [
            ("acme/pay".to_owned(), 901, true, false),
            ("acme/web".to_owned(), 902, false, false)
        ]
    );
    assert_eq!(
        fx.gh_calls().len(),
        4,
        "one call per poll, the third with its direct read"
    );
    assert!(is_lookup(&fx.gh_calls()[3]));
}

#[test]
fn a_search_that_lists_fewer_nodes_than_its_issue_count_deletes_and_clears_nothing() {
    let fx = poll_fx("dispatch");
    let a = node("acme/pay", 901, "OPEN", None, None);
    fx.search(std::slice::from_ref(&a));
    fx.poll_all();
    let id = fx
        .pull_request_row(901)
        .expect("stored by the first poll")
        .0;

    // More than 100 open pull requests: the search says 3 and lists one node.
    let answer = search_answer_with(&[node("acme/web", 902, "OPEN", None, None)], &[], 4987)
        .replacen(r#""issueCount":1,"nodes""#, r#""issueCount":3,"nodes""#, 1);
    assert!(answer.contains(r#""issueCount":3"#), "{answer}");
    fx.search_raw(&answer);
    fx.lookup(&[Some(viewer(a, true, false))]);
    let out = fx.poll_all();

    let added: Vec<&Value> = out["changed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| &c["number"])
        .collect();
    assert_eq!(added, [&json!(902)], "902 is added: {out}");
    assert_eq!(
        fx.memberships(),
        [
            ("acme/pay".to_owned(), 901, true, false),
            ("acme/web".to_owned(), 902, true, false)
        ]
    );
    assert_eq!(fx.pull_request_row(901).map(|r| r.0), Some(id));
    assert_eq!(fx.pull_request_history("delete", None), 0);
    assert_eq!(
        fx.widget_prs(),
        [("acme/pay".to_owned(), 901), ("acme/web".to_owned(), 902)]
    );
    assert_eq!(fx.lookup_texts().len(), 1, "901 was read directly");
}

#[test]
fn a_second_identical_poll_writes_nothing() {
    let fx = poll_fx("deploy");
    fx.search(&[
        pay("OPEN", None, Some("FAILURE")),
        node("acme/web", 902, "OPEN", None, None),
    ]);
    let first = fx.poll();
    assert_eq!(first["changed"].as_array().unwrap().len(), 2);

    let (txns, events) = (fx.txns(), fx.event_count());
    let again = fx.poll();

    assert_eq!(
        again,
        json!({"ok": true, "polled": 2, "changed": [], "base_changed": [], "feedback": [], "update_jobs": [], "merged": [], "done": [],
               "rate_limit": {"cost": 1, "remaining": 4987, "reset_at": RESET}, "workflows": again["workflows"]})
    );
    assert_eq!(
        (fx.txns(), fx.event_count()),
        (txns, events),
        "no write transaction, no Event"
    );
    assert_eq!(fx.gh_calls().iter().filter(|c| is_search(c)).count(), 2);
    assert!(
        fx.gh_calls().iter().all(|c| !is_lookup(c)),
        "812 is in the search: no lookup"
    );
}

#[test]
fn a_value_outside_the_mergeable_sets_fails_the_poll_and_stores_nothing() {
    let fx = poll_fx("deploy");
    for bad in [
        merge_state(pay("OPEN", None, None), "MAYBE", "DIRTY"),
        merge_state(pay("OPEN", None, None), "CONFLICTING", "QUEUED"),
    ] {
        fx.search(&[bad]);
        let run = fx.pr("poll");
        assert_eq!(code(&run), ("gh_failed".into(), 1));
    }
    assert!(fx.events("pr-feedback").is_empty());
    assert_eq!(fx.shown_pull_request()["mergeable"], "unknown");
}

#[test]
fn an_owned_pull_request_that_left_the_search_and_is_closed_is_told_to_the_plan_once() {
    let fx = poll_fx("deploy");
    fx.search(&[]);
    fx.lookup(&[Some(pay(
        "CLOSED",
        Some("CHANGES_REQUESTED"),
        Some("FAILURE"),
    ))]);

    let out = fx.poll();

    assert_eq!(
        (
            out["feedback"].clone(),
            out["merged"].clone(),
            out["done"].clone()
        ),
        (json!([{"plan": "pay", "pr": "repo"}]), json!([]), json!([]))
    );
    let events = fx.events("pr-feedback");
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].1,
        feedback(
            "closed",
            "failed",
            "changes_requested",
            &["checks_failed", "changes_requested", "closed"]
        )
    );
    assert!(
        events[0].0.contains(
            "needs attention: checks failed, changes requested, closed without a merge. Read"
        ),
        "{}",
        events[0].0
    );
    assert_eq!(fx.pr_data().3, "closed");
    assert_eq!(
        (fx.plan_row().0, fx.workflow_status("repo")),
        ("deploy".to_owned(), "open".to_owned()),
        "the PR stays open in cpd"
    );

    // Polled again: still closed, told once; the row is already right, so it is not looked up again (commands-W4).
    let txns = fx.txns();
    assert_eq!(fx.poll()["feedback"], json!([]));
    assert_eq!((fx.txns(), fx.events("pr-feedback").len()), (txns, 1));
    assert_eq!(fx.gh_calls().iter().filter(|c| is_lookup(c)).count(), 1);

    // Reopened: it is in the search again, and nobody is told. Closed again: told again.
    fx.search(&[pay("OPEN", None, None)]);
    fx.poll();
    assert_eq!(fx.pr_data().3, "open");
    assert_eq!(fx.events("pr-feedback").len(), 1);
    fx.search(&[]);
    fx.lookup(&[Some(pay("CLOSED", None, None))]);
    fx.poll();
    assert_eq!(fx.events("pr-feedback").len(), 2);
}

#[test]
fn an_owned_pull_request_that_left_the_search_and_is_merged_takes_the_merged_path() {
    let fx = poll_fx("deploy");
    let (worktree, branch) = kept_attempt(&fx);
    fx.git(&fx.lead, &["push", "-q", "origin", "cpd/pay"]);
    let started = fx.tmux(&[
        "new-session",
        "-d",
        "-s",
        "cpd-work",
        "-n",
        "pay--b-a1",
        "cat",
    ]);
    assert!(started.status.success(), "{started:?}");
    assert!(
        fx.tmux(&[
            "new-window",
            "-d",
            "-t",
            "cpd-work",
            "-n",
            "other-plan",
            "cat"
        ])
        .status
        .success()
    );
    fx.search(&[]);
    fx.lookup(&[Some(
        pay("MERGED", Some("APPROVED"), Some("SUCCESS")).replace("Pay the bills", TITLE),
    )]);

    let out = fx.poll();

    assert_eq!(out["merged"], json!([{"plan": "pay", "pr": "repo"}]));
    assert_eq!(
        (
            out["done"].clone(),
            out["feedback"].clone(),
            out["polled"].clone()
        ),
        (json!(["pay"]), json!([]), json!(0))
    );
    assert_eq!(
        out["changed"],
        json!([{"repo": "acme/pay", "number": 812, "state": "merged", "draft": false, "checks": "passed", "review_decision": "approved", "mergeable": "unknown", "merge_state": "unknown"}])
    );
    assert_eq!(
        out["workflows"],
        json!([{"key": "repo", "status": "merged", "pull_request": fx.shown_pull_request()}])
    );
    assert_eq!(fx.shown_pull_request()["merged_at"], "2026-10-14T10:00:00Z");
    assert_eq!(
        fx.pr_data(),
        (
            TITLE.into(),
            "approved".into(),
            "passed".into(),
            "merged".into()
        )
    );
    assert_eq!(
        (fx.plan_row().0, fx.workflow_status("repo")),
        ("done".to_owned(), "merged".to_owned())
    );
    assert!(fx.plan_row().5.is_some());
    let done = fx.events("plan-done");
    assert_eq!(done.len(), 1);
    assert_eq!(
        done[0].1,
        json!({"workflows": [{"key": "repo", "number": 812, "url": URL, "merged_at": "2026-10-14T10:00:00Z"}]})
    );
    let merged = fx.events("pr-merged");
    assert_eq!(merged.len(), 1);
    assert_eq!(
        merged[0].1,
        json!({"workflow": "repo", "number": 812, "url": URL, "merged_at": "2026-10-14T10:00:00Z"})
    );
    assert!(fx.events("pr-feedback").is_empty());
    // The poll cleans nothing (commands.md §9.2): the Attempt's worktree and branch, the Lead's, and the windows stay for `cpd janitor`.
    assert!(worktree.exists());
    assert_ne!(fx.git(&fx.repo, &["branch", "--list", &branch]), "");
    assert!(fx.lead.exists());
    assert_ne!(fx.git(&fx.repo, &["branch", "--list", "cpd/pay"]), "");
    let windows = fx.window_names();
    assert!(
        windows.contains("pay--b-a1") && windows.contains("cpd-work:other-plan"),
        "{windows}"
    );

    // Done: a rerun polls again and finds nothing new.
    let txns = fx.txns();
    let again = fx.poll();
    assert_eq!(
        (
            again["merged"].clone(),
            again["done"].clone(),
            again["changed"].clone()
        ),
        (json!([]), json!([]), json!([]))
    );
    assert_eq!((fx.txns(), fx.events("plan-done").len()), (txns, 1));
}

#[test]
fn an_owned_row_a_search_misses_is_read_and_an_open_answer_with_the_same_values_changes_nothing() {
    let fx = poll_fx("deploy");
    fx.mark_read(812);
    fx.search(&[]);
    fx.lookup(&[Some(pay("OPEN", None, None))]);
    let (txns, events) = (fx.txns(), fx.event_count());

    let out = fx.poll();

    assert_eq!(
        (out["polled"].clone(), out["changed"].clone()),
        (json!(0), json!([]))
    );
    assert!(out.get("removed").is_none(), "nothing is removed: {out}");
    assert_eq!((fx.txns(), fx.event_count()), (txns, events));
    assert_eq!(fx.pull_requests(), [("acme/pay".to_owned(), 812)]);
    let calls = fx.gh_calls();
    assert!(
        is_search(&calls[0]) && is_lookup(&calls[1]) && calls.len() == 2,
        "{calls:?}"
    );
    assert!(calls[1][3].contains(r#"p0: repository(owner: "acme", name: "pay") { pullRequest(number: 812) { repository { nameWithOwner defaultBranchRef { name } } number url"#), "{}", calls[1][3]);
    assert!(
        calls[1][3].contains(
            "viewerDidAuthor viewerLatestReviewRequest { id } repository { isArchived } }"
        ),
        "{}",
        calls[1][3]
    );
}

#[test]
fn dispatch_rows_carry_label_number_state_checks_decision_and_title() {
    let fx = poll_fx("deploy");
    // Before the first poll the row is what `pr open` wrote: the Plan's title, no review, no checks.
    assert_eq!(
        dispatch_card(&fx),
        (
            "Lead".into(),
            Some((
                812,
                PrState::Open,
                Some(("Pay the bills".into(), ReviewDecision::None, Checks::None))
            ))
        )
    );

    fx.search(&[
        pay("OPEN", Some("CHANGES_REQUESTED"), Some("FAILURE")).replace("Pay the bills", TITLE)
    ]);
    fx.poll();
    assert_eq!(
        dispatch_card(&fx),
        (
            "Lead".into(),
            Some((
                812,
                PrState::Open,
                Some((
                    TITLE.into(),
                    ReviewDecision::ChangesRequested,
                    Checks::Failed
                ))
            ))
        )
    );

    fx.search(&[]);
    fx.lookup(&[Some(
        pay("MERGED", Some("APPROVED"), Some("SUCCESS")).replace("Pay the bills", TITLE),
    )]);
    fx.poll();
    assert_eq!(
        dispatch_card(&fx),
        (
            "Lead".into(),
            Some((
                812,
                PrState::Merged,
                Some((TITLE.into(), ReviewDecision::Approved, Checks::Passed))
            ))
        )
    );
}

#[test]
fn improvements_are_stored_and_tell_nobody() {
    let fx = poll_fx("deploy");
    fx.search(&[pay("OPEN", None, Some("FAILURE"))]);
    fx.poll();
    assert_eq!(
        fx.pr_data(),
        (
            "Pay the bills".into(),
            "none".into(),
            "failed".into(),
            "open".into()
        )
    );

    fx.search(&[pay("OPEN", Some("APPROVED"), Some("SUCCESS")).replace("Pay the bills", TITLE)]);
    let out = fx.poll();

    assert_eq!(out["feedback"], json!([]));
    assert_eq!(
        fx.pr_data(),
        (
            TITLE.into(),
            "approved".into(),
            "passed".into(),
            "open".into()
        )
    );
    assert_eq!(
        fx.events("pr-feedback").len(),
        1,
        "only the first poll's checks turning failed"
    );
}

// ---------------------------------------------------------------- pr rebase
//
// In-process, like the poll and open groups: `cpd pr rebase` runs on `run_with` over the git route (the rebase, its stop at a
// conflict, `--continue` and `--abort` are the route's: `rebase-merge/` on disk, markers in the files), the Gate's `sh` and
// the `gh` that nothing may call are handlers. A Gate that the old tests paused for a second process (`paused_gate`) is the
// `sh` handler: what the other process did meanwhile, it does when the Gate runs.

/// The Plan in `phase`, as the old full fixture left it: where the PR is open, its Lead Branch is pushed (`-u`).
pub(super) fn rebase_fx(phase: &str) -> Poll {
    let fx = poll_fx(phase);
    if phase == "deploy" {
        fx.git(&fx.lead, &["push", "-q", "-u", "origin", &fx.branch]);
    }
    fx
}

impl Poll {
    /// The Gate's `pre-merge = "git rev-parse HEAD"`: the HEAD of the directory the hook runs in.
    pub(super) fn sh_prints_head(&self) {
        let ports = self.fakes.ports.clone();
        self.sh(move |call| {
            assert_eq!(
                call.args[1], "git rev-parse HEAD",
                "the only hook of the test"
            );
            Reply::ok(&format!(
                "{}\n",
                git_out_in_hook(
                    &ports,
                    call.cwd.as_ref().expect("a hook runs in a directory"),
                    &["rev-parse", "HEAD"]
                )
            ))
        });
    }

    /// No gate worktree is left: none on disk next to the repo, none known to git.
    fn assert_no_gate_worktree(&self) {
        let left: Vec<String> = fs::read_dir(self.repo.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".gate-"))
            .collect();
        assert!(left.is_empty(), "{left:?}");
        let listed = self.git(&self.repo, &["worktree", "list", "--porcelain"]);
        assert!(!listed.contains(".gate-"), "{listed}");
    }
}

#[test]
fn rebase_onto_a_moved_base_gates_and_force_pushes_with_a_lease() {
    let fx = rebase_fx("deploy");
    fx.hooks("pre-merge = \"git rev-parse HEAD\"\n");
    fx.sh_prints_head();
    let old = fx.lead_sha();
    let base = fx.advance_base("base.txt", "base\n");

    let out = fx.pr("rebase").ok();

    let new = fx.lead_sha();
    assert_ne!(new, old);
    assert_eq!(
        out,
        json!({"ok": true, "plan": "pay", "pr": "repo", "rebased": true, "old": old, "new": new, "pushed": true})
    );
    assert!(
        fx.git(&fx.repo, &["merge-base", "--is-ancestor", &base, &new])
            .is_empty()
    );
    // The Lead worktree is on the rebased branch and clean.
    assert_eq!(
        fs::read_to_string(fx.lead.join("base.txt")).unwrap(),
        "base\n"
    );
    assert_eq!(fx.git(&fx.lead, &["status", "--porcelain"]), "");
    // The Gate ran after the rebase; the push followed it.
    assert_eq!(fx.gate_log().trim(), new);
    assert_eq!(fx.remote_branch("cpd/pay"), Some(new));
    // No Ledger row, no Event.
    assert!(fx.events("plan-phase").is_empty());
    assert_eq!(fx.plan_row().0, "deploy");
}

impl Poll {
    /// Seeds the PR's row as the poll left it after a red run: failed checks, conflicting, dirty.
    fn seed_red_pull_request(&self) {
        self.env.sql(
            "UPDATE pull_requests SET checks = 'failed', mergeable = 'conflicting', merge_state = 'dirty'
             WHERE id = (SELECT pull_request FROM workflows WHERE plan = 'pay' AND key = 'repo');",
        );
    }

    /// `(checks, mergeable, merge_state)` of the row the Workflow `repo` of Plan `pay` points to.
    fn pull_request_poll_state(&self) -> (String, String, String) {
        self.store()
            .read(|c| {
                Ok(c.query_row(
                    "SELECT q.checks, q.mergeable, q.merge_state FROM workflows w JOIN pull_requests q ON q.id = w.pull_request
                     WHERE w.plan = 'pay' AND w.key = 'repo'",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )?)
            })
            .unwrap()
    }
}

fn reset_state() -> (String, String, String) {
    ("pending".into(), "unknown".into(), "unknown".into())
}

#[test]
fn pr_rebase_resets_stored_checks_mergeable_and_merge_state() {
    let fx = rebase_fx("deploy");
    fx.hooks("pre-merge = \"git rev-parse HEAD\"\n");
    fx.sh_prints_head();
    fx.advance_base("base.txt", "base\n");
    fx.seed_red_pull_request();

    let out = fx.pr("rebase").ok();

    assert_eq!(out["pushed"], true, "{out}");
    assert_eq!(fx.pull_request_poll_state(), reset_state());
}

#[test]
fn pr_rebase_adopted_resets_stored_checks_mergeable_and_merge_state() {
    let fx = rebase_fx("deploy");
    fx.hooks("pre-merge = \"git rev-parse HEAD\"\n");
    fx.sh_prints_head();
    // The PR's head `feat/x` is one commit past the base; the Lead Branch lacks it and has its own commits.
    fx.commit_file(&fx.repo, "head.txt", "the head\n", "the head");
    fx.git(
        &fx.repo,
        &["push", "-q", "origin", "HEAD:refs/heads/feat/x"],
    );
    fx.git(&fx.repo, &["reset", "-q", "--hard", "HEAD~1"]);
    fx.env.sql(
        "DROP TRIGGER workflows_head_ref_fixed; UPDATE workflows SET head_ref = 'feat/x';
         CREATE TRIGGER workflows_head_ref_fixed BEFORE UPDATE OF head_ref ON workflows\nBEGIN SELECT RAISE(ABORT, 'workflow_locked'); END;",
    );
    fx.seed_red_pull_request();

    let out = fx.pr("rebase").ok();

    assert_eq!(out["pushed"], true, "{out}");
    assert_eq!(fx.remote_branch("feat/x"), Some(fx.lead_sha()));
    assert_eq!(fx.pull_request_poll_state(), reset_state());
}

#[test]
fn rebase_with_the_base_unmoved_only_gates_and_pushes() {
    let fx = rebase_fx("deploy");
    // A Job Merged in phase pr: on the Lead Branch, not yet on the PR.
    fs::write(fx.lead.join("more.txt"), "more\n").unwrap();
    fx.git(&fx.lead, &["add", "-A"]);
    fx.git(&fx.lead, &["commit", "-q", "-m", "more"]);
    let tip = fx.lead_sha();
    assert_ne!(fx.remote_branch("cpd/pay"), Some(tip.clone()));

    let out = fx.pr("rebase").ok();

    assert_eq!(
        out,
        json!({"ok": true, "plan": "pay", "pr": "repo", "rebased": false, "old": tip, "new": tip, "pushed": true})
    );
    assert_eq!(fx.remote_branch("cpd/pay"), Some(tip));
    // Idempotent: again, the same.
    assert_eq!(fx.pr("rebase").ok()["rebased"], false);
}

impl Poll {
    /// The Lead Branch commits `README.md` as `lead` and pushes; the base then commits it as `base`.
    /// Returns the Lead Branch's tip.
    pub(super) fn conflicting(&self, lead: &[&str]) -> String {
        for text in lead {
            fs::write(self.lead.join("README.md"), format!("{text}\n")).unwrap();
            self.git(&self.lead, &["add", "-A"]);
            self.git(&self.lead, &["commit", "-q", "-m", &format!("lead {text}")]);
        }
        self.git(&self.lead, &["push", "-q", "origin", "cpd/pay"]);
        self.advance_base("README.md", "base\n");
        self.lead_sha()
    }

    /// `cpd pr rebase --plan pay <flag>`.
    pub(super) fn rebase(&self, flag: &str) -> Run {
        self.cpd(&["pr", "rebase", "--plan", "pay", flag])
    }

    pub(super) fn rebasing(&self) -> bool {
        ["rebase-merge", "rebase-apply"].iter().any(|name| {
            let path = self.git(
                &self.lead,
                &["rev-parse", "--path-format=absolute", "--git-path", name],
            );
            Path::new(&path).exists()
        })
    }

    /// Resolves `README.md` in the Lead worktree and stages it, as the Lead does.
    pub(super) fn resolve(&self, text: &str) {
        fs::write(self.lead.join("README.md"), format!("{text}\n")).unwrap();
        self.git(&self.lead, &["add", "README.md"]);
    }
}

#[test]
fn rebase_stops_at_a_conflict_in_the_lead_worktree_and_pushes_nothing() {
    let fx = rebase_fx("deploy");
    fx.hooks("pre-merge = \"git rev-parse HEAD\"\n");
    fx.sh_prints_head();
    let before = fx.conflicting(&["lead"]);

    let run = fx.pr("rebase");

    assert_eq!(code(&run), ("rebase_conflict".into(), 3));
    let details = &run.error()["details"];
    assert_eq!(details["files"], json!(["README.md"]));
    assert_eq!(details["worktree"], fx.lead.to_str().unwrap());
    assert_eq!(
        details["next"],
        "cpd pr rebase --workflow repo --plan pay --continue"
    );
    assert_eq!(
        details["abort"],
        "cpd pr rebase --workflow repo --plan pay --abort"
    );
    // The rebase waits in the Lead worktree, with markers; the ref and the remote are as they were.
    assert!(fx.rebasing());
    assert!(
        fs::read_to_string(fx.lead.join("README.md"))
            .unwrap()
            .contains("<<<<<<<")
    );
    assert_eq!(fx.lead_sha(), before);
    assert_eq!(fx.remote_branch("cpd/pay"), Some(before));
    // No Gate, no `gh`, no gate worktree, no Event.
    assert_eq!(fx.gate_log(), "");
    assert!(fx.gh_calls().is_empty());
    fx.assert_no_gate_worktree();
    assert!(fx.events("error").is_empty());

    // A second plain run does not start another one.
    let again = fx.pr("rebase");
    assert_eq!(code(&again), ("rebase_in_progress".into(), 3));
    assert_eq!(again.error()["details"]["next"], details["next"]);
    assert_eq!(again.error()["details"]["abort"], details["abort"]);
    assert!(fx.rebasing());
}

#[test]
fn rebase_continue_gates_and_pushes_the_resolved_rebase_with_the_lease() {
    let fx = rebase_fx("deploy");
    fx.hooks("pre-merge = \"git rev-parse HEAD\"\n");
    fx.sh_prints_head();
    let before = fx.conflicting(&["lead"]);
    assert_eq!(code(&fx.pr("rebase")).0, "rebase_conflict");
    fx.resolve("resolved");

    let out = fx.rebase("--continue").ok();

    let new = fx.lead_sha();
    assert_ne!(new, before);
    assert_eq!(
        out,
        json!({"ok": true, "plan": "pay", "pr": "repo", "rebased": true, "old": before, "new": new, "pushed": true})
    );
    assert!(!fx.rebasing());
    assert_eq!(fx.git(&fx.lead, &["status", "--porcelain"]), "");
    assert_eq!(
        fs::read_to_string(fx.lead.join("README.md")).unwrap(),
        "resolved\n"
    );
    assert_eq!(fx.git(&fx.lead, &["rev-parse", "HEAD"]), new);
    // The Gate ran on the rebased commit, then the push.
    assert_eq!(fx.gate_log().trim(), new);
    assert_eq!(fx.remote_branch("cpd/pay"), Some(new));
    fx.assert_no_gate_worktree();
    // No Event, and nothing left to continue.
    assert!(fx.events("error").is_empty());
    assert_eq!(
        code(&fx.rebase("--continue")),
        ("no_rebase_in_progress".into(), 3)
    );
}

#[test]
fn rebase_continue_stops_again_at_a_second_conflicting_commit() {
    let fx = rebase_fx("deploy");
    let before = fx.conflicting(&["lead 1", "lead 2"]);
    assert_eq!(code(&fx.pr("rebase")).0, "rebase_conflict");
    fx.resolve("resolved 1");

    let run = fx.rebase("--continue");

    assert_eq!(code(&run), ("rebase_conflict".into(), 3));
    assert_eq!(run.error()["details"]["files"], json!(["README.md"]));
    assert_eq!(
        run.error()["details"]["next"],
        "cpd pr rebase --workflow repo --plan pay --continue"
    );
    assert!(fx.rebasing());
    assert_eq!(fx.lead_sha(), before);
    assert_eq!(fx.remote_branch("cpd/pay"), Some(before.clone()));

    fx.resolve("resolved 2");
    let out = fx.rebase("--continue").ok();
    assert_eq!(
        (out["rebased"].as_bool(), out["old"].as_str()),
        (Some(true), Some(before.as_str()))
    );
    assert_eq!(fx.remote_branch("cpd/pay"), Some(fx.lead_sha()));
    assert_eq!(
        fs::read_to_string(fx.lead.join("README.md")).unwrap(),
        "resolved 2\n"
    );
    assert!(!fx.rebasing());
}

#[test]
fn rebase_continue_refuses_while_conflict_markers_remain() {
    let fx = rebase_fx("deploy");
    let before = fx.conflicting(&["lead"]);
    assert_eq!(code(&fx.pr("rebase")).0, "rebase_conflict");

    // Unmerged.
    let run = fx.rebase("--continue");
    assert_eq!(code(&run), ("rebase_conflict".into(), 3));
    assert_eq!(run.error()["details"]["files"], json!(["README.md"]));
    assert_eq!(
        run.error()["details"]["next"],
        "cpd pr rebase --workflow repo --plan pay --continue"
    );

    // Staged with the markers in it.
    fx.git(&fx.lead, &["add", "README.md"]);
    let run = fx.rebase("--continue");
    assert_eq!(code(&run), ("rebase_conflict".into(), 3));
    assert_eq!(run.error()["details"]["files"], json!(["README.md"]));

    assert!(fx.rebasing());
    assert_eq!(fx.lead_sha(), before);
    assert_eq!(fx.remote_branch("cpd/pay"), Some(before));
    assert_eq!(fx.gate_log(), "");
}

#[test]
fn rebase_continue_and_abort_refuse_when_no_rebase_is_in_progress() {
    let fx = rebase_fx("deploy");
    let before = fx.lead_sha();

    for flag in ["--continue", "--abort"] {
        let run = fx.rebase(flag);
        assert_eq!(code(&run), ("no_rebase_in_progress".into(), 3), "{flag}");
    }
    assert_eq!(fx.lead_sha(), before);
    assert!(fx.gh_calls().is_empty());
    assert_eq!(fx.gate_log(), "");
}

#[test]
fn rebase_continue_and_abort_together_are_a_usage_error() {
    let fx = rebase_fx("deploy");
    let run = fx.cpd(&["pr", "rebase", "--plan", "pay", "--continue", "--abort"]);
    assert_eq!(code(&run), ("usage".into(), 2));
}

#[test]
fn rebase_abort_restores_the_branch() {
    let fx = rebase_fx("deploy");
    fx.hooks("pre-merge = \"git rev-parse HEAD\"\n");
    fx.sh_prints_head();
    let before = fx.conflicting(&["lead"]);
    assert_eq!(code(&fx.pr("rebase")).0, "rebase_conflict");

    let out = fx.rebase("--abort").ok();

    assert_eq!(
        out,
        json!({"ok": true, "plan": "pay", "pr": "repo", "aborted": true, "tip": before})
    );
    assert!(!fx.rebasing());
    assert_eq!(fx.lead_sha(), before);
    assert_eq!(fx.git(&fx.lead, &["rev-parse", "HEAD"]), before);
    assert_eq!(
        fx.git(&fx.lead, &["symbolic-ref", "--short", "HEAD"]),
        "cpd/pay"
    );
    assert_eq!(fx.git(&fx.lead, &["status", "--porcelain"]), "");
    assert_eq!(
        fs::read_to_string(fx.lead.join("README.md")).unwrap(),
        "lead\n"
    );
    assert_eq!(fx.remote_branch("cpd/pay"), Some(before));
    assert_eq!(fx.gate_log(), "");
    assert!(fx.events("error").is_empty());
    // Idempotent: no.
    assert_eq!(
        code(&fx.rebase("--abort")),
        ("no_rebase_in_progress".into(), 3)
    );
}

#[test]
fn rebase_continue_does_not_overwrite_a_push_it_has_not_seen() {
    let fx = rebase_fx("deploy");
    fx.conflicting(&["lead"]);
    assert_eq!(code(&fx.pr("rebase")).0, "rebase_conflict");
    // Someone else pushes to the PR's branch during the stop.
    let clone = fx.env.root().join("repos/clone");
    fx.git(
        fx.env.root(),
        &[
            "clone",
            "-q",
            fx.origin.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    fx.git(&clone, &["checkout", "-q", "cpd/pay"]);
    fs::write(clone.join("theirs.txt"), "theirs\n").unwrap();
    fx.git(&clone, &["add", "-A"]);
    fx.git(&clone, &["commit", "-q", "-m", "theirs"]);
    fx.git(&clone, &["push", "-q", "origin", "cpd/pay"]);
    let theirs = fx.remote_branch("cpd/pay");
    fx.resolve("resolved");

    let run = fx.rebase("--continue");

    assert_eq!(code(&run), ("git_failed".into(), 1));
    assert_eq!(fx.remote_branch("cpd/pay"), theirs);
    fx.assert_no_gate_worktree();
}

#[test]
fn rebase_continue_refuses_when_the_lead_branch_moved_during_the_stop() {
    let fx = rebase_fx("deploy");
    // The parent of the Lead Branch's tip after the conflicting commit (the fake git does not read `<sha>^`).
    let moved = fx.lead_sha();
    let before = fx.conflicting(&["lead"]);
    assert_eq!(code(&fx.pr("rebase")).0, "rebase_conflict");
    // A merge lands during the stop: the ref moves under the rebase.
    fx.git(&fx.repo, &["update-ref", "refs/heads/cpd/pay", &moved]);
    fx.resolve("resolved");

    let run = fx.rebase("--continue");

    assert_eq!(code(&run), ("cas_lost".into(), 3));
    assert_eq!(run.error()["details"]["reason"], "lead_branch_moved");
    assert_eq!(run.error()["details"]["tip"], moved);
    assert!(fx.rebasing());
    assert_eq!(fx.remote_branch("cpd/pay"), Some(before));
}

#[test]
fn rebase_with_tracked_local_changes_that_a_conflict_needs_refuses_and_changes_nothing() {
    let fx = rebase_fx("deploy");
    let before = fx.conflicting(&["lead"]);
    fs::write(fx.lead.join("api.txt"), "edited\n").unwrap();

    let run = fx.pr("rebase");

    assert_eq!(code(&run), ("lead_worktree_dirty".into(), 3));
    assert_eq!(run.error()["details"]["files"], json!(["api.txt"]));
    assert!(!fx.rebasing());
    assert_eq!(fx.lead_sha(), before);
    assert_eq!(
        fs::read_to_string(fx.lead.join("api.txt")).unwrap(),
        "edited\n"
    );
}

#[test]
fn rebase_refuses_a_pr_that_is_not_open() {
    for (phase, status) in [("dispatch", "dispatch"), ("done", "merged")] {
        let fx = rebase_fx(phase);
        let run = fx.pr("rebase");
        assert_eq!(code(&run), ("illegal_transition".into(), 3), "{phase}");
        assert_eq!(run.error()["details"]["from"], status);
        assert_eq!(run.error()["details"]["pr"], "repo");
    }
}

#[test]
fn rebase_refuses_a_plan_that_is_not_locked() {
    let fx = poll_fx("plan");
    let run = fx.pr("rebase");
    assert_eq!(code(&run), ("illegal_phase".into(), 3));
    assert_eq!(run.error()["details"]["phase"], "plan");
}

#[test]
fn rebase_keeps_the_lead_worktrees_local_changes_and_reports_it_stale() {
    let fx = rebase_fx("deploy");
    fs::write(fx.lead.join("api.txt"), "edited\n").unwrap();
    let base = fx.advance_base("base.txt", "base\n");

    let out = fx.pr("rebase").ok();

    // Rebased and pushed: the local changes refuse nothing (no `dirty_target`, hs-d243).
    let new = fx.lead_sha();
    assert_eq!(
        (out["rebased"].as_bool(), out["new"].as_str()),
        (Some(true), Some(new.as_str()))
    );
    assert!(
        fx.git(&fx.repo, &["merge-base", "--is-ancestor", &base, &new])
            .is_empty()
    );
    assert_eq!(fx.remote_branch("cpd/pay"), Some(new));
    // Rule S stopped rather than overwrite them: the edit is kept, reported once.
    assert_eq!(
        fs::read_to_string(fx.lead.join("api.txt")).unwrap(),
        "edited\n"
    );
    assert!(!fx.lead.join("base.txt").exists());
    let errors = fx.events("error");
    assert_eq!(errors.len(), 1, "{errors:?}");
    let data = &errors[0].1;
    assert_eq!(
        (
            data["source"].as_str(),
            data["code"].as_str(),
            data["details"]["pr"].as_str()
        ),
        (Some("pr rebase"), Some("lead_worktree_stale"), Some("repo"))
    );
    assert_eq!(data["details"]["worktree"], fx.lead.to_str().unwrap());
}

#[test]
fn rebase_of_a_pr_merged_during_its_gate_pushes_nothing() {
    let fx = rebase_fx("deploy");
    fx.hooks("pre-merge = \"paused-gate\"\n");
    fx.git(&fx.lead, &["push", "-q", "origin", "cpd/pay"]);
    let pushed = fx.remote_branch("cpd/pay");
    fx.advance_base("base.txt", "base\n");
    // The Gate is paused here: the poll records the merge of the PR meanwhile.
    let home = fx.home.clone();
    fx.sh(move |_| {
        Store::open(&home, 5000)
            .unwrap()
            .write("pr poll", |tx| {
                tx.execute_batch(
                    "UPDATE workflows SET status = 'merged', merged_at = 't' WHERE plan = 'pay'",
                )?;
                Ok(())
            })
            .unwrap();
        Reply::ok("")
    });

    let out: Value = serde_json::from_str(&fx.pr("rebase").stdout).unwrap();

    assert_eq!(
        (
            out["error"]["code"].as_str(),
            out["error"]["details"]["from"].as_str()
        ),
        (Some("illegal_transition"), Some("merged")),
        "{out}"
    );
    assert_eq!(fx.commands(), ["paused-gate"], "the Gate ran");
    assert_eq!(fx.remote_branch("cpd/pay"), pushed, "nothing pushed");
    fx.assert_no_gate_worktree();
}

#[test]
fn rebase_succeeds_when_the_remote_already_contains_the_rebased_commit() {
    // What a second `pr rebase` of the PR at once can meet: the other run pushed the same R'
    // (and a merge onto it followed) after this run read its lease (Escalation nl-pr-a1-q2).
    let fx = rebase_fx("deploy");
    fx.hooks("pre-merge = \"paused-gate\"\n");
    fx.git(&fx.lead, &["push", "-q", "origin", "cpd/pay"]);
    fx.advance_base("base.txt", "base\n");
    // The Gate is paused here: the other run's merge lands on R' and is pushed, over the lease this run read.
    let (ports, repo, lead) = (fx.fakes.ports.clone(), fx.repo.clone(), fx.lead.clone());
    let seen = Arc::new(Mutex::new(None));
    let during = seen.clone();
    fx.sh(move |_| {
        let rebased = git_out_in_hook(&ports, &repo, &["rev-parse", "refs/heads/cpd/pay"]);
        fs::write(lead.join("m.txt"), "m\n").unwrap();
        git_in_hook(&ports, &lead, &["add", "-A"]);
        git_in_hook(&ports, &lead, &["commit", "-q", "-m", "m"]);
        let merged = git_out_in_hook(&ports, &repo, &["rev-parse", "refs/heads/cpd/pay"]);
        // `push --force`, as the fake git has it: a lease on the very value the remote branch holds.
        let held = git_out_in_hook(&ports, &repo, &["rev-parse", "refs/remotes/origin/cpd/pay"]);
        git_in_hook(
            &ports,
            &lead,
            &[
                "push",
                "-q",
                &format!("--force-with-lease=refs/heads/cpd/pay:{held}"),
                "origin",
                "cpd/pay",
            ],
        );
        *during.lock().unwrap() = Some((rebased, merged));
        Reply::ok("")
    });

    let out: Value = serde_json::from_str(&fx.pr("rebase").stdout).unwrap();

    let (rebased, merged) = seen.lock().unwrap().clone().expect("the Gate ran");
    assert_eq!(
        (out["ok"].as_bool(), out["new"].as_str()),
        (Some(true), Some(rebased.as_str())),
        "{out}"
    );
    assert_eq!(fx.remote_branch("cpd/pay"), Some(merged), "nothing rewound");
}

#[test]
fn two_rebases_of_one_pr_at_once_both_succeed_and_the_remote_is_the_local_tip() {
    let fx = rebase_fx("deploy");
    fx.hooks("pre-merge = \"sleep 1\"\n");
    fx.sh(|_| Reply::ok(""));
    fx.git(&fx.lead, &["push", "-q", "origin", "cpd/pay"]);
    let base = fx.advance_base("base.txt", "base\n");

    // Both run in this one process, whose pid names the temporary rebase worktree: the two cannot overlap, so the second
    // starts when the first is done and finds the rebase done (the case the compare-and-swap leaves it).
    let outs = [fx.pr("rebase"), fx.pr("rebase")];

    // The compare-and-swap picks one; the other finds the rebase done (or loses every round).
    for out in &outs {
        let json: Value = serde_json::from_str(&out.stdout).unwrap();
        assert!(
            json["ok"] == true || json["error"]["code"] == "cas_lost",
            "{json}"
        );
    }
    let tip = fx.lead_sha();
    assert!(
        fx.git(&fx.repo, &["merge-base", "--is-ancestor", &base, &tip])
            .is_empty()
    );
    assert_eq!(
        fx.git(
            &fx.repo,
            &["rev-list", "--count", &format!("{base}..{tip}")]
        ),
        "2",
        "rebased once: api and hooks on the base"
    );
    assert_eq!(fx.remote_branch("cpd/pay"), Some(tip));
    fx.assert_no_gate_worktree();
}

#[test]
fn rebase_with_a_failing_gate_keeps_the_rebased_branch_and_pushes_nothing() {
    let fx = rebase_fx("deploy");
    fx.hooks("pre-merge = \"echo red; exit 1\"\n");
    fx.sh(|_| Reply::exit(1, "red\n", ""));
    fx.git(&fx.lead, &["push", "-q", "origin", "cpd/pay"]);
    let pushed = fx.remote_branch("cpd/pay");
    let old = fx.lead_sha();
    fx.advance_base("base.txt", "base\n");

    let run = fx.pr("rebase");

    assert_eq!(code(&run), ("gate_failed".into(), 3));
    let details = &run.error()["details"];
    assert!(details["tail"].as_str().unwrap().contains("red"));
    assert!(
        details["log"]
            .as_str()
            .unwrap()
            .ends_with("logs/pr-repo.pre-merge.log")
    );
    // The rebased Lead Branch stays; the remote does not move.
    assert_ne!(fx.lead_sha(), old);
    assert!(fx.lead.join("base.txt").exists());
    assert_eq!(fx.remote_branch("cpd/pay"), pushed);
}

#[test]
fn rebase_with_a_gate_that_cannot_be_judged_is_gate_error() {
    let fx = rebase_fx("deploy");
    fx.hooks("pre-merge = \"echo {{ no_such_variable }}\"\n");
    fx.git(&fx.lead, &["push", "-q", "origin", "cpd/pay"]);
    let pushed = fx.remote_branch("cpd/pay");

    let run = fx.pr("rebase");

    assert_eq!(code(&run), ("gate_error".into(), 1));
    assert_eq!(fx.remote_branch("cpd/pay"), pushed);
}

#[test]
fn rebase_does_not_overwrite_a_push_it_has_not_seen() {
    let fx = rebase_fx("deploy");
    // Someone else pushes to the PR's branch.
    let clone = fx.env.root().join("repos/clone");
    fx.git(
        fx.env.root(),
        &[
            "clone",
            "-q",
            fx.origin.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    fx.git(&clone, &["checkout", "-q", "cpd/pay"]);
    fs::write(clone.join("theirs.txt"), "theirs\n").unwrap();
    fx.git(&clone, &["add", "-A"]);
    fx.git(&clone, &["commit", "-q", "-m", "theirs"]);
    fx.git(&clone, &["push", "-q", "origin", "cpd/pay"]);
    let theirs = fx.remote_branch("cpd/pay");

    let run = fx.pr("rebase");

    assert_eq!(code(&run), ("git_failed".into(), 1));
    assert_eq!(fx.remote_branch("cpd/pay"), theirs);
}

// ------------------------------------------------------------- two PRs (R34)

#[test]
fn open_refuses_a_pr_that_is_waiting_and_names_only_its_own_jobs() {
    let fx = poll_fx("dispatch");
    fx.add_waiting_web();

    let run = fx.pr_cmd("open", "web");
    assert_eq!(code(&run), ("illegal_transition".into(), 3));
    assert_eq!(run.error()["details"]["from"], "waiting");

    let run = fx.pr_cmd("open", "nope");
    assert_eq!(code(&run), ("workflow_not_found".into(), 4));

    // `web`'s queued Job does not keep `repo` from getting its opener (the Plan stays in dispatch: `web` waits).
    let out = fx.pr_cmd("open", "repo").ok();
    assert_eq!(
        (&out["job"], &out["status"]),
        (&json!("open-repo"), &json!("queued")),
        "{out}"
    );
    assert_eq!(fx.phase(), "dispatch");
    assert_eq!(fx.workflow_status("web"), "waiting");
}

#[test]
fn rebase_touches_only_its_pr() {
    let fx = rebase_fx("deploy");
    let web = fx.add_open_web();
    let web_repo = web.parent().unwrap().join("web");
    let (repo_before, web_before) = (
        fx.lead_sha(),
        fx.git(&web_repo, &["rev-parse", "refs/heads/cpd/pay/web"]),
    );
    let web_remote = |fx: &Poll| {
        fx.git(
            &fx.env.root().join("repos/web-origin.git"),
            &["rev-parse", "refs/heads/cpd/pay/web"],
        )
    };
    let web_remote_before = web_remote(&fx);
    // The base moves in `web` only.
    fs::write(web_repo.join("base.txt"), "base\n").unwrap();
    fx.git(&web_repo, &["add", "-A"]);
    fx.git(&web_repo, &["commit", "-q", "-m", "base"]);
    fx.git(&web_repo, &["push", "-q", "origin", "main"]);

    // Two PRs: --workflow is required.
    assert_eq!(code(&fx.pr("rebase")), ("workflow_required".into(), 2));

    let out = fx.pr_cmd("rebase", "web").ok();

    let new = fx.git(&web_repo, &["rev-parse", "refs/heads/cpd/pay/web"]);
    assert_eq!(
        out,
        json!({"ok": true, "plan": "pay", "pr": "web", "rebased": true, "old": web_before, "new": new, "pushed": true})
    );
    assert_ne!(new, web_before);
    assert_eq!(web_remote(&fx), new);
    assert_ne!(web_remote(&fx), web_remote_before);
    assert_eq!(fs::read_to_string(web.join("base.txt")).unwrap(), "base\n");
    // `repo`'s Lead Branch, worktree and remote are as they were.
    assert_eq!(fx.lead_sha(), repo_before);
    assert_eq!(fx.remote_branch("cpd/pay"), Some(repo_before));
    assert!(fx.events("plan-phase").is_empty() && fx.gh_calls().is_empty());
}

/// Plan `pay` in `phase` (`done` or `dispatch`) with its one PR `repo`, in-process: `cpd` runs through `run_with` on the
/// fake ports, and the repo is a keyed repo of the git route. What `pr add` reads of git is the repo's refs.
pub(crate) struct Added {
    pub(crate) fx: Kit,
    pub(crate) repo: PathBuf,
}

pub(crate) fn added(phase: &str) -> Added {
    let fx = Kit::with_ports();
    fx.env.owner_policy("acme", "[pr]\ngate = \"local\"\n");
    let repo = fx.keyed_repo("pay").canonicalize().unwrap();
    // A bare `origin` of the route, as the full fixture's: `pr add` on a reopened Plan asks it for the Lead Branch.
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
    fx.git(&repo, &["push", "-q", "origin", "main"]);
    let merged_sha = fx.git(&repo, &["rev-parse", "HEAD"]);
    // The Lead Branch of PR `repo`, `cpd/pay`, and its Lead worktree (a plain directory: `pr add` only reads its name).
    fx.git(&repo, &["branch", "cpd/pay"]);
    let lead = fx.env.root().join("lead");
    fs::create_dir_all(&lead).unwrap();
    fx.env.write_tmux_config("");
    let (status, columns, values, merged) = match phase {
        "dispatch" => ("dispatch", "", String::new(), "NULL"),
        "done" => (
            "merged",
            ", pr_number, pr_url, pr_opened_at, pull_request, merged_at",
            format!(", 812, '{URL}', 'opened', (SELECT id FROM pull_requests), 'finished'"),
            "'finished'",
        ),
        other => panic!("no phase {other}"),
    };
    let pull_request = match phase {
        "done" => format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
             VALUES ('acme/pay', 812, '{URL}', 'Pay the bills', 0, 'merged', 'none', 'none', 'opened', 'finished', 'finished');"
        ),
        _ => String::new(),
    };
    let sql = format!(
        "INSERT INTO contexts (id, title, owner, created_at, created_on) VALUES ('ctx', 'Ctx', 'acme', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay', 'ctx', 'Pay the bills', '{phase}', 'lead1', 't', 't', {merged});
         {pull_request}
         INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at{columns})
           VALUES ('pay', 'repo', '{repo}', 'pay', 'main', '{status}', 'cpd/pay', '{lead}', '{REPO_TITLE}', 'config', cpd_txn(), 't', 't'{values});",
        repo = repo.display(),
        lead = lead.display(),
    );
    let home = CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(&sql)?;
            tx.execute(
                "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                   status, appended, fingerprint, merged_sha, created_txn, created_at)
                 VALUES ('pay', 'repo', 'api', 'The | API', 'S2', 'g', 'c', '[\"**\"]', '[\"b\"]', '[]', 'merged', 0, ?1, ?2, ?3, 't')",
                params!["f".repeat(64), merged_sha, tx.id()],
            )?;
            Ok(())
        })
        .unwrap();
    Added { fx, repo }
}

impl Added {
    /// The `needs` of PR `key` in Plan `pay`, sorted.
    pub(crate) fn needs_of(&self, key: &str) -> Vec<String> {
        self.fx.env.read(|c| {
            let mut q = c.prepare("SELECT needs FROM workflow_needs WHERE plan = 'pay' AND workflow = ?1 ORDER BY needs")?;
            let rows = q.query_map([key], |r| r.get::<_, String>(0))?.collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
    }

    fn phase(&self) -> String {
        self.fx
            .env
            .query("SELECT phase FROM plans WHERE id = 'pay'")
    }

    /// The `data` of the `workflow-added` Events of Plan `pay`, in order.
    pub(crate) fn pr_added(&self) -> Vec<Value> {
        let data: Vec<String> = self.fx.env.read(|c| {
            let mut stmt = c.prepare("SELECT data FROM events WHERE plan = 'pay' AND kind = 'workflow-added' ORDER BY id")?;
            stmt.query_map([], |r| r.get::<_, String>(0))?.collect()
        });
        data.iter()
            .map(|d| serde_json::from_str(d).unwrap())
            .collect()
    }

    pub(crate) fn add(&self, key: &str, repo: &Path, extra: &[&str]) -> Run {
        let mut args = vec![
            "workflow",
            "add",
            key,
            "--repo",
            repo.to_str().unwrap(),
            "--title",
            "Change the PR",
            "--shape",
            "config",
            "--plan",
            "pay",
        ];
        args.extend(extra);
        self.fx.cpd(&args)
    }

    fn add_fix(&self, extra: &[&str]) -> Run {
        self.add_queued("fix", &self.repo, extra)
    }

    /// [`Added::add`] with `--jobs`: after the lock cpd queues the template's Jobs, never stub drafts (pro-d404).
    fn add_queued(&self, key: &str, repo: &Path, extra: &[&str]) -> Run {
        let spec = json!({"goal": "Do it.", "context": "Because.", "touches": ["src/a.rs"], "acceptance": ["cargo test"], "out_of_scope": ["Everything else."]});
        let file = self.fx.env.root().join(format!("{key}-jobs.json"));
        fs::write(
            &file,
            json!({"config": {"tier": "S2", "spec": spec}}).to_string(),
        )
        .unwrap();
        self.fx.install_catalog();
        let mut args = vec!["--jobs", file.to_str().unwrap()];
        args.extend(extra);
        self.add(key, repo, &args)
    }

    /// An unmerged PR `side` in `repo`, as the lock leaves one (`dispatch`): a PR stacked on it (`--base pay/side`) waits for it, and
    /// so its stub drafts are legal after the lock (a PR that starts at once is refused `draft_parked`).
    fn side(&self, repo: &Path) {
        self.fx.env.sql(&format!(
            "INSERT INTO workflows (plan, key, repo, name, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at, started_at)
               VALUES ('pay', 'side', '{}', 'pay', 'main', 'dispatch', 'cpd/pay/side', '/w/side', 'Side', 'config', cpd_txn(), 't', 't');",
            repo.display()
        ));
    }
}

#[test]
fn pr_add_on_a_reopened_plan_needs_its_merged_prs_by_default() {
    let f = added("done");
    f.side(&f.repo);
    let out = f.add_fix(&["--base", "pay/side"]).ok();
    assert_eq!(out["workflow"]["needs"], json!(["repo"]));
    assert_eq!(f.needs_of("fix"), ["repo"]);
    assert_eq!(f.pr_added()[0]["needs"], json!(["repo"]));
    // A rerun (the Plan is in dispatch now) is `already`, not `workflow_exists`.
    assert_eq!(f.add_fix(&["--base", "pay/side"]).ok()["already"], true);
}

#[test]
fn pr_add_no_needs_and_needs_override_the_reopen_default() {
    // One in-process `Fx` at a time (its lock is process-wide): each block drops its own before the next.
    {
        let a = added("done");
        a.side(&a.repo);
        assert_eq!(
            a.add_fix(&["--no-needs", "--base", "pay/side"]).ok()["workflow"]["needs"],
            json!([])
        );
        assert!(a.needs_of("fix").is_empty());
    }
    {
        let b = added("done");
        b.side(&b.repo);
        b.add_queued("x", &b.repo, &["--no-needs", "--base", "pay/side"])
            .ok();
        // The Plan is in dispatch now; `--needs x` names a PR that is not merged.
        assert_eq!(
            b.add_fix(&["--needs", "x"]).ok()["workflow"]["needs"],
            json!(["x"])
        );
        assert_eq!(b.needs_of("fix"), ["x"]);
    }
    {
        let c = added("done");
        assert_eq!(
            code(&c.add_fix(&["--needs", "repo", "--no-needs"])),
            ("usage".into(), 2)
        );
        assert_eq!(c.phase(), "done");
    }
}

#[test]
fn pr_add_on_a_plan_in_dispatch_with_no_needs_has_no_needs() {
    let f = added("dispatch");
    // Another repo: the fixture's own holds `cpd/pay` for its unmerged PR.
    let other = f.fx.keyed_repo("other").canonicalize().unwrap();
    f.side(&other);
    let out = f
        .add_queued("fix", &other, &["--no-needs", "--base", "pay/side"])
        .ok();
    assert_eq!(out["workflow"]["needs"], json!([]));
    assert!(f.needs_of("fix").is_empty());
}

// ------------------------------------------------------------ plan reviews

/// Inserts pending reviews r1 and r2 of `plan` (fingerprints all `a` and all `b`).
fn insert_pending_reviews(env: &TestEnv, plan: &str) {
    env.sql(&format!(
        "INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at) VALUES ('{plan}', 'r1', 1, '{}', 'pending', 't');
         INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at) VALUES ('{plan}', 'r2', 2, '{}', 'pending', 't');",
        "a".repeat(64),
        "b".repeat(64)
    ));
}

/// Asserts r1 and r2 of `plan` are superseded and their `plan-review` Events come, in order, before the one `plan-done`.
fn assert_reviews_superseded_before_done(env: &TestEnv, plan: &str) {
    assert_eq!(
        env.query::<String>(&format!("SELECT phase FROM plans WHERE id = '{plan}'")),
        "done"
    );
    for (id, fingerprint) in [("r1", "a"), ("r2", "b")] {
        assert_eq!(
            env.query::<String>(&format!(
                "SELECT status || '|' || (decided_at IS NOT NULL) || '|' || (feedback IS NULL) || '|' || fingerprint FROM plan_reviews WHERE plan = '{plan}' AND id = '{id}'"
            )),
            format!("superseded|1|1|{}", fingerprint.repeat(64)),
            "{id}"
        );
    }
    let reviews: Vec<String> = env.read(|c| {
        let mut stmt = c.prepare(&format!(
            "SELECT data FROM events WHERE plan = '{plan}' AND kind = 'plan-review' ORDER BY id"
        ))?;
        stmt.query_map([], |r| r.get::<_, String>(0))?.collect()
    });
    let reviews: Vec<Value> = reviews
        .iter()
        .map(|d| serde_json::from_str(d).unwrap())
        .collect();
    assert_eq!(
        reviews,
        [
            json!({"review": "r1", "fingerprint": "a".repeat(64), "status": "superseded"}),
            json!({"review": "r2", "fingerprint": "b".repeat(64), "status": "superseded"})
        ]
    );
    assert_eq!(
        env.query::<i64>(&format!(
            "SELECT count(*) FROM events WHERE plan = '{plan}' AND kind = 'plan-done'"
        )),
        1
    );
    assert_eq!(
        env.query::<i64>(&format!(
            "SELECT (SELECT max(id) FROM events WHERE plan = '{plan}' AND kind = 'plan-review') < (SELECT id FROM events WHERE plan = '{plan}' AND kind = 'plan-done')"
        )),
        1
    );
}

// ------------------------------------------------------------------ the update Job on a merged base (hs-d794, hs-n617)
//
// The regression test of the widened trigger: a stacked PR whose base PR merged and whose base GitHub repointed to the
// default branch, with main ahead of the Lead Branch by the base fix and no conflict. The poll queues one `update` Job,
// `reason` `base-merged`; its Worker merges `origin/main` into the Lead Branch even though there is no conflict, and the
// red-test Job of the stacked PR then lists its test as found.

/// A `node` with GitHub's `baseRefName` and the repo's default branch, as the poll's search prints them (`pr_stacked_ready`'s
/// own, private there): the poll needs both to follow a repoint and to know the new base is the default branch.
fn repointed(node: String, base: &str, default: &str) -> String {
    let node = based(node, base);
    let node = node.replacen(
        r#","isArchived":false},"number":"#,
        &format!(r#","isArchived":false,"defaultBranchRef":{{"name":"{default}"}}}},"number":"#),
        1,
    );
    assert!(node.contains("defaultBranchRef"), "{node}");
    node
}

/// 814, the stacked PR's pull request, with its base and the repo's default branch GitHub reports.
fn web_repointed(base: &str, default: &str) -> String {
    repointed(up("OPEN"), base, default)
}

/// The repo `acme/pay`'s `[test-commands]` of the regression: `list` finds `t::one` only when the base fix (`harness.txt`)
/// is in the worktree, and `run` fails, so the `test` Job is red.
const REGRESS_TESTS: &str = "[test-commands]\n\
    build = \"build\"\n\
    list = \"list {{ test }}\"\n\
    run = \"run {{ tests }}\"\n";

/// The answer of `sh -c <command>` for [`REGRESS_TESTS`]: `build` always passes, `list t::one` passes only once
/// `harness.txt` (the base fix) exists in the worktree, `list` of another name fails, and `run` fails (a red test).
fn red_tests_answer(call: &Call) -> Reply {
    let command = call.args[1].clone();
    let cwd = call
        .cwd
        .clone()
        .expect("a [test-commands] command runs in a directory");
    match command.as_str() {
        "build" => Reply::ok("building\n"),
        "list t::one" => Reply::exit(i32::from(!cwd.join("harness.txt").exists()), "", ""),
        "run t::one" => Reply::exit(1, "running\n", ""),
        other => panic!("a [test-commands] command nothing scripted: {other}"),
    }
}

impl Poll {
    /// The Lead worktree path of PR `key` of Plan `pay`.
    fn lead_worktree_of(&self, key: &str) -> PathBuf {
        let path: String = self
            .store()
            .read(|c| {
                Ok(c.query_row(
                    "SELECT lead_worktree FROM workflows WHERE plan = 'pay' AND key = ?1",
                    [key],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        PathBuf::from(path)
    }

    /// Cuts Attempt `<job>-a1` for a running `test` Job of PR `key` - its red test named `t::one` in `tests/t.rs` - from
    /// the PR's Lead worktree and commits the red test. Returns the Attempt worktree; [`Poll::submit_red`] submits it.
    fn red_test_attempt(&self, key: &str, job: &str) -> PathBuf {
        self.red_test_attempt_number(key, job, 1)
    }

    /// [`Poll::red_test_attempt`] for the Job's `number`-th Attempt: the Job row is written on the first only.
    fn red_test_attempt_number(&self, key: &str, job: &str, number: i64) -> PathBuf {
        let lead = self.lead_worktree_of(key);
        let attempt = AttemptId::parse(&format!("{job}-a{number}")).unwrap();
        let setup = Setup {
            timeout: Duration::from_secs(30),
            log: self.env.root().join(format!("red-{number}.log")),
        };
        let made = self
            .scoped(|| create_attempt_worktree(&self.repo, &lead, &plan(), &attempt, &setup))
            .unwrap();
        self.store()
            .write("fixture", |tx| {
                tx.execute(
                    "INSERT OR IGNORE INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                                       status, appended, fingerprint, created_txn, created_at, kind, tests, test_files)
                     VALUES ('pay', ?4, ?1, 'Red', 'S2', 'g', 'c', '[\"tests/t.rs\"]', '[\"b\"]', '[]', 'running', 0, ?2, ?3, 't', 'test', ?5, ?6)",
                    params![job, "f".repeat(64), tx.id(), key, r#"["t::one"]"#, r#"["tests/t.rs"]"#],
                )?;
                tx.execute(
                    "INSERT INTO attempts (plan, id, job, number, tier, origin, status, branch, worktree, base_sha, started_at)
                     VALUES ('pay', ?1, ?2, ?3, 'S2', 'ladder', 'running', ?4, ?5, ?6, 't')",
                    params![attempt.to_string(), job, number, made.branch, made.path.to_str().unwrap(), made.base_sha],
                )?;
                Ok(())
            })
            .unwrap();
        self.commit_file(
            &made.path,
            "tests/t.rs",
            "// the red test t::one\n",
            "the red test",
        );
        made.path
    }

    /// Ends the Attempt `id` `failed` (`cause` `no_submit`): the blocked Job stops running, so no Attempt of the PR runs.
    fn end_attempt(&self, id: &str) {
        self.store()
            .write("fixture", |tx| {
                tx.execute("UPDATE attempts SET status = 'ended', outcome = 'failed', cause = 'no_submit', ended_at = 't' WHERE plan = 'pay' AND id = ?1", [id])?;
                Ok(())
            })
            .unwrap();
    }

    /// `cpd attempt submit red-a1 --plan pay` in the red-test Attempt's worktree.
    fn submit_red(&self, worktree: &Path) -> Run {
        self.submit_attempt(worktree, "red-a1")
    }

    /// `cpd attempt submit <attempt> --plan pay` in the Attempt's worktree.
    fn submit_attempt(&self, worktree: &Path, attempt: &str) -> Run {
        self.cpd_in(worktree, &["attempt", "submit", attempt, "--plan", "pay"])
    }
}

/// A stacked PR's red-test Job lists its test as found once the base fix merges to main (hs-d794, hs-n617): the base PR
/// `repo` merged and GitHub repointed the stacked pull request `web` to the default branch `main`, which holds the base
/// fix and no conflict with `web`'s Lead Branch. The poll queues one `update` Job, `reason` `base-merged`; running it
/// merges `origin/main` into the Lead Branch even though there is no conflict; and the `test` Job of `web` then lists its
/// test `t::one` as found, with no hand-merge by the Lead.
#[test]
fn a_stacked_red_test_job_lists_its_test_as_found_once_the_base_fix_merges_to_main() {
    let fx = poll_fx_stacked("deploy");
    poll_stacked_pr(&fx);
    fx.mark_opened("web", 814);
    // The repo runs the repo's own `[test-commands]`: no real build, no real cargo, no real git.
    fx.env.repo_policy("acme", "pay", REGRESS_TESTS);
    fx.sh(red_tests_answer);

    // The base fix reaches `main`; `web`'s Lead Branch was cut from the base PR's Lead Branch before it: it lacks the fix.
    let fix = fx.advance_base("harness.txt", "the base fix\n");
    let web = fx.lead_worktree_of("web");
    assert_eq!(
        fx.git(&fx.repo, &["rev-parse", "origin/main"]),
        fix,
        "the base fix is on main on the remote"
    );
    assert!(
        !fx.git_output(&web, &["cat-file", "-e", "HEAD:harness.txt"])
            .status
            .success(),
        "the Lead Branch lacks the base fix before the update Job"
    );

    // The red-test Job of `web` is blocked on the base fix: its `list` finds no `t::one`, and cpd refuses the submit
    // `tests_not_found`. The Lead's hand-merge of `origin/main` is the defect's deadlock and must not be needed.
    let red = fx.red_test_attempt("web", "red");
    let error = common::refused(&fx.submit_red(&red), "tests_not_found", 3);
    assert_eq!(error["details"]["tests"], json!(["t::one"]));
    assert_eq!(
        fx.status_of_attempt("red-a1"),
        "running",
        "the blocked Job stays running"
    );

    // The base PR merged: 812 left the open search and GitHub repointed 814 to `main`.
    fx.search(&[web_repointed("main", "main")]);
    fx.lookup(&[Some(pay("MERGED", None, None))]);

    let out = fx.poll();

    // The base follow learns the repoint, and the poll queues exactly one update Job, `reason` `base-merged`.
    assert_eq!(
        out["base_changed"],
        json!([{"plan": "pay", "pr": "web", "from": fx.branch, "to": "main"}])
    );
    assert_eq!(
        fx.update_jobs(),
        [("up-web".to_owned(), "queued".to_owned())]
    );
    let added: Vec<_> = fx
        .events("job-added")
        .into_iter()
        .filter(|(_, data)| data["workflow"] == json!("web"))
        .collect();
    assert_eq!(added.len(), 1, "{added:?}");
    assert_eq!(
        (
            &added[0].1["kind"],
            &added[0].1["by"],
            &added[0].1["reason"]
        ),
        (&json!("update"), &json!("poll"), &json!("base-merged"))
    );
    assert_eq!(
        fx.workflow_status("web"),
        "open",
        "the repoint changes no status"
    );

    // Running that Job: its Worker merges `origin/main` into the Lead Branch even though the two do not conflict.
    fx.git(&web, &["fetch", "-q", "origin", "main"]);
    fx.git(&web, &["merge", "-q", "--no-edit", "origin/main"]);
    fx.git(&web, &["push", "-q", "origin", "cpd/pay/web"]);
    assert!(
        fx.git_output(&web, &["cat-file", "-e", "HEAD:harness.txt"])
            .status
            .success(),
        "the Lead Branch holds the base fix after the update Job"
    );

    // The red-test Job takes the Lead Branch's new tip into its Attempt and submits again; its `list` now finds `t::one`,
    // and the Job passes (its `run` is red).
    fx.git(&red, &["merge", "-q", "--no-edit", "cpd/pay/web"]);
    let submitted = fx.submit_red(&red).ok();
    assert_eq!(submitted["submitted"], json!(true), "{submitted}");
    let lists: Vec<String> = fx
        .calls("sh")
        .into_iter()
        .map(|call| call.args[1].clone())
        .filter(|command| command.starts_with("list"))
        .collect();
    assert_eq!(
        lists,
        ["list t::one", "list t::one"],
        "the Job's list ran before and after the base fix"
    );
    assert!(
        red.join("harness.txt").exists(),
        "the Attempt worktree holds the base fix"
    );
    assert_eq!(
        fx.status_of_attempt("red-a1"),
        "merging",
        "the red-test Job submitted once its test was found"
    );
}

impl Poll {
    /// The status of Attempt `id` of Plan `pay`.
    fn status_of_attempt(&self, id: &str) -> String {
        self.store()
            .read(|c| {
                Ok(c.query_row(
                    "SELECT status FROM attempts WHERE plan = 'pay' AND id = ?1",
                    [id],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    }
}

/// A pre-open (`dispatch`) PR's blocked red-test Job submits once a pre-open `pr push` carries a main-side fix
/// (hs-d801, hs-n636): the tdd submit check builds cpd from the branch's sources, so the Job's `list t::one` finds
/// its test only when the fix (`harness.txt`) is in the Lead Branch, which pre-open only `pr push` can publish. With
/// no push, the fix stays in the Lead worktree and the submit stays `tests_not_found`.
#[test]
fn a_preopen_push_lets_a_blocked_test_job_submit() {
    let fx = poll_fx("dispatch");
    // The Gate's `pre-merge` and the Job's `[test-commands]` share the one fake `sh`.
    fx.env.repo_policy(
        "acme",
        "pay",
        &format!("{REGRESS_TESTS}[hooks]\npre-merge = \"pre-merge\"\n"),
    );
    fx.sh(|call| {
        if call.args[1] == "pre-merge" {
            Reply::ok("")
        } else {
            red_tests_answer(call)
        }
    });

    // The base fix reaches `main` on `origin`; the pre-open PR's Lead Branch, cut from before it, lacks it.
    let fix = fx.advance_base("harness.txt", "the base fix\n");
    let lead = fx.lead_worktree_of("repo");
    assert_eq!(
        fx.git(&fx.repo, &["rev-parse", "origin/main"]),
        fix,
        "the base fix is on main on the remote"
    );
    assert!(
        !fx.git_output(&lead, &["cat-file", "-e", "HEAD:harness.txt"])
            .status
            .success(),
        "the Lead Branch lacks the base fix before the push"
    );

    // The red-test Job is blocked: its `list t::one` finds nothing, and cpd refuses the submit `tests_not_found`.
    let red = fx.red_test_attempt("repo", "red");
    let error = common::refused(&fx.submit_red(&red), "tests_not_found", 3);
    assert_eq!(error["details"]["tests"], json!(["t::one"]));

    // The blocked Attempt ends (the Job is failed, not retried): the push lands between Attempts (hs-d801).
    fx.end_attempt("red-a1");

    // The Lead merges the fix into the Lead Branch (no hand-push), then a pre-open `pr push` Gates and pushes it.
    fx.git(&lead, &["fetch", "-q", "origin", "main"]);
    fx.git(&lead, &["merge", "-q", "--no-edit", "origin/main"]);
    let pushed = fx.lead_sha();
    let out = fx.pr("push").ok();
    assert_eq!(out["pushed"], json!(true), "{out}");
    assert_eq!(out["sha"], json!(pushed), "{out}");
    assert_eq!(
        fx.remote_branch("cpd/pay"),
        Some(pushed.clone()),
        "the fix reached origin through the pre-open push"
    );

    // The Job's next Attempt is cut from the pushed Lead Branch tip, so its `list t::one` finds the test and the
    // submit is accepted (a `test` Job; its `run` is red).
    let again = fx.red_test_attempt_number("repo", "red", 2);
    let submitted = fx.submit_attempt(&again, "red-a2").ok();
    assert_eq!(submitted["submitted"], json!(true), "{submitted}");
    assert!(
        again.join("harness.txt").exists(),
        "the Attempt worktree holds the base fix"
    );
    assert_eq!(
        fx.status_of_attempt("red-a2"),
        "merging",
        "the blocked red-test Job submitted once its test was found"
    );
}
