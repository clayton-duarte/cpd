//! Red tests of the `tier_advice` rule (contract §2.7) and of the advice the lint computes (§3.4b): the rule is pure over
//! `Input.advice`; the advice is read from a tempdir store with a fixed catalog (no pi, no PATH, no CPD_HOME).

use serde_json::json;

use super::jobs::{Job, JobSpec};
use super::lint::{Finding, Input, Rule, Severity, run};
use super::lint_cmd::advice;
use super::prs::PrRow;
use super::row::Phase;
use super::tier_advice::Advice;
use crate::config::{Catalog, Tier, TierModel};
use crate::env::Env;
use crate::home::CpdHome;
use crate::ids::PlanId;

fn t(s: &str) -> Tier {
    Tier::parse(s).unwrap()
}

fn job(key: &str, pr: &str, kind: &str, status: &str, touches: &[&str], needs: &[&str]) -> Job {
    Job {
        seq: 1,
        key: key.into(),
        pr: Some(pr.into()),
        title: format!("Title {key}"),
        tier: Some("S2".into()),
        spec: Some(JobSpec {
            goal: "g".into(),
            context: "c".into(),
            touches: json!(touches),
            acceptance: json!(["b"]),
            out_of_scope: json!([]),
        }),
        status: status.into(),
        appended: false,
        kind: kind.into(),
        tests: None,
        test_files: None,
        resources: None,
        test_approved: None,
        touches_approved: None,
        urgent: false,
        fingerprint: None,
        retry_tier: None,
        cancel_reason: None,
        cancel_requested_at: None,
        failed_cause: None,
        merged_sha: None,
        summary: None,
        created_at: "t".into(),
        needs: needs.iter().map(|n| (*n).to_owned()).collect(),
    }
}

fn at(key: &str, kind: &str, status: &str, tier: &str) -> Job {
    let mut j = job(key, "web", kind, status, &[], &[]);
    j.tier = Some(tier.into());
    j
}

fn pr(key: &str) -> PrRow {
    PrRow::fixture(key, "/repo", "main", &[], "waiting")
}

fn adv(kind: &str, tier: &str) -> Advice {
    Advice {
        kind: kind.into(),
        tier: t(tier),
        n: 633,
        passed: 619,
        rate: 0.978,
        lower: 0.9636,
        expected_cost: 0.3149,
    }
}

fn lint(jobs: &[Job], advice: &[Advice]) -> Vec<Finding> {
    let plan = PlanId::parse("pl").unwrap();
    let prs = [pr("web")];
    let input = Input::new(&plan, Phase::Dispatch, &prs, jobs, None).with_advice(advice);
    run(&input)
        .into_iter()
        .filter(|f| f.rule == Rule::TierAdvice)
        .collect()
}

const BASE: &str = "of 633 first Attempts at S1, 619 merged (97.8%, lower bound 96.4%), at an expected cost of $0.31. That is what happened to Jobs that ran at S1, not what the Tier causes: the Lead picks harder Jobs for higher Tiers.";

#[test]
fn tier_advice_warns_when_a_draft_is_above_the_advised_tier() {
    let found = lint(&[at("a", "impl", "draft", "S2")], &[adv("impl", "S1")]);
    assert_eq!(found.len(), 1, "{found:?}");
    let f = &found[0];
    assert_eq!(f.severity(), Severity::Warn);
    assert_eq!(f.pr.as_deref(), Some("web"));
    assert_eq!(f.job.as_deref(), Some("a"));
    assert_eq!(f.other, None);
    assert!(f.paths.is_empty());
    assert!(f.details.is_empty());
    assert_eq!(
        f.message,
        format!(
            "a is S2 but history favours S1 for impl Jobs: {BASE} Keep S2 if this Job is harder than the usual one, or change it with cpd job edit a --tier S1."
        )
    );
}

