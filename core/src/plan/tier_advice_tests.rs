//! The pure Tier advice (contract §3.4b): the rule over in-memory rows and the first-Attempt query over a tempdir store.

use super::tier_advice::{first_attempts, qualifying, recommend, wilson_lower, Row};
use crate::config::{Catalog, Tier, TierModel};
use crate::home::CpdHome;
use crate::store::Store;

const EPS: f64 = 0.0005;

fn near(a: f64, b: f64) {
    assert!((a - b).abs() < EPS, "{a} != {b}");
}

fn model(name: &str, efforts: &[u8]) -> TierModel {
    TierModel { model: name.into(), efforts: efforts.to_vec() }
}

/// The real `Catalog::next` as the ladder (H3 -> S1, S3 -> none).
fn ladder() -> impl Fn(Tier) -> Option<Tier> {
    let catalog = Catalog {
        pi_version: "1.2.0".into(),
        o: model("opus", &[0, 1, 2, 3, 4, 5]),
        s: model("sonnet", &[0, 1, 2, 3]),
        h: model("haiku", &[0, 1, 2, 3]),
        g: model("gemini", &[]),
    };
    move |t| catalog.next(t).unwrap()
}

fn t(s: &str) -> Tier {
    Tier::parse(s).unwrap()
}

fn row(kind: &str, tier: &str, n: u32, passed: u32, avg_cost: Option<f64>) -> Row {
    Row { kind: kind.into(), tier: t(tier), n, passed, avg_cost }
}

#[test]
fn needs_the_minimum_sample() {
    let rows = [row("work", "S1", 29, 29, Some(0.2)), row("work", "S2", 40, 40, Some(0.5))];
    let got = recommend(&rows[..1], 30, ladder());
    assert!(got.is_empty());
    let got = recommend(&rows, 30, ladder());
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].tier, t("S2"));
}

#[test]
fn wilson_floor_26_of_30_qualifies_25_of_30_does_not() {
    let yes = recommend(&[row("work", "S3", 30, 26, Some(0.5))], 30, ladder());
    assert_eq!(yes.len(), 1);
    assert_eq!(yes[0].tier, t("S3"));
    assert!(recommend(&[row("work", "S3", 30, 25, Some(0.5))], 30, ladder()).is_empty());
}

#[test]
fn wilson_lower_is_the_95_percent_score_interval() {
    for (passed, n, want) in [(26, 30, 0.7032), (25, 30, 0.6644), (304, 310, 0.9584), (76, 76, 0.9519), (0, 1, 0.0), (0, 0, 0.0)] {
        near(wilson_lower(passed, n), want);
    }
}

#[test]
fn picks_the_lowest_expected_cost_not_the_lowest_tier() {
    let rows = [
        row("docs", "H3", 40, 38, Some(0.48)),
        row("docs", "S1", 40, 40, Some(0.31)),
        row("docs", "S2", 5, 5, Some(1.00)),
    ];
    let got = recommend(&rows, 30, ladder());
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].kind, "docs");
    assert_eq!(got[0].tier, t("S1"));
}

#[test]
fn ties_go_to_the_lower_tier() {
    let rows = [row("work", "S2", 40, 40, Some(0.5)), row("work", "S3", 40, 40, Some(0.5))];
    let got = recommend(&rows, 30, ladder());
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].tier, t("S2"));
}

#[test]
fn no_tier_above_the_floor_gives_no_advice() {
    let rows = [
        row("work", "S3", 29, 29, Some(0.5)),
        row("docs", "S3", 30, 25, Some(0.5)),
        row("test", "S3", 10, 10, Some(0.5)),
        row("test", "S2", 100, 60, Some(0.5)),
    ];
    assert!(recommend(&rows, 30, ladder()).is_empty());
}

// ---- the store ----

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let mut store = Store::open(&home, 5000).unwrap();
    store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead', 'lead', 'ctx', 'pl', 'Lead', '/', 'O3', 'working', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at) VALUES ('pl', 'ctx', 'P', 'dispatch', 'lead', 't', 't');
                 INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
                   VALUES ('pl', 'r', '/r', 'main', 'dispatch', 'cpd/pl', '/w', cpd_txn(), 't', 't');",
            )?;
            Ok(())
        })
        .unwrap();
    (dir, store)
}

/// SQL for the Job `key` of kind `kind` (status `merged` or `failed`, a stub spec).
fn job(key: &str, kind: &str, status: &str) -> String {
    let (failed, merged) = if status == "failed" { ("'gate_failed'", "NULL") } else { ("NULL", "'abc'") };
    format!(
        "INSERT INTO jobs (plan, pr, key, title, kind, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, failed_cause, merged_sha, cancel_reason, cancel_requested_at, created_txn, created_at)
         VALUES ('pl', 'r', '{key}', 'T', '{kind}', 'S1', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '{status}', 1, '{}', {failed}, {merged}, NULL, NULL, cpd_txn(), 't');",
        "f".repeat(64)
    )
}

