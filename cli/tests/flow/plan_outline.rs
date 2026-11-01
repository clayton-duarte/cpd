//! `plan outline`, the Plan summary files and the refusals of `plan review` (commands.md §4.13, §4.5, §4.6; contract
//! §2.3 as amended by hs-d502, D1–D4): the built binary against a temporary `$CPD_HOME` with Plans written straight
//! into the database. Written from the docs, before the code exists.

use crate::common;

use std::fs;
use std::path::PathBuf;

use common::fx::Fx;
use common::refused;
use cpd_core::home::CpdHome;
use cpd_core::ids::PlanId;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const FP: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn home(fx: &Fx) -> CpdHome {
    CpdHome::resolve(Some(fx.env.cpd_home().into()), None).unwrap()
}

/// `run/<plan>/<name>`, beside `plan.md`.
fn run_file(fx: &Fx, plan: &str, name: &str) -> PathBuf {
    home(fx)
        .plan_md(&PlanId::parse(plan).unwrap())
        .with_file_name(name)
}

fn sha256(text: &str) -> String {
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// ---------- fixtures ----------

/// A Plan in `phase` of Context `ctx`, its Lead Session beside it.
fn plan(fx: &Fx, id: &str, phase: &str, archived: bool) {
    let locked = if matches!(phase, "grounding" | "plan") {
        "NULL"
    } else {
        "'2000-01-01T00:00:00.000Z'"
    };
    let done = if phase == "done" {
        "'2000-01-02T00:00:00.000Z'"
    } else {
        "NULL"
    };
    let archived = if archived {
        "'2000-01-03T00:00:00.000Z'"
    } else {
        "NULL"
    };
    fx.env.sql(&format!(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('lead-{id}', 'lead', 'ctx', '{id}', '{id}', '/', 'O3', 'idle', 't', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at, archived_at)
           VALUES ('{id}', 'ctx', 'Title of {id}', '{phase}', 'lead-{id}', '2000-01-01T00:00:00.000Z', {locked}, {done}, {archived});"
    ));
}

/// A PR of `plan`: `status` is `waiting`, `dispatch` or `cancelled`; `name` and `title` as stored (null when `None`).
#[allow(clippy::too_many_arguments)] // one SQL row per call, each column a plain argument of the test helper
fn pr(
    fx: &Fx,
    plan: &str,
    key: &str,
    repo: &str,
    base: &str,
    status: &str,
    name: Option<&str>,
    title: Option<&str>,
    needs: &[&str],
) {
    let (worktree, started) = if status == "dispatch" {
        ("'/w'", "'t'")
    } else {
        ("NULL", "NULL")
    };
    let q = |v: Option<&str>| v.map_or("NULL".to_owned(), |v| format!("'{v}'"));
    fx.env.sql(&format!(
        "INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, created_txn, created_at, started_at, name, title)
         VALUES ('{plan}', '{key}', '{repo}', '{base}', '{status}', 'cpd/{plan}/{key}', {worktree}, cpd_txn(), 't', {started}, {}, {});",
        q(name),
        q(title)
    ));
    for need in needs {
        fx.env.sql(&format!(
            "INSERT INTO pr_needs (plan, pr, needs) VALUES ('{plan}', '{key}', '{need}');"
        ));
    }
}

/// A Job of PR `pr`. `tier` `None` makes a stub draft (no Tier, no Spec). A non-draft Job is locked.
#[allow(clippy::too_many_arguments)] // one SQL row per call, each column a plain argument of the test helper
fn job(
    fx: &Fx,
    plan: &str,
    pr: &str,
    key: &str,
    status: &str,
    tier: Option<&str>,
    touches: &[&str],
    needs: &[&str],
) {
    let (tier, spec) = match tier {
        Some(t) => (
            format!("'{t}'"),
            format!(
                "'Goal.', 'Context.', '{}', '[\"cargo test\"]', '[]'",
                json!(touches)
            ),
        ),
        None => ("NULL".to_owned(), "NULL, NULL, NULL, NULL, NULL".to_owned()),
    };
    let fingerprint = if status == "draft" {
        "NULL".to_owned()
    } else {
        format!("'{FP}'")
    };
    fx.env.sql(&format!(
        "INSERT INTO jobs (plan, pr, key, title, tier, goal, context, touches, acceptance, out_of_scope, status, appended, fingerprint, created_txn, created_at)
         VALUES ('{plan}', '{pr}', '{key}', 'Title {key}', {tier}, {spec}, '{status}', 0, {fingerprint}, cpd_txn(), 't');"
    ));
    for need in needs {
        fx.env.sql(&format!(
            "INSERT INTO dependencies (plan, job, needs) VALUES ('{plan}', '{key}', '{need}');"
        ));
    }
}

/// Context `ctx` with the records `t-d1` and `t-d2`, Context `other` with `t-x1`; Plan `mine` in phase `plan` with the
/// PR `api` (`/r/api`, base `main`) and its draft Job `x`.
fn fixture() -> Fx {
    let fx = Fx::new();
    fs::write(home(&fx).config(), "version = 1\nmachine = \"t\"\n").unwrap();
    fx.env.sql(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 't'), ('other', 'Other', 't', 't');
         INSERT INTO context_records (context, id, kind, at, machine, text) VALUES
           ('ctx', 't-d1', 'decision', '2026-10-20T00:00:00.000Z', 't', 'First'),
           ('ctx', 't-d2', 'decision', '2026-10-20T00:00:01.000Z', 't', 'Second'),
           ('other', 't-x1', 'decision', '2026-10-20T00:00:02.000Z', 't', 'Elsewhere');",
    );
    plan(&fx, "mine", "plan", false);
    pr(
        &fx,
        "mine",
        "api",
        "/r/api",
        "main",
        "waiting",
        Some("api-name"),
        Some("API work"),
        &[],
    );
    job(
        &fx,
        "mine",
        "api",
        "x",
        "draft",
        Some("S2"),
        &["src/x.rs"],
        &[],
    );
    fx
}

