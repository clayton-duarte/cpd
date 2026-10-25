//! `cpd review open|close|list` (hs-d496): the plannotator sessions a Session has open, run through the command tree in this process (`Fx`), against
//! a temporary `$CPD_HOME`. The Session is written straight into the database, as a fixture.
//!
//! Assumptions where the docs leave the output open (guess, to settle with pnbox-corei): `review list` prints the open
//! rows as `reviews`, each with `pn_id`, `kind`, `subject` and `port`; a malformed `--pn`, `--kind` or `--ended` is `usage`.

use crate::common;

use common::fx::Fx;
use common::refused;
use serde_json::{Value, json};

const PN: &str = "pn-0a1b2c";
const OTHER: &str = "pn-0d3e4f";

/// Context `ctx` with its Manager `mgr`, idle.
fn fx() -> Fx {
    let fx = Fx::new();
    std::fs::write(fx.env.cpd_home().join("config.toml"), "version = 1\nmachine = \"m1\"\n").unwrap();
    fx.env.sql(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Context', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
         VALUES ('mgr', 'manager', 'ctx', NULL, 'Manager', '/', 'O3', 'idle', 't', 't');",
    );
    fx
}

fn open(fx: &Fx, pn: &str, kind: &str, subject: &str, port: &str) -> common::Run {
    fx.cpd(&["review", "open", "--session", "mgr", "--pn", pn, "--kind", kind, "--subject", subject, "--port", port])
}

fn list(fx: &Fx) -> Vec<Value> {
    fx.cpd(&["review", "list", "--session", "mgr"]).ok()["reviews"].as_array().expect("`reviews` is an array").clone()
}

fn pns(fx: &Fx) -> Vec<String> {
    let mut pns: Vec<String> = list(fx).iter().map(|r| r["pn_id"].as_str().unwrap().to_owned()).collect();
    pns.sort();
    pns
}

fn events(fx: &Fx, kind: &str) -> Vec<Value> {
    fx.cpd(&["events", "list", "--kind", kind]).ok()["events"].as_array().unwrap().clone()
}

#[test]
fn open_then_list_then_close() {
    let fx = fx();
    assert_eq!(list(&fx), Vec::<Value>::new(), "nothing open yet");
    open(&fx, PN, "annotate", "docs/plan.md", "20001").ok();
    let reviews = list(&fx);
    assert_eq!(reviews.len(), 1, "{reviews:?}");
    let fields = |r: &Value| (r["pn_id"].clone(), r["kind"].clone(), r["subject"].clone(), r["port"].clone());
    assert_eq!(fields(&reviews[0]), (json!(PN), json!("annotate"), json!("docs/plan.md"), json!(20001)));
    fx.cpd(&["review", "close", "--session", "mgr", "--pn", PN]).ok();
    assert_eq!(list(&fx), Vec::<Value>::new(), "a closed row is no longer listed");
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM plannotator_reviews WHERE closed_at IS NOT NULL"), 1, "closed, not deleted");
}

#[test]
fn open_is_idempotent_on_session_and_pn_id() {
    let fx = fx();
    open(&fx, PN, "plan", "first", "20001").ok();
    open(&fx, PN, "plan", "first", "20001").ok();
    assert_eq!(pns(&fx), [PN]);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM plannotator_reviews"), 1);
    assert_eq!(events(&fx, "review-opened").len(), 1, "the second open changed nothing and wrote no Event");
}

#[test]
fn close_by_pn_leaves_the_others_and_close_all_closes_every_open_one() {
    let fx = fx();
    open(&fx, PN, "annotate", "a", "20001").ok();
    open(&fx, OTHER, "review", "b", "20002").ok();
    fx.cpd(&["review", "close", "--session", "mgr", "--pn", PN]).ok();
    assert_eq!(pns(&fx), [OTHER]);
    fx.cpd(&["review", "close", "--session", "mgr", "--all"]).ok();
    assert_eq!(pns(&fx), Vec::<String>::new());
}