/// SQL for Attempt `number` of `job`: `outcome` and `cause` are SQL literals (`NULL` while running), `metrics` a SQL literal.
fn attempt(job: &str, number: u32, tier: &str, outcome: &str, cause: &str, metrics: &str) -> String {
    let (status, ended) = if outcome == "NULL" { ("running", "NULL") } else { ("ended", "'t'") };
    format!(
        "INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, ended_at, branch, worktree, started_at, metrics)
         VALUES ('pl', '{job}-a{number}', '{job}', {number}, '{tier}', 'initial', '{status}', {outcome}, {cause}, {ended}, 'cpd/pl--{job}-a{number}', '/w', 't', {metrics});"
    )
}

fn load(sql: &str) -> Vec<Row> {
    let (_d, mut s) = store();
    s.write("fixture", |tx| {
        tx.execute_batch(sql)?;
        Ok(())
    })
    .unwrap();
    s.read(|conn| first_attempts(conn)).unwrap()
}

#[test]
fn excludes_provider_error_and_cancelled() {
    let sql = [
        job("j1", "impl", "merged"),
        attempt("j1", 1, "S1", "'merged'", "'merged'", "NULL"),
        job("j2", "impl", "failed"),
        attempt("j2", 1, "S1", "'failed'", "'gate_failed'", "NULL"),
        job("j3", "impl", "failed"),
        attempt("j3", 1, "S1", "'cancelled'", "'cancel_requested'", "NULL"),
        job("j4", "impl", "failed"),
        attempt("j4", 1, "S1", "'failed'", "'provider_error'", "NULL"),
        job("j5", "impl", "failed"),
        attempt("j5", 1, "S1", "NULL", "NULL", "NULL"),
    ]
    .concat();
    let rows = load(&sql);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, "impl");
    assert_eq!(rows[0].tier, t("S1"));
    assert_eq!((rows[0].n, rows[0].passed), (2, 1));
}

#[test]
fn query_counts_first_attempts_only() {
    let sql = [
        job("j1", "impl", "merged"),
        attempt("j1", 1, "S1", "'failed'", "'gate_failed'", "'{\"cost_usd\": 0.25}'"),
        attempt("j1", 2, "S2", "'merged'", "'merged'", "'{\"cost_usd\": 0.75}'"),
        job("j2", "impl", "merged"),
        attempt("j2", 1, "S1", "'merged'", "'merged'", "NULL"),
    ]
    .concat();
    let rows = load(&sql);
    assert_eq!(rows.len(), 1, "no (impl, S2) row: {rows:?}");
    assert_eq!((rows[0].tier, rows[0].n, rows[0].passed), (t("S1"), 2, 1));
    near(rows[0].avg_cost.expect("the one cost"), 0.25);

    let none = load(&[job("j1", "impl", "merged"), attempt("j1", 1, "S1", "'merged'", "'merged'", "NULL")].concat());
    assert_eq!(none.len(), 1);
    assert_eq!(none[0].n, 1);
    assert_eq!(none[0].avg_cost, None);
}

// ---- expected cost ----

#[test]
fn expected_cost_adds_the_next_rungs_cost_scaled_by_the_failure_rate() {
    let rows = [
        row("impl", "S1", 310, 304, Some(0.2249)),
        row("impl", "S2", 64, 63, Some(0.6605)),
        row("impl", "S3", 35, 35, Some(0.8365)),
    ];
    let got = qualifying(&rows, 30, ladder());
    assert_eq!(got.iter().map(|a| a.tier).collect::<Vec<_>>(), [t("S1"), t("S2"), t("S3")]);
    near(got[0].expected_cost, 0.2377);
    near(got[1].expected_cost, 0.6736);
    near(got[2].expected_cost, 0.8365);
    near(got[0].rate, 0.9806);
    near(got[0].lower, 0.9584);
    let best = recommend(&rows, 30, ladder());
    assert_eq!(best.len(), 1);
    assert_eq!(best[0].tier, t("S1"));
}

#[test]
fn a_tier_whose_next_rung_has_no_cost_is_not_advised() {
    let s1 = row("test", "S1", 40, 40, Some(0.2));
    assert!(qualifying(&[s1.clone()], 30, ladder()).is_empty());
    assert!(recommend(&[s1.clone()], 30, ladder()).is_empty());

    let no_cost = [s1.clone(), row("test", "S2", 3, 3, None)];
    assert!(qualifying(&no_cost, 30, ladder()).is_empty());
    assert!(recommend(&no_cost, 30, ladder()).is_empty());

    let with_cost = [s1, row("test", "S2", 3, 3, Some(0.5))];
    let got = qualifying(&with_cost, 30, ladder());
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].tier, t("S1"));
}

#[test]
fn a_cell_without_a_cost_gives_no_advice() {
    let rows = [row("scout", "H3", 40, 40, None), row("scout", "S1", 3, 3, Some(0.3))];
    assert!(qualifying(&rows, 30, ladder()).is_empty());
    assert!(recommend(&rows, 30, ladder()).is_empty());
}

#[test]
fn a_tier_a_worker_may_not_have_is_never_advised() {
    let o2 = row("work", "O2", 100, 100, Some(0.01));
    assert!(recommend(&[o2.clone()], 30, ladder()).is_empty());
    let rows = [o2, row("work", "S1", 40, 40, Some(0.3)), row("work", "S2", 3, 3, Some(0.5))];
    let got = recommend(&rows, 30, ladder());
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].tier, t("S1"));
}