/// Writes `value` as the file `name` under the test root; returns its path.
fn file(fx: &Fx, name: &str, text: &str) -> String {
    let path = fx.env.root().join(name);
    fs::write(&path, text).unwrap();
    path.display().to_string()
}

/// The smallest valid outline.
fn minimal() -> Value {
    json!({"goal": "Show the review in chat"})
}

fn outline(fx: &Fx, value: &Value) -> common::Run {
    let path = file(fx, "outline.json", &value.to_string());
    fx.cpd(&["plan", "outline", "--file", &path, "--plan", "mine"])
}

fn count(fx: &Fx, sql: &str) -> i64 {
    fx.env.query(sql)
}

fn outlined_events(fx: &Fx) -> Vec<Value> {
    fx.cpd(&["events", "list", "--kind", "plan-outlined"]).ok()["events"]
        .as_array()
        .unwrap()
        .clone()
}

/// The outline of the summary test: every field set.
fn full_outline() -> Value {
    json!({
        "goal": "Show the review in chat",
        "decisions": [{"question": "Where is it shown?", "answer": "In the Lead's chat"}],
        "assumptions": ["The Lead writes the outline"],
        "risks": ["The dialog API differs between modes"],
        "done_when": {"ui": ["The summary is written", "The dialog opens"]},
        "out_of_scope": ["Changing the dialog API"],
        "decided": ["t-d1", "t-d2"]
    })
}

// ---------- plan outline (D1) ----------