#[test]
fn close_is_idempotent_and_closing_what_is_not_open_changes_nothing() {
    let fx = fx();
    fx.cpd(&["review", "close", "--session", "mgr", "--pn", PN]).ok();
    fx.cpd(&["review", "close", "--session", "mgr", "--all"]).ok();
    assert_eq!(events(&fx, "review-closed").len(), 0, "nothing was open: no Event");
    open(&fx, PN, "last", "s", "20001").ok();
    fx.cpd(&["review", "close", "--session", "mgr", "--pn", PN, "--ended", "lost"]).ok();
    fx.cpd(&["review", "close", "--session", "mgr", "--pn", PN, "--ended", "decided"]).ok();
    assert_eq!(events(&fx, "review-closed").len(), 1, "the second close changed nothing");
    assert_eq!(fx.env.query::<String>("SELECT ended FROM plannotator_reviews"), "lost", "the first close's `ended` stays");
}

#[test]
fn a_close_says_decided_unless_told_otherwise() {
    let fx = fx();
    open(&fx, PN, "plan", "s", "20001").ok();
    fx.cpd(&["review", "close", "--session", "mgr", "--pn", PN]).ok();
    assert_eq!(fx.env.query::<String>("SELECT ended FROM plannotator_reviews"), "decided");
}

#[test]
fn open_and_close_append_a_review_opened_and_a_review_closed_event_for_the_session() {
    let fx = fx();
    open(&fx, PN, "review", "src/lib.rs", "20001").ok();
    fx.cpd(&["review", "close", "--session", "mgr", "--pn", PN, "--ended", "lost"]).ok();
    let opened = events(&fx, "review-opened");
    assert_eq!(opened.len(), 1, "{opened:?}");
    assert_eq!(opened[0]["session"], "mgr");
    assert_eq!(opened[0]["context"], "ctx");
    assert_eq!(opened[0]["data"], json!({"pn_id": PN, "kind": "review", "subject": "src/lib.rs", "port": 20001}));
    let closed = events(&fx, "review-closed");
    assert_eq!(closed.len(), 1, "{closed:?}");
    assert_eq!(closed[0]["session"], "mgr");
    assert_eq!(closed[0]["data"], json!({"pn_id": PN, "ended": "lost"}));
}

#[test]
fn an_unknown_session_is_refused_by_all_three_commands() {
    let fx = fx();
    for argv in [
        &["review", "open", "--session", "nobody", "--pn", PN, "--kind", "plan", "--subject", "s", "--port", "20001"][..],
        &["review", "close", "--session", "nobody", "--all"],
        &["review", "list", "--session", "nobody"],
    ] {
        refused(&fx.cpd(argv), "session_not_found", 4);
    }
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM plannotator_reviews"), 0);
}

#[test]
fn an_invalid_pn_id_kind_or_ended_is_a_usage_error_and_writes_nothing() {
    let fx = fx();
    for pn in ["pn-0A1B2C", "pn-0a1b2", "pn-0a1b2cd", "0a1b2c", "pn-ghijkl", ""] {
        refused(&open(&fx, pn, "plan", "s", "20001"), "usage", 2);
    }
    for kind in ["diff", "Plan", ""] {
        refused(&open(&fx, PN, kind, "s", "20001"), "usage", 2);
    }
    refused(&open(&fx, PN, "plan", "", "20001"), "usage", 2);
    refused(&open(&fx, PN, "plan", &"x".repeat(201), "20001"), "usage", 2);
    for port in ["0", "65536", "x"] {
        refused(&open(&fx, PN, "plan", "s", port), "usage", 2);
    }
    refused(&fx.cpd(&["review", "close", "--session", "mgr", "--pn", "nope"]), "usage", 2);
    refused(&fx.cpd(&["review", "close", "--session", "mgr", "--pn", PN, "--ended", "gone"]), "usage", 2);
    refused(&fx.cpd(&["review", "close", "--session", "mgr"]), "usage", 2);
    refused(&fx.cpd(&["review", "close", "--session", "mgr", "--pn", PN, "--all"]), "usage", 2);
    assert_eq!(fx.env.query::<i64>("SELECT count(*) FROM plannotator_reviews"), 0);
}
