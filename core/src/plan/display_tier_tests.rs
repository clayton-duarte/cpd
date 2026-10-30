//! Red tests for the Tier a Job is shown at (docs/commands.md 13.2, 4.5, 5.4): `Display::job_tier`, one derivation:
//! the active Attempt's Tier, else `retry_tier` while a `job retry` has no Attempt since, else the latest Attempt's Tier,
//! else `tier`; a stub draft with no `tier` and no Attempt has none. Every test goes through `Display::load` over a store.

use crate::home::CpdHome;
use crate::ids::PlanId;
use crate::plan::display::Display;
use crate::plan::fingerprint::load_prs;
use crate::plan::jobs;
use crate::plan::render::{Heading, markdown};
use crate::store::Store;

/// One Attempt row: (number, tier, origin, `None` for active (`running`) or the cause an ended one failed with).
type Att = (u32, &'static str, &'static str, bool);

fn attempt(job: &str, (n, tier, origin, ended): Att) -> String {
    let (status, outcome, tail) = if ended { ("ended", "'failed'", ", 'timed_out', 't'") } else { ("running", "NULL", ", NULL, NULL") };
    format!(
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, ended_at, branch, worktree, started_at)
         VALUES ('pl', '{job}-a{n}', '{job}', {n}, '{tier}', '{origin}', '{status}', {outcome}{tail}, 'cpd/pl--{job}-a{n}', '/w', 't');"
    )
}

fn event(kind: &str, job: &str) -> String {
    format!("INSERT INTO events (txn, at, kind, context, plan, job, text, data) VALUES (cpd_txn(), 't', '{kind}', 'ctx', 'pl', '{job}', 'x', '{{}}');")
}

/// A store holding the Plan `p` with its PR `r` (`dispatch`) and the Job `a` (`status`, `tier`, `retry_tier`), then `extra` SQL.
fn store_with(status: &str, tier: Option<&str>, retry_tier: Option<&str>, extra: &str) -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    let q = |t: Option<&str>| t.map_or("NULL".to_owned(), |t| format!("'{t}'"));
    let spec = if tier.is_some() { "'g', 'c', '[\"a\"]', '[\"b\"]', '[]'" } else { "NULL, NULL, NULL, NULL, NULL" };
    store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead', 'lead', 'ctx', 'pl', 'Lead', '/', 'O3', 'working', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at) VALUES ('pl', 'ctx', 'P', 'dispatch', 'lead', 't', 't');
                 INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
                   VALUES ('pl', 'r', '/r', 'main', 'dispatch', 'cpd/pl', '/w', cpd_txn(), 't', 't');
                 INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, failed_cause, retry_tier, created_txn, created_at)
                   VALUES ('pl', 'r', 'a', 'Retried', {}, {spec}, '{status}', 0, {}, {}, {}, cpd_txn(), 't');
                 {extra}",
                q(tier),
                if status == "draft" { "NULL".to_owned() } else { format!("'{}'", "f".repeat(64)) },
                if status == "failed" { "'timed_out'" } else { "NULL" },
                q(retry_tier)
            ))?;
            Ok(())
        })
        .unwrap();
    (dir, store)
}

/// The Tier `Job a` is shown at, read through `Display::load`.
fn shown(store: &mut Store) -> Option<String> {
    let plan = PlanId::parse("pl").unwrap();
    store
        .read(|conn| {
            let display = Display::load(conn, &plan)?;
            let all = jobs::load(conn, &plan)?;
            Ok(display.job_tier(all.iter().find(|j| j.key == "a").unwrap()))
        })
        .unwrap()
}

#[test]
fn an_active_attempt_sets_the_shown_tier() {
    let extra = [attempt("a", (1, "H3", "ladder", true)), event("attempt-started", "a"), attempt("a", (2, "S1", "ladder", false)), event("attempt-started", "a")].join("\n");
    let (_dir, mut store) = store_with("running", Some("H3"), None, &extra);
    assert_eq!(shown(&mut store).as_deref(), Some("S1"));
}

#[test]
fn a_pending_retry_sets_the_shown_tier() {
    let extra = [attempt("a", (1, "H3", "ladder", true)), event("attempt-started", "a"), event("job-retried", "a")].join("\n");
    let (_dir, mut store) = store_with("queued", Some("H3"), Some("S1"), &extra);
    assert_eq!(shown(&mut store).as_deref(), Some("S1"));
}

#[test]
fn a_retry_attempt_on_another_tier_is_the_shown_tier() {
    // the report (open-claim-2): tier H3, retry_tier S1, a1 H3 (ladder), then a retry's Attempts on S1; the retry is not pending
    let extra = [
        attempt("a", (1, "H3", "ladder", true)),
        event("attempt-started", "a"),
        event("job-retried", "a"),
        attempt("a", (2, "S1", "retry", true)),
        event("attempt-started", "a"),
    ]
    .join("\n");
    let (_dir, mut store) = store_with("failed", Some("H3"), Some("S1"), &extra);
    assert_eq!(shown(&mut store).as_deref(), Some("S1"));
}

#[test]
fn an_ended_attempt_tier_is_shown_when_no_retry_is_pending() {
    // a pending ladder step: a1 on H0 ended, a2 not started: the Job's `tier` is H3, a1's Tier is shown
    let extra = [attempt("a", (1, "H0", "ladder", true)), event("attempt-started", "a")].join("\n");
    let (_dir, mut store) = store_with("queued", Some("H3"), None, &extra);
    assert_eq!(shown(&mut store).as_deref(), Some("H0"));
}

#[test]
fn with_no_attempt_and_no_retry_the_shown_tier_is_the_jobs_tier() {
    let (_dir, mut store) = store_with("queued", Some("H3"), None, "");
    assert_eq!(shown(&mut store).as_deref(), Some("H3"));
}

#[test]
fn a_stub_draft_has_no_shown_tier() {
    let (_dir, mut store) = store_with("draft", None, None, "");
    assert_eq!(shown(&mut store), None);
}

#[test]
fn plan_render_prints_the_shown_tier_in_the_table_and_the_section() {
    let extra = [attempt("a", (1, "H3", "ladder", true)), event("attempt-started", "a"), event("job-retried", "a")].join("\n");
    let (_dir, mut store) = store_with("queued", Some("H3"), Some("S1"), &extra);
    let plan = PlanId::parse("pl").unwrap();
    let text = store
        .read(|conn| {
            let display = Display::load(conn, &plan)?;
            Ok(markdown(&Heading::Plan { title: "T", plan: "pl" }, "ctx", "dispatch", &load_prs(conn, &plan)?, &jobs::load(conn, &plan)?, &[], "O1", &display))
        })
        .unwrap();
    let row = text.lines().find(|l| l.contains("Retried") && l.starts_with('|')).expect("the Job's table row");
    assert!(row.contains("| S1 |") && !row.contains("H3"), "table row: {row}");
    let section = text.lines().find(|l| l.contains("· Tier:")).expect("the Job's section line");
    assert!(section.contains("Tier: `S1`"), "section line: {section}");
}
