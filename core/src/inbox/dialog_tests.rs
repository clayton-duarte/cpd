//! The label of a `dialog` item (contract §2.2, §3.7 E8; pro-d150): a Session blocked with a `verdict: ` state label is
//! named by it, `<who> <state_label>`; any other dialog is `<who> asks you`.

use super::model::{Condition, Kind, Urgency};
use super::rules::conditions_with;
use crate::home::CpdHome;
use crate::store::Store;
use time::OffsetDateTime;

const T: &str = "2026-01-01T00:00:00.000Z";

struct Fixture {
    _dir: tempfile::TempDir,
    home: CpdHome,
    store: Store,
}

/// A Context `ctx` with its Manager `mgr`, and a Plan `pay` in `phase` with its Lead `l-pay`; all `working`.
fn fixture(phase: &str) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    let mut f = Fixture { _dir: dir, home, store };
    let locked = if matches!(phase, "grounding" | "plan") { "NULL".to_owned() } else { format!("'{T}'") };
    let done = if phase == "done" { format!("'{T}'") } else { "NULL".to_owned() };
    f.sql(&format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');
         INSERT INTO sessions (id, role, context, name, cwd, tier, state, state_at, created_at)
           VALUES ('mgr', 'manager', 'ctx', 'Manager', '/', 'O3', 'working', '{T}', '{T}');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('l-pay', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'working', '{T}', '{T}');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay', 'ctx', 'Pay', '{phase}', 'l-pay', '{T}', {locked}, {done});"
    ));
    f
}

impl Fixture {
    fn sql(&mut self, sql: &str) {
        self.store
            .write("fixture", |tx| {
                tx.execute_batch(sql)?;
                Ok(())
            })
            .unwrap();
    }

    /// A side fork: a Session of role lead, with `plan` or none.
    fn fork(&mut self, id: &str, plan: Option<&str>) {
        let plan = plan.map_or("NULL".to_owned(), |p| format!("'{p}'"));
        self.sql(&format!(
            "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
             VALUES ('{id}', 'lead', 'ctx', {plan}, 'n', '/', 'O3', 'working', '{T}', '{T}');"
        ));
    }

    /// The Session is in `state` with `label`, and its `session-state` Event is written.
    fn state(&mut self, session: &str, state: &str, label: &str) {
        let label = if state == "working" { "NULL".to_owned() } else { format!("'{}'", label.replace('\'', "''")) };
        self.sql(&format!(
            "UPDATE sessions SET state = '{state}', state_label = {label} WHERE id = '{session}';
             INSERT INTO events (txn, at, kind, context, session, text, data)
             VALUES (cpd_txn(), '{T}', 'session-state', 'ctx', '{session}', 'x', '{{\"from\":\"working\",\"label\":null,\"to\":\"{state}\"}}');"
        ));
    }

    fn dialogs(&mut self) -> Vec<Condition> {
        let now = OffsetDateTime::parse("2026-01-01T00:00:00Z", &time::format_description::well_known::Rfc3339).unwrap();
        let all = conditions_with(&mut self.store, &self.home, now, 20, 1, None).unwrap();
        all.into_iter().filter(|c| c.kind == Kind::Dialog).collect()
    }
}

fn one(mut found: Vec<Condition>) -> Condition {
    assert_eq!(found.len(), 1, "{found:?}");
    found.remove(0)
}

#[test]
fn a_lead_blocked_with_a_verdict_label_is_a_dialog_named_by_its_verdict() {
    for (phase, urgency) in [
        ("grounding", Urgency::Magenta),
        ("plan", Urgency::Magenta),
        ("dispatch", Urgency::Red),
        ("deploy", Urgency::Red),
        ("done", Urgency::Magenta),
    ] {
        let mut f = fixture(phase);
        f.state("l-pay", "blocked", "verdict: Approve with comments");
        let c = one(f.dialogs());
        assert_eq!(c.label, "pay verdict: Approve with comments", "{phase}");
        assert_eq!(c.urgency, urgency, "{phase}");
        assert_eq!(c.subject, "session:l-pay");
    }
}

#[test]
fn a_manager_and_a_fork_with_a_verdict_label_name_themselves_in_it() {
    let mut f = fixture("dispatch");
    f.fork("fork-1234", Some("pay"));
    f.fork("loose-fork-id", None);
    f.state("mgr", "blocked", "verdict: Reject");
    f.state("fork-1234", "blocked", "verdict: Approve");
    f.state("loose-fork-id", "blocked", "verdict: Approve");
    let found = f.dialogs();
    assert_eq!(found.len(), 3);
    let label = |subject: &str| found.iter().find(|c| c.subject == subject).unwrap().label.clone();
    assert_eq!(label("session:mgr"), "ctx verdict: Reject");
    assert_eq!(label("session:fork-1234"), "pay·fork verdict: Approve");
    assert_eq!(label("session:loose-fork-id"), "loose-fo verdict: Approve");
}

#[test]
fn only_the_verdict_prefix_names_the_verdict() {
    for (state_label, expected) in [
        ("verdict: Approve", "pay verdict: Approve"),
        ("verdict:Approve", "pay asks you"),
        ("Verdict: Approve", "pay asks you"),
        ("verdict", "pay asks you"),
        ("dialog", "pay asks you"),
        ("φ: Worker question", "pay asks you"),
        ("Which repo?", "pay asks you"),
    ] {
        let mut f = fixture("dispatch");
        f.state("l-pay", "blocked", state_label);
        assert_eq!(one(f.dialogs()).label, expected, "{state_label}");
    }
}

// ---- not named: these pass today and must keep passing ----

#[test]
fn a_verdict_labelled_session_that_is_working_or_waiting_has_no_dialog() {
    for state in ["working", "waiting"] {
        let mut f = fixture("dispatch");
        f.state("l-pay", state, "verdict: Approve");
        assert!(f.dialogs().is_empty(), "{state}");
    }
}

#[test]
fn the_plan_review_and_loop_guard_labels_are_still_not_dialogs() {
    let mut f = fixture("dispatch");
    f.state("l-pay", "blocked", "φ: plan review");
    f.state("mgr", "blocked", crate::session::LOOP_GUARD_LABEL);
    assert!(f.dialogs().is_empty());
}
