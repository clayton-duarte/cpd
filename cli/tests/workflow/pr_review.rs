//! `review submit` (commands.md §31.1, §21.9; pro-d107, pro-d108, pro-d110; A4, A5), run in-process on `run_with` over a
//! `gh` handler that the runner records. The command needs no Plan, Context or PR key; the queue is read through SQL.

use crate::common;

use std::sync::Arc;

use common::fx::Fx;
use common::refused;
use cpd_core::home::CpdHome;
use cpd_core::host::runner::{Call, Handler, Reply};
use serde_json::{Value, json};

const REF: &str = "acme/api#7";
const URL: &str = "https://github.com/acme/api/pull/7";
const CHANNEL: &str = "C02JU6C2Z1D";
const TS: &str = "1793040197.490209";
const SLACK: &str = "C02JU6C2Z1D/1793040197.490209";

const HEAD: &str = "0123456789abcdef0123456789abcdef01234567";
const MOVED: &str = "fedcba9876543210fedcba9876543210fedcba98";

/// The in-process `gh`: every call exits 9; `review submit` queues and never calls it.
struct Gh;

impl Handler for Gh {
    fn handle(&self, _call: &Call) -> Reply {
        Reply::exit(9, "", "unexpected gh call")
    }
}

/// A fixture whose cache holds `acme/api#7` at `head` (the head the pass read): the command checks it there and queues.
fn fixture_at(head: &str) -> Fx {
    let fx = Fx::with_ports();
    let home = CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap();
    std::fs::write(home.config(), "version = 1\nmachine = \"t\"\n").unwrap();
    fx.env.sql(&format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at, head_oid)
           VALUES ('acme/api', 7, '{URL}', 'T', 0, 'open', 'none', 'passed', 't', 't', '{head}')"
    ));
    let _ = (*fx.fakes.runner).clone().route("gh", Arc::new(Gh));
    fx
}

fn fixture(_head: (), _post: ()) -> Fx {
    fixture_at(HEAD)
}

/// The mutations: `(target_key, op_kind, status, vars, patch)`.
/// The GitHub mutations of the outbox: the review, not the Slack reaction the same submit queues.
fn github_mutations(fx: &Fx) -> Vec<(String, String, String, Value, Value)> {
    mutations(fx)
        .into_iter()
        .filter(|m| m.0.starts_with("github:"))
        .collect()
}

fn mutations(fx: &Fx) -> Vec<(String, String, String, Value, Value)> {
    fx.env.read(|c| {
        let mut stmt = c.prepare("SELECT target_key, op_kind, status, vars_json, coalesce(patch_json, 'null') FROM swr_mutations ORDER BY id")?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    serde_json::from_str(&r.get::<_, String>(3)?).unwrap(),
                    serde_json::from_str(&r.get::<_, String>(4)?).unwrap(),
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    })
}

/// Writes `text` as the review file and returns its path.
fn input(fx: &Fx, text: &str) -> String {
    let path = fx.env.cpd_home().join("review.json");
    std::fs::write(&path, text).unwrap();
    path.display().to_string()
}

fn review() -> Value {
    json!({"body": "Looks right.",
           "comments": [{"path": "src/a.rs", "line": 3, "side": "RIGHT", "body": "nit"}]})
}

fn submit(
    fx: &Fx,
    pr: &str,
    head: &str,
    event: &str,
    file: &str,
    slack: Option<&str>,
) -> common::Run {
    let mut args = vec![
        "review", "submit", "--pr", pr, "--head", head, "--event", event, "--input", file,
    ];
    if let Some(slack) = slack {
        args.extend(["--slack", slack]);
    }
    fx.cpd(&args)
}

/// The Slack reaction mutations of the outbox: `channel`, `ts`, `emoji`, `reacted_at` (set once the send is done).
fn rows(fx: &Fx) -> Vec<Value> {
    let text: String = fx.env.query(
        "SELECT coalesce(json_group_array(json_object('channel', json_extract(vars_json, '$.channel'), 'ts', json_extract(vars_json, '$.ts'),
                  'emoji', json_extract(vars_json, '$.emoji'), 'reacted_at', IIF(status IN ('revalidating', 'done'), completed_at, NULL))), '[]')
           FROM (SELECT * FROM swr_mutations WHERE system = 'slack' AND op_kind = 'reaction' ORDER BY id)",
    );
    serde_json::from_str(&text).unwrap()
}