#[test]
fn a_valid_outline_is_stored_canonically_and_plan_show_returns_it() {
    let fx = fixture();
    // Keys out of order and with spaces; absent fields are stored as `[]` (`{}` for done_when).
    let text = "{ \"risks\": [\"Slow\"], \"goal\": \"Show the review\", \"done_when\": {\"api\": [\"It works\"]},\n  \"decided\": [\"t-d1\"] }";
    let path = file(&fx, "outline.json", text);
    assert_eq!(show(&fx)["outline"], Value::Null);

    let out = fx
        .cpd(&["plan", "outline", "--file", &path, "--plan", "mine"])
        .ok();
    let expected = json!({
        "assumptions": [], "decided": ["t-d1"], "decisions": [], "done_when": {"api": ["It works"]},
        "goal": "Show the review", "out_of_scope": [], "risks": ["Slow"]
    });
    assert_eq!(
        (&out["plan"], &out["replaced"], &out["outline"]),
        (&json!("mine"), &json!(false), &expected)
    );
    assert_eq!(show(&fx)["outline"], expected);
    // `plans.outline` holds the canonical JSON: keys sorted, no whitespace.
    let stored: String = fx.env.query("SELECT outline FROM plans WHERE id = 'mine'");
    assert_eq!(
        stored,
        "{\"assumptions\":[],\"decided\":[\"t-d1\"],\"decisions\":[],\"done_when\":{\"api\":[\"It works\"]},\"goal\":\"Show the review\",\"out_of_scope\":[],\"risks\":[\"Slow\"]}"
    );
    let events = outlined_events(&fx);
    assert_eq!(events.len(), 1);
    assert_eq!(
        (&events[0]["plan"], &events[0]["data"]),
        (&json!("mine"), &json!({"replaced": false}))
    );
}

fn show(fx: &Fx) -> Value {
    fx.cpd(&["plan", "show", "--plan", "mine"]).ok()["plan"].clone()
}