#[test]
fn tier_advice_is_downward_only() {
    let jobs = [
        at("up", "impl", "draft", "S1"),
        at("high", "work", "draft", "H3"),
        at("same", "docs", "draft", "S1"),
    ];
    let advice = [adv("impl", "S2"), adv("work", "S1"), adv("docs", "S1")];
    let control = lint(&[at("h", "impl", "draft", "S1")], &[adv("impl", "H3")]);
    assert_eq!(control.len(), 1, "an S1 Job with advice H3 is warned");
    let found = lint(&jobs, &advice);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn tier_advice_reads_draft_and_queued_jobs_of_five_kinds_only() {
    let advice: Vec<Advice> = [
        "work", "docs", "test", "impl", "verify", "open", "update", "scout", "research",
    ]
    .iter()
    .map(|k| adv(k, "S1"))
    .collect();
    let mut jobs: Vec<Job> = Vec::new();
    for k in ["work", "docs", "test", "impl", "verify"] {
        jobs.push(at(&format!("d-{k}"), k, "draft", "S2"));
    }
    for k in ["open", "update", "scout", "research"] {
        jobs.push(at(&format!("n-{k}"), k, "draft", "S2"));
    }
    for s in ["running", "merged", "failed", "cancelled"] {
        jobs.push(at(&format!("s-{s}"), "impl", s, "S2"));
    }
    jobs.push(at("q", "impl", "queued", "S2"));
    let found = lint(&jobs, &advice);
    let mut keys: Vec<&str> = found.iter().filter_map(|f| f.job.as_deref()).collect();
    keys.sort();
    assert_eq!(
        keys,
        ["d-docs", "d-impl", "d-test", "d-verify", "d-work", "q"]
    );
    let q = found
        .iter()
        .find(|f| f.job.as_deref() == Some("q"))
        .unwrap();
    assert!(
        q.message
            .starts_with("q is S2 but history favours S1 for impl Jobs: ")
    );
    assert!(!q.message.contains("Keep"), "{}", q.message);
    assert!(!q.message.contains("cpd job edit"), "{}", q.message);
}

#[test]
fn tier_advice_skips_stub_drafts_and_kinds_without_advice() {
    let mut stub = at("stub", "impl", "draft", "S2");
    stub.tier = None;
    stub.spec = None;
    let control = at("ok", "impl", "draft", "S2");
    let found = lint(
        &[stub, control, at("other", "docs", "draft", "S2")],
        &[adv("impl", "S1")],
    );
    assert!(found.iter().all(|f| f.job.as_deref() != Some("stub")));
    assert!(found.iter().all(|f| f.job.as_deref() != Some("other")));
}

#[test]
fn tier_advice_needs_no_catalog_and_no_store() {
    assert!(lint(&[at("a", "impl", "draft", "S2")], &[]).is_empty());
}

#[test]
fn tier_advice_findings_follow_the_overlaps_in_catalogue_order() {
    let plan = PlanId::parse("pl").unwrap();
    let prs = [pr("web")];
    let mut x = job("x", "web", "impl", "draft", &["src/a.rs"], &[]);
    x.tier = Some("S2".into());
    let y = job("y", "web", "work", "draft", &["src/a.rs"], &[]);
    let mut b = at("b", "docs", "draft", "S2");
    b.spec.as_mut().unwrap().touches = json!(["src/b.rs"]);
    let jobs = [b, y, x];
    let advice = [adv("impl", "S1"), adv("work", "S1"), adv("docs", "S1")];
    let input = Input::new(&plan, Phase::Dispatch, &prs, &jobs, None).with_advice(&advice);
    let found = run(&input);
    let order: Vec<(Rule, Option<&str>)> = found
        .iter()
        .filter(|f| matches!(f.rule, Rule::ParallelWriteOverlap | Rule::TierAdvice))
        .map(|f| (f.rule, f.job.as_deref()))
        .collect();
    assert_eq!(
        order,
        [
            (Rule::ParallelWriteOverlap, Some("x")),
            (Rule::TierAdvice, Some("b")),
            (Rule::TierAdvice, Some("x")),
            (Rule::TierAdvice, Some("y")),
        ]
    );
}

#[test]
fn finding_row_of_tier_advice_is_short() {
    let found = lint(&[at("a", "impl", "draft", "S2")], &[adv("impl", "S1")]);
    assert_eq!(found.len(), 1, "control");
    assert_eq!(found[0].row(), "a is S2, history favours S1 for impl");
}

// ---- the advice, from a store ----

fn model(name: &str, efforts: &[u8]) -> TierModel {
    TierModel {
        model: name.into(),
        efforts: efforts.to_vec(),
    }
}

fn catalog(s_efforts: &[u8]) -> Catalog {
    Catalog {
        pi_version: "1.2.0".into(),
        o: model("opus", &[0, 1, 2, 3, 4, 5]),
        s: model("sonnet", s_efforts),
        h: model("haiku", &[0, 1, 2, 3]),
        g: model("gemini", &[]),
    }
}

/// An Env on a tempdir store with context `ctx`, Plan `pl`, PR `r` and the catalog fixed.
fn env(dir: &tempfile::TempDir, catalog: Catalog) -> Env {
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    std::fs::create_dir_all(home.config().parent().unwrap()).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"m1\"\n").unwrap();
    let owner = home.config().parent().unwrap().join("policy").join("acme");
    std::fs::create_dir_all(&owner).unwrap();
    std::fs::write(owner.join("owner.toml"), "").unwrap();
    let mut env = Env::open_home(home).unwrap();
    env.catalog = Some(catalog);
    env.store
        .write("fixture", |tx| {
            tx.execute_batch(
                "INSERT INTO contexts (id, title, created_at, created_on, owner) VALUES ('ctx', 'Ctx', 't', 'm1', 'acme');
                 INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
                   VALUES ('lead', 'lead', 'ctx', 'pl', 'Lead', '/', 'O3', 'working', 't', 't');
                 INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at) VALUES ('pl', 'ctx', 'P', 'dispatch', 'lead', 't', 't');
                 INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at)
                   VALUES ('pl', 'r', '/r', 'main', 'dispatch', 'cpd/pl', '/w', cpd_txn(), 't', 't');",
            )?;
            Ok(())
        })
        .unwrap();
    env
}