fn gh_calls(fx: &Fx) -> Vec<Call> {
    fx.calls("gh")
}

fn message(error: &Value) -> String {
    error["message"].as_str().unwrap_or_default().to_owned()
}

#[test]
fn approve_queues_the_review_and_the_check_mark() {
    let fx = fixture((), ());
    let file = input(&fx, &review().to_string());
    let out = submit(&fx, REF, HEAD, "APPROVE", &file, Some(SLACK)).ok();
    assert_eq!(out["queued"], true, "{out}");
    assert!(out["mutation_id"].is_i64(), "{out}");
    assert_eq!(
        out["review"],
        json!({"event": "APPROVE", "commit_id": HEAD, "queued": true})
    );
    assert_eq!(
        out["reaction"],
        json!({"channel": CHANNEL, "ts": TS, "emoji": "github-check-mark", "already": false})
    );
    assert_eq!(
        rows(&fx),
        [json!({"channel": CHANNEL, "ts": TS, "emoji": "github-check-mark", "reacted_at": null})]
    );
    assert!(
        gh_calls(&fx).is_empty(),
        "a command never calls GitHub for a write"
    );
}

#[test]
fn request_changes_queues_the_review_and_changes_requested() {
    let fx = fixture((), ());
    let file = input(&fx, &review().to_string());
    let out = submit(&fx, REF, HEAD, "REQUEST_CHANGES", &file, Some(SLACK)).ok();
    assert_eq!(out["review"]["event"], "REQUEST_CHANGES");
    assert_eq!(
        out["reaction"],
        json!({"channel": CHANNEL, "ts": TS, "emoji": "github-changes-requested", "already": false})
    );
    let queued = github_mutations(&fx);
    assert_eq!(queued.len(), 1, "{queued:?}");
    assert_eq!(queued[0].4, json!({"review_decision": "changes_requested"}));
}

#[test]
fn the_queued_review_is_the_input_with_the_event_and_commit_id_added() {
    let fx = fixture((), ());
    let file = input(&fx, &review().to_string());
    submit(&fx, REF, HEAD, "APPROVE", &file, None).ok();
    let mut want = review();
    want["event"] = json!("APPROVE");
    want["commit_id"] = json!(HEAD);
    let queued = mutations(&fx);
    assert_eq!(queued.len(), 1, "{queued:?}");
    assert_eq!(
        (
            queued[0].0.as_str(),
            queued[0].1.as_str(),
            queued[0].2.as_str()
        ),
        ("github:pr:acme/api#7", "approve", "pending")
    );
    assert_eq!(queued[0].3, want);
    assert_eq!(queued[0].4, json!({"review_decision": "approved"}));
}

#[test]
fn a_pr_url_and_owner_repo_hash_n_name_the_same_pull_request() {
    for pr in [REF, URL, &format!("{URL}/")] {
        let fx = fixture((), ());
        let file = input(&fx, &review().to_string());
        submit(&fx, pr, HEAD, "APPROVE", &file, None).ok();
        let queued = mutations(&fx);
        assert_eq!(queued.len(), 1, "{pr}: {queued:?}");
        assert_eq!(queued[0].0, "github:pr:acme/api#7", "{pr}");
    }
}

#[test]
fn a_second_submit_queues_no_second_review_and_one_reaction_row() {
    let fx = fixture((), ());
    let file = input(&fx, &review().to_string());
    let first = submit(&fx, REF, HEAD, "APPROVE", &file, Some(SLACK)).ok();
    let second = submit(&fx, REF, HEAD, "APPROVE", &file, Some(SLACK)).ok();
    assert_eq!(first["reaction"]["already"], false);
    assert_eq!(second["reaction"]["already"], true);
    assert_eq!(
        first["mutation_id"], second["mutation_id"],
        "the same review dedupes"
    );
    assert_eq!(github_mutations(&fx).len(), 1);
    assert_eq!(rows(&fx).len(), 1);
}

