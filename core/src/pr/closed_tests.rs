//! The closed-unmerged predicate: an `open` PR whose stored pull request is `closed`.

use super::closed::{ClosedUnmerged, closed_unmerged};
use crate::home::CpdHome;
use crate::ids::{ContextId, PlanId, PrKey, SessionId};
use crate::store::Store;

const T: &str = "2026-01-01T00:00:00.000Z";

struct Fixture {
    _dir: tempfile::TempDir,
    store: Store,
}

/// Context `ctx` with Plans `pay` and `shop`, each with its Lead.
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    let mut f = Fixture { _dir: dir, store };
    f.sql(&format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', '{T}', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('l-pay', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', '{T}', '{T}'),
                  ('l-shop', 'lead', 'ctx', 'shop', 'Lead', '/', 'O3', 'idle', '{T}', '{T}');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'dispatch', 'l-pay', '{T}', '{T}'),
                  ('shop', 'ctx', 'Shop', 'dispatch', 'l-shop', '{T}', '{T}');"
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

    /// A pull request `acme/api#n` in `state` (`merged` has its `merged_at`).
    fn pull(&mut self, n: i64, state: &str) {
        let merged = if state == "merged" { format!("'{T}'") } else { "NULL".to_owned() };
        self.sql(&format!(
            "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, merged_at, changed_at)
             VALUES ('acme/api', {n}, 'https://github.com/acme/api/pull/{n}', 'Title {n}', 0, '{state}', 'none', 'none', '{T}', {merged}, '{T}');"
        ));
    }

    /// PR `key` of `plan` in `status` (`open` or `merged`) on pull request `n`.
    fn pr_on(&mut self, plan: &str, key: &str, status: &str, n: i64) {
        let merged = if status == "merged" { format!("'{T}'") } else { "NULL".to_owned() };
        self.sql(&format!(
            "INSERT INTO prs (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at, pull_request,
                              created_txn, created_at, started_at, merged_at)
             VALUES ('{plan}', '{key}', '/r/acme-api', 'main', '{status}', 'cpd/{plan}/{key}', '/lw/{plan}/{key}', {n},
                     'https://github.com/acme/api/pull/{n}', '{T}', (SELECT id FROM pull_requests WHERE number = {n}), cpd_txn(), '{T}', '{T}', {merged});"
        ));
    }

    fn found(&mut self, plan: Option<&str>) -> Vec<ClosedUnmerged> {
        self.store.read(|c| closed_unmerged(c, plan)).unwrap()
    }
}

fn row(plan: &str, key: &str, lead: &str, n: i64) -> ClosedUnmerged {
    ClosedUnmerged {
        plan: PlanId::parse(plan).unwrap(),
        pr: PrKey::parse(key).unwrap(),
        context: ContextId::parse("ctx").unwrap(),
        lead_session: Some(SessionId::parse(lead).unwrap()),
        repo: "acme/api".to_owned(),
        number: n,
        url: format!("https://github.com/acme/api/pull/{n}"),
        title: format!("Title {n}"),
    }
}

#[test]
fn an_open_pr_whose_pull_request_is_closed_is_closed_unmerged() {
    let mut f = fixture();
    f.pull(7, "closed");
    f.pr_on("pay", "api", "open", 7);
    assert_eq!(f.found(None), [row("pay", "api", "l-pay", 7)]);
}

#[test]
fn an_open_pr_whose_pull_request_is_open_is_not_closed_unmerged() {
    let mut f = fixture();
    f.pull(7, "open");
    f.pr_on("pay", "api", "open", 7);
    assert_eq!(f.found(None), []);
}

#[test]
fn a_cancelled_pr_whose_pull_request_is_closed_is_not_closed_unmerged() {
    let mut f = fixture();
    f.pull(7, "closed");
    f.pr_on("pay", "api", "open", 7);
    // `pr cancel` of an open PR: the status is all that the predicate may read.
    f.sql("PRAGMA ignore_check_constraints = ON; UPDATE prs SET status = 'cancelled' WHERE key = 'api'; PRAGMA ignore_check_constraints = OFF;");
    assert_eq!(f.found(None), []);
}

#[test]
fn a_merged_pr_is_not_closed_unmerged() {
    let mut f = fixture();
    f.pull(7, "merged");
    f.pr_on("pay", "api", "merged", 7);
    assert_eq!(f.found(None), []);
}

#[test]
fn a_waiting_pr_with_no_pull_request_is_not_closed_unmerged() {
    let mut f = fixture();
    f.sql(&format!(
        "INSERT INTO prs (plan, key, repo, base, status, lead_branch, created_txn, created_at)
         VALUES ('pay', 'api', '/r/acme-api', 'main', 'waiting', 'cpd/pay/api', cpd_txn(), '{T}');"
    ));
    assert_eq!(f.found(None), []);
}

#[test]
fn the_plan_filter_returns_only_that_plans_prs() {
    let mut f = fixture();
    f.pull(7, "closed");
    f.pull(8, "closed");
    f.pr_on("shop", "web", "open", 8);
    f.pr_on("pay", "api", "open", 7);
    assert_eq!(f.found(None), [row("pay", "api", "l-pay", 7), row("shop", "web", "l-shop", 8)], "every Plan, by plan id then key");
    assert_eq!(f.found(Some("shop")), [row("shop", "web", "l-shop", 8)]);
    assert_eq!(f.found(Some("pay")), [row("pay", "api", "l-pay", 7)]);
}