#[test]
fn the_same_file_twice_writes_one_event_and_the_same_content_in_another_key_order_too() {
    let fx = fixture();
    let path = file(&fx, "a.json", r#"{"goal": "G", "risks": ["R"]}"#);
    let first = fx
        .cpd(&["plan", "outline", "--file", &path, "--plan", "mine"])
        .ok();
    assert_eq!(first["replaced"], false);
    assert!(first.get("already").is_none(), "{first}");
    let again = fx
        .cpd(&["plan", "outline", "--file", &path, "--plan", "mine"])
        .ok();
    assert_eq!(again["already"], true);
    assert!(again.get("replaced").is_none(), "{again}");
    assert_eq!(again["outline"], first["outline"]);
    // The same content, spelled differently: the same canonical JSON.
    let reordered = file(
        &fx,
        "b.json",
        "{\"risks\":[\"R\"],\n\"goal\":\"G\",\"assumptions\":[]}",
    );
    let third = fx
        .cpd(&["plan", "outline", "--file", &reordered, "--plan", "mine"])
        .ok();
    assert_eq!(third["already"], true);
    assert_eq!(outlined_events(&fx).len(), 1);
}

#[test]
fn a_different_outline_replaces_the_whole_outline_with_replaced_true() {
    let fx = fixture();
    outline(
        &fx,
        &json!({"goal": "One", "risks": ["R"], "assumptions": ["A"]}),
    )
    .ok();
    let out = outline(&fx, &json!({"goal": "Two"})).ok();
    assert_eq!(out["replaced"], true);
    // No merge: what the second file leaves out is empty.
    assert_eq!(show(&fx)["outline"]["risks"], json!([]));
    assert_eq!(show(&fx)["outline"]["assumptions"], json!([]));
    assert_eq!(show(&fx)["outline"]["goal"], "Two");
    let events = outlined_events(&fx);
    assert_eq!(events.len(), 2);
    assert_eq!(events[1]["data"], json!({"replaced": true}));
}

#[test]
fn the_outline_works_in_every_phase() {
    let fx = fixture();
    for phase in ["grounding", "dispatch", "deploy", "done"] {
        plan(&fx, &format!("p-{phase}"), phase, false);
        let path = file(&fx, "o.json", &minimal().to_string());
        let out = fx
            .cpd(&[
                "plan",
                "outline",
                "--file",
                &path,
                "--plan",
                &format!("p-{phase}"),
            ])
            .ok();
        assert_eq!(out["replaced"], false, "{phase}");
    }
}

#[test]
fn the_outline_refusals_that_are_not_about_its_content() {
    let fx = fixture();
    plan(&fx, "old", "plan", true);
    let path = file(&fx, "o.json", &minimal().to_string());
    refused(
        &fx.cpd(&["plan", "outline", "--file", &path, "--plan", "nope"]),
        "plan_not_found",
        4,
    );
    refused(
        &fx.cpd(&["plan", "outline", "--file", &path, "--plan", "old"]),
        "archived",
        3,
    );
    let missing = fx.env.root().join("missing.json").display().to_string();
    refused(
        &fx.cpd(&["plan", "outline", "--file", &missing, "--plan", "mine"]),
        "file_unreadable",
        2,
    );
    assert_eq!(
        count(&fx, "SELECT count(*) FROM plans WHERE outline IS NOT NULL"),
        0
    );
    assert!(outlined_events(&fx).is_empty());
}

/// `text` is refused `outline_invalid` (exit 2) at `field`; nothing is written: no outline, and no new Event.
fn invalid(fx: &Fx, text: &str, field: &str) -> Value {
    let events = outlined_events(fx).len();
    let path = file(fx, "bad.json", text);
    let error = refused(
        &fx.cpd(&["plan", "outline", "--file", &path, "--plan", "mine"]),
        "outline_invalid",
        2,
    );
    assert_eq!(error["details"]["field"], field, "{text}");
    assert_eq!(
        count(fx, "SELECT count(*) FROM plans WHERE outline IS NOT NULL"),
        0,
        "{text}"
    );
    assert_eq!(outlined_events(fx).len(), events, "{text}");
    error
}

#[test]
fn an_outline_with_a_bad_shape_is_invalid_at_the_path_of_the_failure() {
    let fx = fixture();
    let long = "x".repeat(201);
    invalid(&fx, "this is not json", "$");
    invalid(&fx, "[\"goal\"]", "$");
    // Unknown fields are refused, before every other check (the first in byte order).
    invalid(&fx, r#"{"zeta": 1, "bogus": 1}"#, "bogus");
    invalid(&fx, r#"{"bogus": 1}"#, "bogus");
    invalid(&fx, "{}", "goal");
    invalid(&fx, r#"{"goal": ""}"#, "goal");
    invalid(&fx, &json!({"goal": long}).to_string(), "goal");
    invalid(&fx, "{\"goal\": \"two\\nlines\"}", "goal");
    invalid(&fx, "{\"goal\": \"a\\ttab\"}", "goal");
    invalid(
        &fx,
        r#"{"goal": "G", "decisions": [{"question": "Q", "answer": "A"}, {"question": "Q", "answer": ""}]}"#,
        "decisions[1].answer",
    );
    invalid(
        &fx,
        &json!({"goal": "G", "assumptions": ["ok", long]}).to_string(),
        "assumptions[1]",
    );
    invalid(
        &fx,
        &json!({"goal": "G", "done_when": {"api": [long]}}).to_string(),
        "done_when.api[0]",
    );
    invalid(
        &fx,
        r#"{"goal": "G", "out_of_scope": ["1", "2", "3", "4", "5", "6"]}"#,
        "out_of_scope[5]",
    );
}

#[test]
fn an_outline_with_more_than_three_risks_is_invalid() {
    let fx = fixture();
    // Three are allowed; the fourth is the failure, as the contract's `risks[3]`.
    outline(&fx, &json!({"goal": "G", "risks": ["1", "2", "3"]})).ok();
    fx.env.sql("UPDATE plans SET outline = NULL;");
    invalid(
        &fx,
        r#"{"goal": "G", "risks": ["1", "2", "3", "4"]}"#,
        "risks[3]",
    );
}

#[test]
fn a_done_when_key_must_be_a_pr_of_the_plan_that_is_not_cancelled() {
    let fx = fixture();
    pr(
        &fx,
        "mine",
        "gone",
        "/r/gone",
        "main",
        "cancelled",
        None,
        None,
        &[],
    );
    let error = invalid(
        &fx,
        r#"{"goal": "G", "done_when": {"nope": ["Done"]}}"#,
        "done_when.nope",
    );
    assert_eq!(error["details"]["key"], "nope");
    let error = invalid(
        &fx,
        r#"{"goal": "G", "done_when": {"api": ["Done"], "gone": ["Done"]}}"#,
        "done_when.gone",
    );
    assert_eq!(error["details"]["key"], "gone");
    // A PR of another Plan is no PR of this one.
    plan(&fx, "busy", "plan", false);
    pr(
        &fx,
        "busy",
        "svc",
        "/r/svc",
        "main",
        "waiting",
        None,
        None,
        &[],
    );
    invalid(
        &fx,
        r#"{"goal": "G", "done_when": {"svc": ["Done"]}}"#,
        "done_when.svc",
    );
}

#[test]
fn a_decided_id_must_be_a_record_of_the_plans_context() {
    let fx = fixture();
    let error = invalid(&fx, r#"{"goal": "G", "decided": ["t-d999"]}"#, "decided[0]");
    assert_eq!(error["details"]["record"], "t-d999");
    let error = invalid(
        &fx,
        r#"{"goal": "G", "decided": ["t-d1", "t-d999"]}"#,
        "decided[1]",
    );
    assert_eq!(error["details"]["record"], "t-d999");
    // A record of another Context does not count.
    let error = invalid(&fx, r#"{"goal": "G", "decided": ["t-x1"]}"#, "decided[0]");
    assert_eq!(error["details"]["record"], "t-x1");
    // A superseded record does.
    fx.env.sql(
        "INSERT INTO context_records (context, id, kind, at, machine, text, supersedes)
         VALUES ('ctx', 't-d3', 'decision', '2026-10-20T00:00:05.000Z', 't', 'Third', 't-d1');",
    );
    outline(&fx, &json!({"goal": "G", "decided": ["t-d1", "t-d3"]})).ok();
}

// ---------- the plan fingerprint (D2) ----------

/// The plan fingerprint of `fixture()` by today's rule (contract §2.3): `{jobs, prs}`, plus `outline` when set.
fn fingerprint_of_fixture(outline: Option<&str>) -> String {
    let job = "{\"acceptance\":[\"cargo test\"],\"context\":\"Context.\",\"goal\":\"Goal.\",\"key\":\"x\",\"needs\":[],\"out_of_scope\":[],\"pr\":\"api\",\"tier\":\"S2\",\"title\":\"Title x\",\"touches\":[\"src/x.rs\"]}";
    let prs = "[{\"base\":\"main\",\"key\":\"api\",\"needs\":[],\"repo\":\"/r/api\"}]";
    match outline {
        None => sha256(&format!("{{\"jobs\":[{job}],\"prs\":{prs}}}")),
        Some(o) => sha256(&format!(
            "{{\"jobs\":[{job}],\"outline\":{o},\"prs\":{prs}}}"
        )),
    }
}

#[test]
fn a_plan_without_an_outline_keeps_the_fingerprint_of_jobs_and_prs() {
    let fx = fixture();
    let out = fx.cpd(&["plan", "render", "--plan", "mine"]).ok();
    assert_eq!(out["fingerprint"], fingerprint_of_fixture(None));
}

#[test]
fn a_plan_with_an_outline_has_the_member_outline_in_its_fingerprint() {
    let fx = fixture();
    outline(&fx, &json!({"goal": "G", "done_when": {"api": ["Done"]}})).ok();
    let canonical = "{\"assumptions\":[],\"decided\":[],\"decisions\":[],\"done_when\":{\"api\":[\"Done\"]},\"goal\":\"G\",\"out_of_scope\":[],\"risks\":[]}";
    let out = fx.cpd(&["plan", "render", "--plan", "mine"]).ok();
    assert_eq!(out["fingerprint"], fingerprint_of_fixture(Some(canonical)));
    assert_ne!(out["fingerprint"], fingerprint_of_fixture(None));
}

#[test]
fn changing_the_outline_after_a_review_makes_the_review_not_current() {
    let fx = fixture();
    outline(&fx, &json!({"goal": "One", "done_when": {"api": ["Done"]}})).ok();
    let r1 = fx
        .cpd(&[
            "plan",
            "review",
            "--plan",
            "mine",
            "--title",
            "Review title",
        ])
        .ok();
    assert_eq!(r1["review"]["id"], "r1");
    outline(&fx, &json!({"goal": "Two", "done_when": {"api": ["Done"]}})).ok();

    let decided = fx
        .cpd(&["plan", "decide", "r1", "--approve", "--plan", "mine"])
        .ok();
    assert_eq!(decided["current"], false);
    // The review of the changed Plan is a new one, not the pending one.
    let r2 = fx
        .cpd(&[
            "plan",
            "review",
            "--plan",
            "mine",
            "--title",
            "Review title",
        ])
        .ok();
    assert_eq!(r2["review"]["id"], "r2");
    assert_ne!(r2["review"]["fingerprint"], r1["review"]["fingerprint"]);
}

// ---------- plan review refusals (D3) ----------

/// Nothing a refused review could write: the rows, the Events and the files of the review.
fn state(fx: &Fx) -> (i64, i64, bool, bool, bool) {
    (
        count(fx, "SELECT count(*) FROM plan_reviews"),
        count(fx, "SELECT count(*) FROM events"),
        run_file(fx, "mine", "plan.md").exists(),
        run_file(fx, "mine", "summary.md").exists(),
        run_file(fx, "mine", "summary-r1.md").exists(),
    )
}

#[test]
fn a_review_of_a_plan_without_an_outline_is_refused_and_writes_nothing() {
    let fx = fixture();
    let before = state(&fx);
    let error = refused(
        &fx.cpd(&[
            "plan",
            "review",
            "--plan",
            "mine",
            "--title",
            "Review title",
        ]),
        "outline_missing",
        3,
    );
    assert_eq!(
        error["details"]["next"],
        "cpd plan outline --file <outline.json>"
    );
    assert_eq!(state(&fx), before);
    assert_eq!(before, (0, before.1, false, false, false));
}

#[test]
fn a_pending_review_made_before_the_outline_does_not_let_a_plan_without_one_through() {
    let fx = fixture();
    let fingerprint = fx.cpd(&["plan", "render", "--plan", "mine"]).ok()["fingerprint"]
        .as_str()
        .unwrap()
        .to_owned();
    fx.env.sql(&format!(
        "INSERT INTO plan_reviews (plan, id, number, fingerprint, status, requested_at) VALUES ('mine', 'r1', 1, '{fingerprint}', 'pending', 't');"
    ));
    refused(
        &fx.cpd(&[
            "plan",
            "review",
            "--plan",
            "mine",
            "--title",
            "Review title",
        ]),
        "outline_missing",
        3,
    );
    assert_eq!(count(&fx, "SELECT count(*) FROM plan_reviews"), 1);
}

#[test]
fn a_review_with_a_pr_that_has_a_draft_and_no_done_when_is_refused_and_writes_nothing() {
    let fx = fixture();
    pr(
        &fx,
        "mine",
        "ui",
        "/r/ui",
        "main",
        "waiting",
        None,
        None,
        &[],
    );
    job(
        &fx,
        "mine",
        "ui",
        "y",
        "draft",
        Some("S2"),
        &["src/y.rs"],
        &[],
    );
    pr(
        &fx,
        "mine",
        "idle",
        "/r/idle",
        "main",
        "waiting",
        None,
        None,
        &[],
    );
    pr(
        &fx,
        "mine",
        "gone",
        "/r/gone",
        "main",
        "cancelled",
        None,
        None,
        &[],
    );
    // `idle` has no draft and `gone` is cancelled: neither needs an entry. `api` has one, `ui` has none.
    outline(&fx, &json!({"goal": "G", "done_when": {"api": ["Done"]}})).ok();
    let before = state(&fx);
    let error = refused(
        &fx.cpd(&[
            "plan",
            "review",
            "--plan",
            "mine",
            "--title",
            "Review title",
        ]),
        "outline_incomplete",
        3,
    );
    assert_eq!(error["details"]["prs"], json!(["ui"]));
    assert_eq!(state(&fx), before);

    // Every PR with a draft covered: the review goes through. `idle` is ended first: a live PR with no draft Job
    // is refused `pr_empty` (after the outline check above), and an ended PR needs no entry either.
    fx.env
        .sql("UPDATE prs SET status = 'cancelled' WHERE plan = 'mine' AND key = 'idle';");
    outline(
        &fx,
        &json!({"goal": "G", "done_when": {"api": ["Done"], "ui": ["Done too"]}}),
    )
    .ok();
    fx.cpd(&[
        "plan",
        "review",
        "--plan",
        "mine",
        "--title",
        "Review title",
    ])
    .ok();
}

#[test]
fn a_pr_with_only_locked_jobs_needs_no_done_when() {
    let fx = fixture();
    fx.env
        .sql("UPDATE plans SET phase = 'dispatch', locked_at = 't' WHERE id = 'mine';");
    pr(
        &fx,
        "mine",
        "old",
        "/r/old",
        "main",
        "dispatch",
        None,
        None,
        &[],
    );
    job(
        &fx,
        "mine",
        "old",
        "z",
        "queued",
        Some("S2"),
        &["src/z.rs"],
        &[],
    );
    outline(&fx, &json!({"goal": "G", "done_when": {"api": ["Done"]}})).ok();
    let out = fx
        .cpd(&[
            "plan",
            "review",
            "--plan",
            "mine",
            "--title",
            "Review title",
        ])
        .ok();
    assert_eq!(out["review"]["id"], "r1");
}

// ---------- the summary files (D4) ----------

/// Plan `mine` in phase `dispatch`: PR `api` (dispatch, one locked Job, no draft), PR `ui` (waiting, stacked on `api`
/// and needing it) with the drafts `u-one` and `u-two`, and Plan `busy` with two Jobs overlapping `u-one`.
fn two_pr_plan() -> Fx {
    let fx = Fx::new();
    fs::write(home(&fx).config(), "version = 1\nmachine = \"t\"\n").unwrap();
    fx.env.sql(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 't');
         INSERT INTO context_records (context, id, kind, at, machine, text) VALUES
           ('ctx', 't-d1', 'decision', '2026-10-20T00:00:00.000Z', 't', 'First'),
           ('ctx', 't-d2', 'decision', '2026-10-20T00:00:01.000Z', 't', 'Second');",
    );
    // Built in phase `plan` (the PR triggers refuse a needs row after the lock), then locked.
    plan(&fx, "mine", "plan", false);
    pr(
        &fx,
        "mine",
        "api",
        "/r/api",
        "main",
        "dispatch",
        None,
        None,
        &[],
    );
    job(
        &fx,
        "mine",
        "api",
        "a-locked",
        "queued",
        Some("S2"),
        &["core/x.rs"],
        &[],
    );
    pr(
        &fx,
        "mine",
        "ui",
        "/r/ui",
        "cpd/mine/api",
        "waiting",
        None,
        Some("Build the UI"),
        &["api"],
    );
    job(
        &fx,
        "mine",
        "ui",
        "u-one",
        "draft",
        Some("S3"),
        &["web/a.rs", "web/b.rs"],
        &[],
    );
    job(
        &fx,
        "mine",
        "ui",
        "u-two",
        "draft",
        Some("O3"),
        &["web/c.rs"],
        &["u-one"],
    );
    fx.env.sql("UPDATE plans SET phase = 'dispatch', locked_at = '2000-01-01T00:00:00.000Z' WHERE id = 'mine';");
    plan(&fx, "busy", "dispatch", false);
    pr(
        &fx,
        "busy",
        "svc",
        "/r/ui",
        "main",
        "dispatch",
        None,
        None,
        &[],
    );
    job(
        &fx,
        "busy",
        "svc",
        "y",
        "queued",
        Some("S2"),
        &["web/a.rs"],
        &[],
    );
    job(
        &fx,
        "busy",
        "svc",
        "z",
        "draft",
        Some("S2"),
        &["web/b.rs"],
        &[],
    );
    fx
}

#[test]
fn a_review_writes_summary_r1_with_every_row_in_order_and_returns_no_path() {
    let fx = two_pr_plan();
    outline(&fx, &full_outline()).ok();
    let out = fx
        .cpd(&[
            "plan",
            "review",
            "--plan",
            "mine",
            "--title",
            "Review title",
        ])
        .ok();
    let summary = run_file(&fx, "mine", "summary-r1.md");
    assert_eq!(out["summary"], summary.display().to_string());
    assert!(out.get("path").is_none(), "{out}");

    let expected = "# Review title
Plan mine: Title of mine · review r1

Goal: Show the review in chat

## PRs
- ui (ui, base cpd/mine/api): Build the UI
  Done when: The summary is written
  Done when: The dialog opens

## Decisions
D1 Where is it shown? → In the Lead's chat

## Assumptions
A1 The Lead writes the outline

## Jobs (2 under review, 1 locked)
| Job | Title | Tier | Needs |
|---|---|---|---|
| u-one | Title u-one | S3 | — |
| u-two | Title u-two | O3 | u-one |

Order: api → ui
Stacked on: ui on mine/api

Overlaps: 1
- web/a.rs: busy/svc/y (queued) +2

Decided: t-d1, t-d2

Later: PR api

## Risks
R1 The dialog API differs between modes

## Out of scope
- Changing the dialog API

Approve | Change
"
    .to_owned();
    assert_eq!(fs::read_to_string(&summary).unwrap(), expected);

    // Asking again returns the pending review and its paths, and changes nothing.
    let again = fx
        .cpd(&[
            "plan",
            "review",
            "--plan",
            "mine",
            "--title",
            "Review title",
        ])
        .ok();
    assert_eq!(
        (&again["already"], &again["summary"]),
        (&json!(true), &out["summary"])
    );
    assert_eq!(fs::read_to_string(&summary).unwrap(), expected);
}

#[test]
fn the_summary_of_a_review_is_a_copy_that_a_later_render_cannot_change() {
    let fx = two_pr_plan();
    outline(&fx, &full_outline()).ok();
    fx.cpd(&[
        "plan",
        "review",
        "--plan",
        "mine",
        "--title",
        "Review title",
    ])
    .ok();
    let reviewed = fs::read_to_string(run_file(&fx, "mine", "summary-r1.md")).unwrap();

    let mut changed = full_outline();
    changed["goal"] = json!("Another goal");
    outline(&fx, &changed).ok();
    let out = fx.cpd(&["plan", "render", "--plan", "mine"]).ok();
    assert_eq!(
        out["summary"],
        run_file(&fx, "mine", "summary.md").display().to_string()
    );
    assert_eq!(
        fs::read_to_string(run_file(&fx, "mine", "summary-r1.md")).unwrap(),
        reviewed
    );
    assert!(
        fs::read_to_string(run_file(&fx, "mine", "summary.md"))
            .unwrap()
            .contains("Goal: Another goal\n")
    );
}

#[test]
fn a_render_without_an_outline_writes_summary_md_with_an_empty_goal_and_the_stubs_as_later() {
    let fx = fixture();
    job(&fx, "mine", "api", "later-one", "draft", None, &[], &[]);
    pr(
        &fx,
        "mine",
        "next",
        "/r/next",
        "main",
        "waiting",
        None,
        None,
        &[],
    );

    let out = fx.cpd(&["plan", "render", "--plan", "mine"]).ok();
    let path = run_file(&fx, "mine", "summary.md");
    assert_eq!(out["summary"], path.display().to_string());
    // The sections of the outline are left out, the Goal is `—`, and the heading and the last line are the render's.
    let expected = format!(
        "# Plan: Title of mine
Plan mine · phase plan

Goal: —

## PRs
- api (api-name, base main): API work

## Jobs (1 under review, 0 locked)
| Job | Title | Tier | Needs |
|---|---|---|---|
| x | Title x | S2 | — |

Order: api · next
Stacked on: none

Overlaps: none

Later: later-one, PR next

Full plan ({})
",
        run_file(&fx, "mine", "plan.md").display()
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), expected);
    // Rendering again writes the same bytes.
    fx.cpd(&["plan", "render", "--plan", "mine"]).ok();
    assert_eq!(fs::read_to_string(&path).unwrap(), expected);
}

#[test]
fn the_summary_has_no_spec_no_mermaid_and_no_row_for_a_locked_job() {
    let fx = two_pr_plan();
    outline(&fx, &full_outline()).ok();
    fx.cpd(&[
        "plan",
        "review",
        "--plan",
        "mine",
        "--title",
        "Review title",
    ])
    .ok();
    for name in ["summary-r1.md", "summary.md"] {
        fx.cpd(&["plan", "render", "--plan", "mine"]).ok();
        let text = fs::read_to_string(run_file(&fx, "mine", name)).unwrap();
        assert!(
            !text.contains("mermaid") && !text.contains("#### Goal") && !text.contains("a-locked"),
            "{name}: {text}"
        );
    }
}