#[test]
fn without_slack_nothing_is_queued_and_reaction_is_null() {
    let fx = fixture((), ());
    let file = input(&fx, &review().to_string());
    let out = submit(&fx, REF, HEAD, "REQUEST_CHANGES", &file, None).ok();
    assert_eq!(out["reaction"], Value::Null);
    assert_eq!(out["review"]["queued"], true);
    assert!(rows(&fx).is_empty());
}

#[test]
fn an_input_carrying_an_event_is_refused_before_any_call() {
    let fx = fixture((), ());
    let file = input(&fx, &json!({"body": "x", "event": "COMMENT"}).to_string());
    let error = refused(
        &submit(&fx, REF, HEAD, "APPROVE", &file, Some(SLACK)),
        "usage",
        2,
    );
    assert!(message(&error).contains("event"), "{error}");
    assert!(message(&error).contains("--input"), "{error}");
    assert!(gh_calls(&fx).is_empty());
}

#[test]
fn an_input_that_is_not_a_json_object_is_refused_before_any_call() {
    for text in ["[1]", "\"x\"", "not json", ""] {
        let fx = fixture((), ());
        let file = input(&fx, text);
        let error = refused(&submit(&fx, REF, HEAD, "APPROVE", &file, None), "usage", 2);
        assert!(message(&error).contains("--input"), "{text}: {error}");
        assert!(gh_calls(&fx).is_empty(), "{text}");
    }
    let fx = fixture((), ());
    let missing = fx.env.cpd_home().join("missing.json").display().to_string();
    let error = refused(
        &submit(&fx, REF, HEAD, "APPROVE", &missing, None),
        "usage",
        2,
    );
    assert!(message(&error).contains("--input"), "{error}");
    assert!(gh_calls(&fx).is_empty());
}

#[test]
fn a_malformed_pr_event_or_slack_target_is_refused_before_any_call() {
    let cases: [(&str, &str, Option<&str>, &str); 12] = [
        ("foo", "APPROVE", None, "--pr"),
        ("acme/api#0", "APPROVE", None, "--pr"),
        (REF, "COMMENT", None, "--event"),
        (REF, "approve", None, "--event"),
        (REF, "", None, "--event"),
        (REF, "APPROVE", Some("C02JU6C2Z1D"), "--slack"),
        (
            REF,
            "APPROVE",
            Some("c02JU6C2Z1D/1793040197.490209"),
            "--slack",
        ),
        (REF, "APPROVE", Some("C02JU6C/1793040197.490209"), "--slack"),
        (
            REF,
            "APPROVE",
            Some("C02JU6C2Z1D/179304019.490209"),
            "--slack",
        ),
        (
            REF,
            "APPROVE",
            Some("C02JU6C2Z1D/1793040197490209"),
            "--slack",
        ),
        (
            REF,
            "APPROVE",
            Some("C02JU6C2Z1D/1793040197.49020"),
            "--slack",
        ),
        (
            REF,
            "APPROVE",
            Some("C02JU6C2Z1D/1793040197.490209/x"),
            "--slack",
        ),
    ];
    for (pr, event, slack, flag) in cases {
        let fx = fixture((), ());
        let file = input(&fx, &review().to_string());
        let error = refused(&submit(&fx, pr, HEAD, event, &file, slack), "usage", 2);
        assert!(
            message(&error).contains(flag),
            "{pr} {event} {slack:?}: {error}"
        );
        assert!(gh_calls(&fx).is_empty(), "{pr} {event} {slack:?}");
    }
}

#[test]
fn a_missing_head_is_refused_usage_before_any_call() {
    let fx = fixture((), ());
    let file = input(&fx, &review().to_string());
    let run = fx.cpd(&[
        "review", "submit", "--pr", REF, "--event", "APPROVE", "--input", &file, "--slack", SLACK,
    ]);
    refused(&run, "usage", 2);
    assert!(gh_calls(&fx).is_empty());
    assert!(rows(&fx).is_empty());
}

