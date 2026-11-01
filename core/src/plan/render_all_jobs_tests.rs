//! Red tests of THE RULE (User, pro-d270, pro-d286): every Job of a Plan is one row of `plan render`, whatever its kind and status
//! and whether or not it has a PR (commands.md §4.5). The assertion names no kind and no status: the rows drawn equal `SELECT key FROM jobs`.

use std::collections::BTreeSet;

use crate::home::CpdHome;
use crate::ids::PlanId;
use crate::plan::display::Display;
use crate::plan::fingerprint::load_prs;
use crate::plan::jobs::{self, KINDS, STATUSES, is_explore};
use crate::plan::render::{Heading, markdown};
use crate::store::Store;

/// The Plan of every fixture here (Context `ctx`, a Lead Session).
pub(super) const PLAN: &str = "pl";

/// A store holding the Plan [`PLAN`] with its Context and Lead Session, then `sql`.
pub(super) fn store_with(sql: &str) -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead', 'lead', 'ctx', '{PLAN}', 'Lead', '/', 'O3', 'working', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at) VALUES ('{PLAN}', 'ctx', 'P', 'dispatch', 'lead', 't', 't');
                 {sql}"
            ))?;
            Ok(())
        })
        .unwrap();
    (dir, store)
}

/// One PR per status the `prs.status` CHECK allows, the key the status, with the columns that status requires.
fn prs_sql(statuses: &[&str]) -> String {
    let mut sql = String::new();
    for (n, status) in statuses.iter().enumerate() {
        let n = n + 1;
        let (worktree, started) = if matches!(*status, "waiting" | "cancelled") {
            ("NULL".to_owned(), "NULL")
        } else if matches!(*status, "dispatch" | "open" | "merged") {
            (format!("'/w/{status}'"), "'t'")
        } else {
            panic!("no fixture for a PR in {status}")
        };
        let (row, pull, merged_at) = match *status {
            "open" | "merged" => {
                let state = if *status == "merged" {
                    "merged"
                } else {
                    "open"
                };
                let merged = if *status == "merged" { "'t'" } else { "NULL" };
                (
                    format!(
                        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at, mergeable, merge_state)
                         VALUES ('acme/api', {n}, 'https://github.com/acme/api/pull/{n}', 'Add api', 0, '{state}', 'approved', 'passed', 't', {merged}, 't', 'conflicting', 'dirty');"
                    ),
                    format!("{n}, 'https://github.com/acme/api/pull/{n}', 't', (SELECT id FROM pull_requests WHERE number = {n})"),
                    merged,
                )
            }
            _ => (String::new(), "NULL, NULL, NULL, NULL".to_owned(), "NULL"),
        };
        sql.push_str(&format!(
            "{row}
             INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request, created_txn, created_at, started_at, merged_at)
             VALUES ('{PLAN}', '{status}', '/r/{status}', 'main', '{status}', 'cpd/{PLAN}/{status}', {worktree}, {pull}, cpd_txn(), 't', {started}, {merged_at});\n"
        ));
    }
    sql
}

/// One Job `key` of `kind` in `status`, of PR `pr` (`None`: of the Plan, an explore Job), with the columns the CHECKs and triggers of `jobs` require.
fn job_sql(key: &str, pr: Option<&str>, kind: &str, status: &str) -> String {
    let draft = status == "draft";
    let q = |v: Option<String>| v.map_or("NULL".to_owned(), |v| format!("'{v}'"));
    let fingerprint = q((!draft).then(|| "f".repeat(64)));
    let merged_sha = q((status == "merged").then(|| "a".repeat(40)));
    let failed_cause = q((status == "failed").then(|| "timed_out".to_owned()));
    let cancelled = status == "cancelled";
    let cancel_reason = q(cancelled.then(|| "x".to_owned()));
    let cancel_requested_at = q(cancelled.then(|| "t".to_owned()));
    let summary = q((is_explore(kind) && status == "merged").then(|| "The summary.".to_owned()));
    // a `test` Job names its tests and the files that hold them (trigger `jobs_kind_spec_ins`); only a `test` Job may
    let (tests, test_files) = if kind == "test" {
        ("'[\"t\"]'", "'[\"f.rs\"]'")
    } else {
        ("NULL", "NULL")
    };
    let pr = q(pr.map(str::to_owned));
    format!(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint,
                           failed_cause, merged_sha, cancel_reason, cancel_requested_at, summary, kind, tests, test_files, created_txn, created_at)
         VALUES ('{PLAN}', {pr}, '{key}', 'Title {key}', 'S2', 'Goal of {key}.', 'c', '[\"a\"]', '[\"b\"]', '[]', '{status}', 0, {fingerprint},
                 {failed_cause}, {merged_sha}, {cancel_reason}, {cancel_requested_at}, {summary}, '{kind}', {tests}, {test_files}, cpd_txn(), 't');\n"
    )
}

