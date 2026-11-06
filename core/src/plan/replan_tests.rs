//! Red tests of the guards keyed on `locked_at` and of the return of a re-planned Plan to dispatch (pro-d429, pro-d433;
//! assumptions A1-A6 of Plan `explore-phase-plan`). A re-planned Plan is `phase = 'plan'` with `locked_at` set: the schema
//! forbids that row today, so the fixture seeds it past the CHECK (as `finish_legacy_review_plans_tests` does) and each test
//! then fails on the behaviour it pins.
//! In process on the janitor's `Rig`: git is the `Universe`, `cpd watch --ensure` a scripted reply, the store a temp database.

use clap::Parser;
use serde_json::Value;

use crate::error::{CpdError, ErrorCode};
use crate::host::runner::Reply;
use crate::ids::PlanId;
use crate::janitor_tests::Rig;
use crate::output::Output;
use crate::plan::fingerprint::plan_fingerprint;
use crate::ports::Ports;
use crate::store::Store;

/// `CPD_HOME` is the process's: one test at a time.
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The first lock's time: a re-plan keeps it.
const LOCKED: &str = "2026-01-01T00:00:00.000Z";

#[derive(Parser)]
struct PlanCli {
    #[command(subcommand)]
    command: crate::plan::Command,
}

#[derive(Parser)]
struct WorkflowCli {
    #[command(subcommand)]
    command: crate::workflow::WorkflowCommand,
}