/// `count` first Attempts of `kind` Jobs at `tier`, `merged` of them merged, each at `cost`.
fn history(env: &mut Env, tag: &str, kind: &str, tier: &str, count: u32, merged: u32, cost: f64) {
    let mut sql = String::new();
    for i in 0..count {
        let key = format!("{tag}{i}");
        let (status, outcome, cause, failed, sha) = if i < merged {
            ("merged", "'merged'", "'merged'", "NULL", "'abc'")
        } else {
            (
                "failed",
                "'failed'",
                "'gate_failed'",
                "'gate_failed'",
                "NULL",
            )
        };
        sql.push_str(&format!(
            "INSERT INTO jobs (plan, pr, key, title, kind, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, failed_cause, merged_sha, cancel_reason, cancel_requested_at, created_txn, created_at)
             VALUES ('pl', 'r', '{key}', 'T', '{kind}', '{tier}', 'g', 'c', '[\"a\"]', '[\"b\"]', '[]', '{status}', 1, '{}', {failed}, {sha}, NULL, NULL, cpd_txn(), 't');
             INSERT INTO attempts (plan, id, job, number, tier, origin, status, outcome, cause, ended_at, branch, worktree, started_at, metrics, merged_sha)
             VALUES ('pl', '{key}-a1', '{key}', 1, '{tier}', 'ladder', 'ended', {outcome}, {cause}, 't', 'cpd/pl--{key}-a1', '/w', 't', '{{\"cost_usd\": {cost}}}', {sha});",
            "f".repeat(64)
        ));
    }
    env.store
        .write("fixture", |tx| {
            tx.execute_batch(&sql)?;
            Ok(())
        })
        .unwrap();
}

#[test]
fn advice_recommends_per_kind_from_first_attempts() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = env(&dir, catalog(&[0, 1, 2, 3]));
    history(&mut env, "a", "impl", "S1", 40, 38, 0.20);
    history(&mut env, "b", "impl", "S2", 20, 20, 0.50);
    let got = advice(&mut env, "ctx").unwrap();
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].kind, "impl");
    assert_eq!(got[0].tier, t("S1"));
    assert_eq!((got[0].n, got[0].passed), (40, 38));
}

#[test]
fn advice_drops_a_cell_whose_tier_the_catalog_does_not_accept() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = env(&dir, catalog(&[0, 1, 2, 3]));
    history(&mut env, "a", "impl", "S3", 40, 40, 0.30);
    let got = advice(&mut env, "ctx").unwrap();
    assert_eq!(
        got.len(),
        1,
        "control: S3 is advised when the catalog accepts it"
    );
    assert_eq!(got[0].tier, t("S3"));
    env.catalog = Some(catalog(&[1, 2]));
    let got = advice(&mut env, "ctx").unwrap();
    assert!(got.iter().all(|a| a.tier != t("S3")), "{got:?}");
}

#[test]
fn advice_is_empty_without_a_cell_at_the_floor() {
    let dir = tempfile::tempdir().unwrap();
    let mut env = env(&dir, catalog(&[0, 1, 2, 3]));
    history(&mut env, "a", "impl", "S1", 29, 29, 0.20);
    assert!(advice(&mut env, "ctx").unwrap().is_empty());
}