/// Every explore kind of [`KINDS`] times every status, of the Plan.
fn explore_jobs_sql() -> String {
    let mut sql = String::new();
    for kind in KINDS.into_iter().filter(|k| is_explore(k)) {
        for status in STATUSES {
            sql.push_str(&job_sql(&format!("{kind}-{status}"), None, kind, status));
        }
    }
    sql
}

/// The key (first cell) of every row of every `### Jobs` table and of the `## Explore` table.
fn drawn_keys(md: &str) -> Vec<String> {
    let mut keys = Vec::new();
    let mut in_table = None::<usize>; // lines seen in the table being read
    let mut want_table = false;
    for line in md.lines() {
        if matches!(line, "### Jobs" | "## Explore") {
            want_table = true;
        } else if want_table && line.starts_with('|') {
            let seen = in_table.get_or_insert(0);
            if *seen >= 2 {
                let cell = line.split('|').nth(1).unwrap_or_default().trim();
                keys.push(cell.trim_matches('`').to_owned());
            }
            *seen += 1;
        } else if in_table.is_some() {
            in_table = None;
            want_table = false;
        }
    }
    keys
}

fn assert_every_job_is_one_row(store: &mut Store) {
    let plan = PlanId::parse(PLAN).unwrap();
    let (md, mut want) = store
        .read(|conn| {
            let prs = load_prs(conn, &plan)?;
            let md = markdown(
                &Heading::Plan {
                    title: "T",
                    plan: PLAN,
                },
                "ctx",
                "dispatch",
                &prs,
                &jobs::load(conn, &plan)?,
                &[],
                "S2",
                &Display::of(&prs),
            );
            let mut stmt = conn.prepare("SELECT key FROM jobs WHERE plan = ?1")?;
            let keys = stmt
                .query_map([PLAN], |r| r.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok((md, keys))
        })
        .unwrap();
    let mut got = drawn_keys(&md);
    want.sort();
    got.sort();
    let (want_set, got_set): (BTreeSet<_>, BTreeSet<_>) = (
        want.iter().cloned().collect(),
        got.iter().cloned().collect(),
    );
    let missing: Vec<_> = want_set.difference(&got_set).collect();
    let extra: Vec<_> = got.iter().filter(|k| !want_set.contains(*k)).collect();
    assert!(!want.is_empty(), "the fixture has no Job");
    assert!(
        got == want,
        "Jobs not drawn: {missing:?}\nrows that are no Job (or drawn twice): {extra:?}"
    );
}

#[test]
fn every_job_of_a_plan_with_prs_is_one_row_of_plan_render() {
    let statuses = ["waiting", "dispatch", "open", "merged", "cancelled"];
    let mut sql = prs_sql(&statuses);
    for pr in statuses {
        for kind in KINDS.into_iter().filter(|k| !is_explore(k)) {
            for status in STATUSES {
                sql.push_str(&job_sql(
                    &format!("{pr}-{kind}-{status}"),
                    Some(pr),
                    kind,
                    status,
                ));
            }
        }
    }
    sql.push_str(&explore_jobs_sql());
    let (_dir, mut store) = store_with(&sql);
    assert_every_job_is_one_row(&mut store);
}

#[test]
fn every_job_of_a_plan_with_no_pr_is_one_row_of_plan_render() {
    let (_dir, mut store) = store_with(&explore_jobs_sql());
    assert_every_job_is_one_row(&mut store);
}