#[test]
fn a_head_that_is_not_forty_hex_digits_is_refused_usage_before_any_call() {
    let bad = [
        String::new(),
        HEAD[..39].to_owned(),
        format!("{HEAD}0"),
        format!("{}g", &HEAD[..39]),
        format!("0x{}", &HEAD[..38]),
        format!(" {}", &HEAD[..39]),
        "main".to_owned(),
        "abc1234".to_owned(),
    ];
    for value in bad {
        let fx = fixture((), ());
        let file = input(&fx, &review().to_string());
        let error = refused(
            &submit(&fx, REF, &value, "APPROVE", &file, Some(SLACK)),
            "usage",
            2,
        );
        assert!(message(&error).contains("--head"), "{value:?}: {error}");
        assert!(message(&error).contains("40"), "{value:?}: {error}");
        assert!(gh_calls(&fx).is_empty(), "{value:?}");
        assert!(rows(&fx).is_empty(), "{value:?}");
    }
}

#[test]
fn an_input_carrying_a_commit_id_is_refused_usage_before_any_call() {
    let fx = fixture((), ());
    let file = input(&fx, &json!({"body": "x", "commit_id": HEAD}).to_string());
    let error = refused(
        &submit(&fx, REF, HEAD, "APPROVE", &file, Some(SLACK)),
        "usage",
        2,
    );
    for word in ["commit_id", "--input", "--head"] {
        assert!(message(&error).contains(word), "{word}: {error}");
    }
    assert!(gh_calls(&fx).is_empty());
    assert!(rows(&fx).is_empty());
}

#[test]
fn a_matching_head_queues_the_review_pinned_to_it() {
    let fx = fixture((), ());
    let file = input(&fx, &review().to_string());
    let out = submit(&fx, REF, HEAD, "APPROVE", &file, Some(SLACK)).ok();
    assert_eq!(out["review"]["commit_id"], HEAD);
    assert_eq!(mutations(&fx)[0].3["commit_id"], HEAD);
    assert_eq!(
        rows(&fx),
        [json!({"channel": CHANNEL, "ts": TS, "emoji": "github-check-mark", "reacted_at": null})]
    );
}

#[test]
fn an_upper_case_head_is_accepted_and_used_as_lowercase() {
    let fx = fixture((), ());
    let file = input(&fx, &review().to_string());
    let out = submit(&fx, REF, &HEAD.to_uppercase(), "APPROVE", &file, None).ok();
    assert_eq!(out["review"]["commit_id"], HEAD);
    assert_eq!(mutations(&fx)[0].3["commit_id"], HEAD);
    drop(fx);

    let fx = fixture_at(&HEAD.to_uppercase());
    let file = input(&fx, &review().to_string());
    let out = submit(&fx, REF, HEAD, "APPROVE", &file, None).ok();
    assert_eq!(out["review"]["commit_id"], HEAD);
    assert_eq!(mutations(&fx).len(), 1);
}

#[test]
fn a_moved_head_is_refused_head_moved_and_nothing_is_queued() {
    for (pr, given) in [
        (REF, HEAD.to_owned()),
        (URL, HEAD.to_owned()),
        (REF, HEAD.to_uppercase()),
    ] {
        let fx = fixture_at(MOVED);
        let file = input(&fx, &review().to_string());
        let error = refused(
            &submit(&fx, pr, &given, "APPROVE", &file, Some(SLACK)),
            "head_moved",
            3,
        );
        assert_eq!(
            error["details"],
            json!({"pr": REF, "expected": HEAD, "actual": MOVED}),
            "{pr} {given}"
        );
        assert!(message(&error).contains(HEAD), "{error}");
        assert!(message(&error).contains(MOVED), "{error}");
        assert!(gh_calls(&fx).is_empty());
        assert!(mutations(&fx).is_empty());
        assert!(rows(&fx).is_empty());
    }
}

// --- FR10 (pro-n346, pro-d6): `review submit` finishes the caller's `codereview` Plan, named by `CPD_PLAN`.

const PLAN: &str = "r-pay-812";