/// A re-planned Plan `pay`: phase `plan`, `locked_at` set; Workflow `api` in dispatch with a queued and a running Job, Workflow
/// `old` merged with a merged Job. `drafts` adds a complete draft Job `api-d` to `api` (a decided-work append).
fn replanned(drafts: bool) -> Fx {
    let guard = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut rig = Rig::new();
    rig.workflow_with("api", "dispatch", "queued", "failed");
    rig.workflow_with("old", "merged", "merged", "merged");
    rig.workflow_with("side", "dispatch", "queued", "failed");
    rig.sql(&format!(
        "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                           status, appended, fingerprint, created_txn, created_at)
           VALUES ('pay', 'api', 'api-run', 'R', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'running', 0, '{fp}', cpd_txn(), 't');",
        fp = "f".repeat(64)
    ));
    if drafts {
        rig.sql(
            "INSERT INTO jobs (plan, workflow, key, title, tier, goal, context, touches, acceptance, out_of_scope,
                               kind, status, appended, created_txn, created_at)
               VALUES ('pay', 'api', 'api-d', 'D', 'S2', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', 'work', 'draft', 1, cpd_txn(), 't');",
        );
    }
    rig.sql(&format!(
        "PRAGMA ignore_check_constraints = ON;
         UPDATE plans SET phase = 'plan', locked_at = '{LOCKED}', done_at = NULL WHERE id = 'pay';
         PRAGMA ignore_check_constraints = OFF;"
    ));
    (*rig.fakes.runner)
        .clone()
        .on("cpd", |_| true, Reply::ok(r#"{"ok":true}"#));
    let owner = rig
        .home
        .config()
        .parent()
        .unwrap()
        .join("policy")
        .join("acme");
    std::fs::create_dir_all(&owner).unwrap();
    std::fs::write(owner.join("owner.toml"), "").unwrap();
    rig.sql(
        r#"UPDATE plans SET outline = '{"assumptions":[],"decided":[],"decisions":[],"done_when":{"api":["works"]},"goal":"g","out_of_scope":[],"risks":[]}' WHERE id = 'pay';"#,
    );
    unsafe { std::env::set_var("CPD_HOME", rig.home.root()) };
    Fx { rig, _guard: guard }
}

/// The Rig, and the hold on the process environment.
struct Fx {
    rig: Rig,
    _guard: std::sync::MutexGuard<'static, ()>,
}

impl std::ops::Deref for Fx {
    type Target = Rig;
    fn deref(&self) -> &Rig {
        &self.rig
    }
}

fn plan_id() -> PlanId {
    PlanId::parse("pay").unwrap()
}

fn plan_cmd(rig: &Rig, args: &[&str]) -> Result<Output, CpdError> {
    let argv = ["cpd"].into_iter().chain(args.iter().copied());
    let command = PlanCli::parse_from(argv).command;
    Ports::scope(&rig.fakes.ports, || crate::plan::run(command))
}

fn workflow_cmd(rig: &Rig, args: &[&str]) -> Result<Output, CpdError> {
    let argv = ["cpd"].into_iter().chain(args.iter().copied());
    let command = WorkflowCli::parse_from(argv).command;
    Ports::scope(&rig.fakes.ports, || crate::workflow::run_workflow(command))
}

fn fields(out: Result<Output, CpdError>) -> Value {
    match out.unwrap_or_else(|e| panic!("expected a success, got {e}")) {
        Output::Success(f) => Value::Object(f),
        Output::Owned => panic!("a success"),
    }
}

fn refused(out: Result<Output, CpdError>) -> CpdError {
    match out {
        Ok(_) => panic!("expected a refusal, got a success"),
        Err(e) => e,
    }
}

/// Rows of `sql` as one string per row, columns joined by `|` (NULL as `-`).
fn rows(rig: &Rig, sql: &str) -> Vec<String> {
    let mut store = Store::open(&rig.home, 5000).unwrap();
    store
        .read(|c| {
            let mut stmt = c.prepare(sql)?;
            let n = stmt.column_count();
            let rows = stmt.query_map([], |r| {
                Ok((0..n)
                    .map(|i| match r.get_ref(i).unwrap() {
                        rusqlite::types::ValueRef::Null => "-".to_owned(),
                        rusqlite::types::ValueRef::Integer(v) => v.to_string(),
                        rusqlite::types::ValueRef::Text(t) => {
                            String::from_utf8_lossy(t).into_owned()
                        }
                        _ => "?".to_owned(),
                    })
                    .collect::<Vec<_>>()
                    .join("|"))
            })?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .unwrap()
}

/// `phase|locked_at|done_at is null` of Plan `pay`.
fn plan_row(rig: &Rig) -> String {
    rows(
        rig,
        "SELECT phase || '|' || coalesce(locked_at, '-') || '|' || (done_at IS NULL) FROM plans WHERE id = 'pay'",
    )
    .remove(0)
}

/// The plan-phase Events as `from>to`, in order.
fn phase_events(rig: &Rig) -> Vec<String> {
    rows(
        rig,
        "SELECT json_extract(data, '$.from') || '>' || json_extract(data, '$.to') FROM events WHERE kind = 'plan-phase' ORDER BY id",
    )
}

fn event_count(rig: &Rig, kind: &str) -> usize {
    rows(
        rig,
        &format!("SELECT kind FROM events WHERE kind = '{kind}'"),
    )
    .len()
}

/// What a lock, an approval or an add must leave alone: every Workflow's status, start and Lead worktree, every started Job.
fn untouched(rig: &Rig) -> Vec<String> {
    let mut all = rows(
        rig,
        "SELECT 'w:' || key, status, coalesce(started_at, '-'), coalesce(lead_worktree, '-') FROM workflows ORDER BY key",
    );
    all.extend(rows(
        rig,
        "SELECT 'j:' || key, status, coalesce(fingerprint, '-') FROM jobs WHERE status <> 'draft' ORDER BY key",
    ));
    all
}

/// `after` limited to the rows of what `before` lists: the draft a lock or an approval queues is new, not restarted.
fn same_as(before: &[String], after: Vec<String>) -> Vec<String> {
    let id = |row: &str| row.split('|').next().unwrap_or_default().to_owned();
    let known: Vec<String> = before.iter().map(|r| id(r)).collect();
    after
        .into_iter()
        .filter(|r| known.contains(&id(r)))
        .collect()
}

/// An approved review of the current drafts.
fn approve_current(rig: &Rig) {
    let mut store = Store::open(&rig.home, 5000).unwrap();
    let fp = store.read(|c| plan_fingerprint(c, &plan_id())).unwrap();
    rig.sql(&format!(
        "INSERT INTO plan_reviews (plan, id, number, fingerprint, title, status, requested_at, decided_at)
           VALUES ('pay', 'r1', 1, '{fp}', 'T', 'approved', 't', 't');"
    ));
}

fn pending_review(rig: &Rig) {
    let mut store = Store::open(&rig.home, 5000).unwrap();
    let fp = store.read(|c| plan_fingerprint(c, &plan_id())).unwrap();
    rig.sql(&format!(
        "INSERT INTO plan_reviews (plan, id, number, fingerprint, title, status, requested_at)
           VALUES ('pay', 'r1', 1, '{fp}', 'T', 'pending', 't');"
    ));
}

#[test]
fn plan_lock_on_a_replanned_plan_returns_it_to_dispatch_without_restarting_workflows() {
    let rig = replanned(true);
    approve_current(&rig);
    let before = untouched(&rig);

    let out = fields(plan_cmd(&rig, &["lock", "--plan", "pay"]));

    assert_eq!(out["phase"], "dispatch");
    assert_eq!(
        plan_row(&rig),
        format!("dispatch|{LOCKED}|1"),
        "locked_at is kept"
    );
    assert_eq!(
        phase_events(&rig),
        ["plan>dispatch"],
        "one plan-phase Event"
    );
    assert_eq!(
        rows(&rig, "SELECT status FROM jobs WHERE key = 'api-d'"),
        ["queued"],
        "the approved draft is queued"
    );
    assert_eq!(
        same_as(&before, untouched(&rig)),
        before,
        "started Workflows and their Jobs are left as they are"
    );
    assert_eq!(
        event_count(&rig, "workflow-started"),
        0,
        "no Workflow is started again"
    );
}

#[test]
fn plan_lock_on_a_replanned_plan_without_drafts_needs_no_review() {
    // Open Jobs left: the Plan returns to dispatch, nothing else moves.
    let rig = replanned(false);
    let before = untouched(&rig);
    let out = fields(plan_cmd(&rig, &["lock", "--plan", "pay"]));
    assert_eq!(out["phase"], "dispatch");
    assert_eq!(plan_row(&rig), format!("dispatch|{LOCKED}|1"));
    assert_eq!(phase_events(&rig), ["plan>dispatch"]);
    assert_eq!(untouched(&rig), before);
    drop(rig);

    // Every Workflow merged and every Job final: the lock settles the Plan, which ends done (A4, A6).
    let rig = replanned(false);
    rig.sql(
        "UPDATE jobs SET status = 'running' WHERE plan = 'pay' AND status = 'queued';
         UPDATE jobs SET status = 'merged', merged_sha = 'abc' WHERE plan = 'pay' AND status = 'running';
         PRAGMA ignore_check_constraints = ON;
         UPDATE workflows SET status = 'open' WHERE key IN ('api', 'side');
         UPDATE workflows SET status = 'merged', merged_at = 't', pr_number = 7, pr_url = 'u', pr_opened_at = 't' WHERE key IN ('api', 'side');
         PRAGMA ignore_check_constraints = OFF;",
    );
    fields(plan_cmd(&rig, &["lock", "--plan", "pay"]));
    let row = plan_row(&rig);
    assert!(row.starts_with(&format!("done|{LOCKED}|0")), "{row}");
    assert_eq!(event_count(&rig, "plan-done"), 1);
    assert_eq!(
        phase_events(&rig),
        ["plan>dispatch"],
        "no plan-phase Event for the dispatch -> deploy step of settle_done (A6)"
    );
}

#[test]
fn plan_lock_on_a_replanned_plan_with_drafts_still_needs_an_approved_review() {
    let rig = replanned(true);
    let before = untouched(&rig);
    let e = refused(plan_cmd(&rig, &["lock", "--plan", "pay"]));
    assert_eq!(e.code(), ErrorCode::PlanNotApproved, "{e}");
    assert_eq!(plan_row(&rig), format!("plan|{LOCKED}|1"));
    assert_eq!(
        rows(&rig, "SELECT status FROM jobs WHERE key = 'api-d'"),
        ["draft"]
    );
    assert_eq!(untouched(&rig), before);
    assert!(phase_events(&rig).is_empty());

    // With the review approved for the current drafts the same lock goes through.
    approve_current(&rig);
    fields(plan_cmd(&rig, &["lock", "--plan", "pay"]));
    assert_eq!(plan_row(&rig), format!("dispatch|{LOCKED}|1"));
}

#[test]
fn approving_drafts_on_a_replanned_plan_queues_them_and_returns_it_to_dispatch() {
    let rig = replanned(true);
    pending_review(&rig);
    let before = untouched(&rig);

    let out = fields(plan_cmd(
        &rig,
        &["decide", "r1", "--approve", "--plan", "pay"],
    ));

    assert_eq!(out["jobs"], serde_json::json!(["api-d"]));
    assert_eq!(
        rows(&rig, "SELECT status FROM jobs WHERE key = 'api-d'"),
        ["queued"]
    );
    assert_eq!(plan_row(&rig), format!("dispatch|{LOCKED}|1"));
    assert_eq!(
        phase_events(&rig),
        ["plan>dispatch"],
        "one plan-phase Event"
    );
    assert_eq!(same_as(&before, untouched(&rig)), before);
}

#[test]
fn workflow_add_on_a_replanned_plan_returns_it_to_dispatch() {
    let rig = replanned(false);
    // After the lock an appended Workflow lays its template out as decided work (pro-d404): `--jobs` gives it.
    let jobs = rig.home.root().join("jobs.json");
    std::fs::write(
        &jobs,
        r#"{"docs": {"tier": "S2", "spec": {"goal": "g", "context": "c", "touches": ["docs/a.md"], "acceptance": ["b"], "out_of_scope": []}}}"#,
    )
    .unwrap();
    let jobs = jobs.display().to_string();
    // The model catalog: a `pi` install laid out in a temp dir (on PATH), `node` a scripted reply.
    let agent = rig.home.root().join("agent");
    let release = agent.join("install/releases/1.0.0/node_modules/@earendil-works");
    for entry in ["pi-coding-agent", "pi-ai"] {
        std::fs::create_dir_all(release.join(entry).join("dist")).unwrap();
        std::fs::write(release.join(entry).join("dist/index.js"), "").unwrap();
    }
    std::fs::write(agent.join("install/current-version"), "1.0.0\n").unwrap();
    std::fs::create_dir_all(agent.join("bin")).unwrap();
    let pi = agent.join("bin/pi");
    std::fs::write(&pi, "").unwrap();
    std::fs::set_permissions(&pi, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let path = std::env::var_os("PATH");
    unsafe { std::env::set_var("PATH", agent.join("bin")) };
    (*rig.fakes.runner).clone().on(
        "node",
        |_| true,
        Reply::ok(r#"{"O":["off","low","medium","high"],"S":["off","low","medium","high"],"H":["off","low","medium","high"],"G":["off"]}"#),
    );
    let repo = rig.repo.dir().display().to_string();
    let base = [
        "add",
        "web",
        "--repo",
        repo.as_str(),
        "--title",
        "Web",
        "--shape",
        "docs",
        "--plan",
        "pay",
    ];

    // As after the lock: a Workflow with neither needs nor a stacked base is refused, and nothing is written.
    let e = refused(workflow_cmd(&rig, &base));
    assert_eq!(e.code(), ErrorCode::NeedsRequired, "{e}");
    assert_eq!(plan_row(&rig), format!("plan|{LOCKED}|1"));
    assert!(phase_events(&rig).is_empty());

    let before = untouched(&rig);
    let mut with_needs = base.to_vec();
    with_needs.extend(["--needs", "old", "--jobs", jobs.as_str()]);
    let out = fields(workflow_cmd(&rig, &with_needs));
    assert_eq!(out["workflow"]["appended"], true, "{out}");
    assert_eq!(plan_row(&rig), format!("dispatch|{LOCKED}|1"));
    assert_eq!(
        phase_events(&rig),
        ["plan>dispatch"],
        "one plan-phase Event"
    );
    let after = untouched(&rig);
    assert!(
        before.iter().all(|row| after.contains(row)),
        "nothing started is touched: {after:?}"
    );
    assert_eq!(
        rows(&rig, "SELECT status FROM workflows WHERE key = 'api'"),
        ["dispatch"],
        "the started Workflow is left as it is"
    );
    match path {
        Some(path) => unsafe { std::env::set_var("PATH", path) },
        None => unsafe { std::env::remove_var("PATH") },
    }
}

#[test]
fn plan_review_and_cancel_follow_locked_at_in_a_replanned_plan() {
    let rig = replanned(true);

    // `workflow cancel` of a started Workflow is allowed.
    let out = fields(workflow_cmd(&rig, &["cancel", "side", "--plan", "pay"]));
    assert_eq!(out["workflow"]["status"], "cancelled");
    assert_eq!(
        rows(&rig, "SELECT status FROM workflows WHERE key = 'side'"),
        ["cancelled"]
    );

    // `workflow edit`: after the lock only a restack of a waiting Workflow passes, so a change of needs alone is refused...
    let repo = rig.repo.dir().display();
    rig.sql(&format!(
        "INSERT INTO workflows (plan, key, repo, base, status, lead_branch, created_txn, created_at)
           VALUES ('pay', 'web', '{repo}', 'main', 'waiting', 'pay/web', cpd_txn(), 'u');
         INSERT INTO workflow_needs (plan, workflow, needs) VALUES ('pay', 'web', 'api');"
    ));
    let e = refused(workflow_cmd(
        &rig,
        &["edit", "web", "--needs", "old", "--plan", "pay"],
    ));
    assert_eq!(e.code(), ErrorCode::WorkflowLocked, "{e}");
    // ...and the restack onto the Workflow it needed works.
    fields(workflow_cmd(
        &rig,
        &[
            "edit",
            "web",
            "--base",
            "api",
            "--no-needs",
            "--plan",
            "pay",
        ],
    ));
    assert_eq!(
        rows(
            &rig,
            "SELECT needs FROM workflow_needs WHERE workflow = 'web'"
        ),
        Vec::<String>::new()
    );

    // `plan review` is accepted: the Plan is past the lock, not in grounding.
    let out = fields(plan_cmd(
        &rig,
        &["review", "--title", "Pay", "--plan", "pay"],
    ));
    assert_eq!(out["review"]["status"], "pending");
}
