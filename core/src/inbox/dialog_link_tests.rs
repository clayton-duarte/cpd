//! The url of a Verdict Dialog item (decision pro-d412): a Dialog of a codereview Plan with a reviewed pull request
//! carries its url; every other item keeps none.

use super::model::{Condition, Filter, Kind, State, list, reconcile};
use super::rules::conditions_with;
use crate::home::CpdHome;
use crate::store::Store;
use time::OffsetDateTime;

const T: &str = "2026-01-01T00:00:00.000Z";
const URL: &str = "https://github.com/acme/pay/pull/7";

struct Fixture {
    _dir: tempfile::TempDir,
    home: CpdHome,
    store: Store,
}

/// A codereview Context with a Plan `rev` and its Lead `l-rev`, blocked with `label`; `reviewed` stores the PR.
fn fixture(reviewed: bool, label: &str) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    let mut f = Fixture {
        _dir: dir,
        home,
        store,
    };
    f.sql(&format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('codereview', 'Review', '{T}', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, state_label)
           VALUES ('l-rev', 'lead', 'codereview', 'rev', 'Lead', '/', 'O3', 'blocked', '{T}', '{T}', '{label}');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('rev', 'codereview', 'Review pay#7 Fix', 'dispatch', 'l-rev', '{T}', '{T}');
         INSERT INTO events (txn, at, kind, context, session, text, data)
           VALUES (cpd_txn(), '{T}', 'session-state', 'codereview', 'l-rev', 'x', '{{\"from\":\"working\",\"label\":null,\"to\":\"blocked\"}}');"
    ));
    if reviewed {
        f.review();
    }
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

    fn review(&mut self) {
        self.sql(
            "UPDATE plans SET reviewed_repo = 'acme/pay', reviewed_number = 7, reviewed_title = 'Fix' WHERE id = 'rev';",
        );
    }

    fn conditions(&mut self, kind: Kind) -> Vec<Condition> {
        let now = OffsetDateTime::parse(
            "2026-01-01T00:00:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();
        conditions_with(&mut self.store, &self.home, now, 20, 1, None)
            .unwrap()
            .into_iter()
            .filter(|c| c.kind == kind)
            .collect()
    }
}

fn one(mut found: Vec<Condition>) -> Condition {
    assert_eq!(found.len(), 1, "{found:?}");
    found.remove(0)
}

#[test]
fn a_verdict_dialog_of_a_review_plan_carries_the_pr_url() {
    let mut f = fixture(true, "verdict: Approve");
    let c = one(f.conditions(Kind::Dialog));
    assert_eq!(c.url.as_deref(), Some(URL));
    assert_eq!(c.session.as_ref().map(|s| s.as_str()), Some("l-rev"));
}

#[test]
fn a_dialog_of_a_plan_without_a_reviewed_pr_has_no_url() {
    // A review Plan whose PR is stored carries the url; the same Plan without one must not. Pin both so the test is red today.
    let mut with = fixture(true, "verdict: Approve");
    assert_eq!(one(with.conditions(Kind::Dialog)).url.as_deref(), Some(URL));
    let mut f = fixture(false, "verdict: Approve");
    assert_eq!(one(f.conditions(Kind::Dialog)).url, None);
}

#[test]
fn a_non_dialog_seat_of_a_review_plan_has_no_url() {
    let mut f = fixture(true, "φ: loop guard");
    let c = one(f.conditions(Kind::LoopGuard));
    assert_eq!(c.url, None);
    // the PR is stored, so a Dialog of this Plan would carry it
    f.sql("UPDATE sessions SET state_label = 'verdict: Approve' WHERE id = 'l-rev';");
    assert_eq!(one(f.conditions(Kind::Dialog)).url.as_deref(), Some(URL));
}

#[test]
fn an_open_dialog_item_gets_the_url_on_the_next_reconcile() {
    let mut f = fixture(false, "verdict: Approve");
    let first = f.conditions(Kind::Dialog);
    reconcile(&mut f.store, &first, T).unwrap();
    f.review();
    let next = f.conditions(Kind::Dialog);
    reconcile(&mut f.store, &next, "2026-01-01T00:01:00.000Z").unwrap();
    let open = f
        .store
        .read(|c| {
            list(
                c,
                &Filter {
                    context: None,
                    state: State::Open,
                },
            )
        })
        .unwrap();
    let item = open.iter().find(|i| i.kind == Kind::Dialog).unwrap();
    assert_eq!(item.url.as_deref(), Some(URL));
}