/// A Context `codereview` and one `cpd`, a Lead Session each, a Plan `r-pay-812` of `codereview` and `c-pay` of `cpd`,
/// both in `grounding`.
fn seed_plans(fx: &Fx) {
    fx.env.sql(
        "INSERT OR IGNORE INTO contexts (id, title, created_at, created_on) VALUES ('codereview', 'Code review', 't', 'm');
         INSERT OR IGNORE INTO contexts (id, title, created_at, created_on) VALUES ('cpd', 'Cpd', 't', 'm');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('l1', 'lead', 'codereview', 'r-pay-812', 'L1', '/', 'S3', 'idle', 't', 't');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('l2', 'lead', 'cpd', 'c-pay', 'L2', '/', 'S3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('r-pay-812', 'codereview', 'Review', 'grounding', 'l1', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at)
           VALUES ('c-pay', 'cpd', 'Pay', 'grounding', 'l2', 't');",
    );
}

/// `submit` with `CPD_PLAN` set to `plan` for the call only (nothing stays set after it).
fn submit_as(
    fx: &Fx,
    plan: Option<&str>,
    event: &str,
    file: &str,
    slack: Option<&str>,
) -> common::Run {
    let mut args = vec![
        "review", "submit", "--pr", REF, "--head", HEAD, "--event", event, "--input", file,
    ];
    if let Some(slack) = slack {
        args.extend(["--slack", slack]);
    }
    let vars: Vec<(&str, &str)> = plan.map(|p| ("CPD_PLAN", p)).into_iter().collect();
    fx.cpd_with(fx.env.root(), &args, &vars)
}

fn phase(fx: &Fx, id: &str) -> String {
    fx.env
        .query(&format!("SELECT phase FROM plans WHERE id = '{id}'"))
}

fn plan_done_events(fx: &Fx, id: &str) -> Vec<Value> {
    let text: String = fx.env.query(&format!(
        "SELECT coalesce(json_group_array(json_object('context', context, 'text', text, 'data', json(data))), '[]')
           FROM (SELECT * FROM events WHERE kind = 'plan-done' AND plan = '{id}' ORDER BY id)"
    ));
    serde_json::from_str(&text).unwrap()
}

fn any_plan_done(fx: &Fx) -> i64 {
    fx.env
        .query("SELECT count(*) FROM events WHERE kind = 'plan-done'")
}

#[test]
fn submit_finishes_the_codereview_plan_named_by_cpd_plan() {
    let fx = fixture((), ());
    seed_plans(&fx);
    let file = input(&fx, &review().to_string());
    submit_as(&fx, Some(PLAN), "APPROVE", &file, Some(SLACK)).ok();
    assert_eq!(phase(&fx, PLAN), "done");
    let (locked, done): (String, String) = fx.env.read(|c| {
        c.query_row(
            "SELECT locked_at, done_at FROM plans WHERE id = 'r-pay-812'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    });
    assert!(!locked.is_empty());
    assert_eq!(locked, done, "locked_at = done_at = the transaction time");
    assert_eq!(
        rows(&fx),
        [json!({"channel": CHANNEL, "ts": TS, "emoji": "github-check-mark", "reacted_at": null})]
    );
}

#[test]
fn submit_finishes_the_plan_without_slack_too() {
    let fx = fixture((), ());
    seed_plans(&fx);
    let file = input(&fx, &review().to_string());
    submit_as(&fx, Some(PLAN), "APPROVE", &file, None).ok();
    assert!(rows(&fx).is_empty());
    assert_eq!(phase(&fx, PLAN), "done");
}

#[test]
fn submit_appends_plan_done_with_turn_false_and_no_workflows() {
    let fx = fixture((), ());
    seed_plans(&fx);
    let file = input(&fx, &review().to_string());
    submit_as(&fx, Some(PLAN), "APPROVE", &file, Some(SLACK)).ok();
    assert_eq!(
        plan_done_events(&fx, PLAN),
        [json!({"context": "codereview",
                "text": "Plan r-pay-812 is done: review submitted.",
                "data": {"workflows": [], "turn": false}})]
    );
}

#[test]
fn submit_leaves_a_plan_of_another_context_alone() {
    let fx = fixture((), ());
    seed_plans(&fx);
    let file = input(&fx, &review().to_string());
    submit_as(&fx, Some("c-pay"), "APPROVE", &file, Some(SLACK)).ok();
    assert_eq!(phase(&fx, "c-pay"), "grounding");
    assert_eq!(any_plan_done(&fx), 0);
    assert_eq!(phase(&fx, PLAN), "grounding");
    // The same command finishes a Plan of `codereview`: only the Context differs.
    submit_as(&fx, Some(PLAN), "APPROVE", &file, Some(SLACK)).ok();
    assert_eq!(phase(&fx, PLAN), "done");
    assert_eq!(phase(&fx, "c-pay"), "grounding");
    assert_eq!(any_plan_done(&fx), 1);
}

#[test]
fn submit_leaves_a_done_deleted_or_unknown_plan_alone() {
    let fx = fixture((), ());
    seed_plans(&fx);
    fx.env.sql(
        "INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('r-old-1', 'codereview', 'Old', 'done', 'l1', 't', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, deleted_at)
           VALUES ('r-del-1', 'codereview', 'Deleted', 'grounding', 'l1', 't', 't');",
    );
    let before: String = fx.env.query(
        "SELECT json_group_array(json_array(id, phase, locked_at, done_at, deleted_at)) FROM (SELECT * FROM plans ORDER BY id)",
    );
    let file = input(&fx, &review().to_string());
    for id in ["r-old-1", "r-del-1", "r-none-9"] {
        submit_as(&fx, Some(id), "APPROVE", &file, None).ok();
        assert_eq!(any_plan_done(&fx), 0, "{id}");
    }
    let after: String = fx.env.query(
        "SELECT json_group_array(json_array(id, phase, locked_at, done_at, deleted_at)) FROM (SELECT * FROM plans ORDER BY id)",
    );
    assert_eq!(after, before);
    // A live Plan of `codereview` is finished by the same command.
    submit_as(&fx, Some(PLAN), "APPROVE", &file, None).ok();
    assert_eq!(phase(&fx, PLAN), "done");
    assert_eq!(any_plan_done(&fx), 1);
}

#[test]
fn submit_without_a_usable_cpd_plan_finishes_no_plan() {
    let fx = fixture((), ());
    seed_plans(&fx);
    let file = input(&fx, &review().to_string());
    for plan in [None, Some(""), Some("R PAY!"), Some("x")] {
        submit_as(&fx, plan, "APPROVE", &file, None).ok();
        assert_eq!(phase(&fx, PLAN), "grounding", "{plan:?}");
        assert_eq!(any_plan_done(&fx), 0, "{plan:?}");
    }
}

#[test]
fn a_refused_submit_finishes_no_plan() {
    let fx = fixture_at(MOVED);
    seed_plans(&fx);
    let file = input(&fx, &review().to_string());
    refused(
        &submit_as(&fx, Some(PLAN), "APPROVE", &file, Some(SLACK)),
        "head_moved",
        3,
    );
    assert_eq!(phase(&fx, PLAN), "grounding");
    assert_eq!(any_plan_done(&fx), 0);
}

// --- pro-d416: REQUEST_CHANGES keeps the codereview Plan open (a re-review may follow); APPROVE still finishes it.

fn changes_requested(fx: &Fx) -> bool {
    fx.env.read(|c| {
        c.query_row(
            "SELECT changes_requested_at IS NOT NULL FROM plans WHERE id = 'r-pay-812'",
            [],
            |r| r.get(0),
        )
    })
}

#[test]
fn review_submit_request_changes_keeps_codereview_plan_in_grounding() {
    let fx = fixture((), ());
    seed_plans(&fx);
    let file = input(&fx, &review().to_string());
    submit_as(&fx, Some(PLAN), "REQUEST_CHANGES", &file, Some(SLACK)).ok();
    assert_eq!(phase(&fx, PLAN), "grounding");
    assert_eq!(any_plan_done(&fx), 0, "no plan-done Event");
}

#[test]
fn review_submit_request_changes_sets_changes_requested_at() {
    let fx = fixture((), ());
    seed_plans(&fx);
    assert!(!changes_requested(&fx), "the fixture: not set yet");
    let file = input(&fx, &review().to_string());
    submit_as(&fx, Some(PLAN), "REQUEST_CHANGES", &file, None).ok();
    assert!(changes_requested(&fx), "changes_requested_at is set");
}

#[test]
fn review_submit_approve_finishes_codereview_plan() {
    let fx = fixture((), ());
    seed_plans(&fx);
    let file = input(&fx, &review().to_string());
    submit_as(&fx, Some(PLAN), "APPROVE", &file, None).ok();
    assert_eq!(phase(&fx, PLAN), "done");
    assert_eq!(plan_done_events(&fx, PLAN).len(), 1);
}
