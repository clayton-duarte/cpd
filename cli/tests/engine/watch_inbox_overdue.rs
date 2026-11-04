//! Engine step E8, the `review overdue` label of an authored pull request's Inbox row (contract §2.2 "The authored pull
//! request rule", OVERDUE, §3.7 E8, §4.4 "The overdue post and the RePost"; pro-d386, pro-d388): E8 relabels the open
//! `pr-ready` row in place when the post is 86 400 s old and the pull request is unreviewed, and sends NOTHING to Slack.
//! The engine runs in-process on the `ManualClock` ([`Fx::watch_ticks`]); every external is a fake.

use crate::common;

use common::fx::Fx;
use serde_json::Value;

const HOUR: i64 = 3600;
const DAY: i64 = 24 * HOUR;
const IN_REVIEW: &str = "pay#1 waiting for peer review\nPR p1";
const OVERDUE: &str = "pay#1 review overdue\nPR p1";

struct Overdue {
    fx: Fx,
}

/// Plan `pay` of Context `ctx` with its stopped Lead; the open authored pull request `acme/pay#1` (checks passed, not
/// clean, so its status is `waiting for peer review` once posted) owned by Workflow `p1`, posted `posted_ago` seconds ago.
fn overdue(posted_ago: i64) -> Overdue {
    let fx = Fx::with_ports();
    fx.engine_config("");
    fx.engine_routes();
    let now = fx.stamp_ago(0);
    let posted = fx.stamp_ago(posted_ago * 1000);
    fx.env.sql(&format!(
        "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');
         INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at)
           VALUES ('pay-lead', 'lead', 'ctx', 'pay', 'Lead pay', '/', 'O3', 'stopped', '{now}', 't');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at)
           VALUES ('pay', 'ctx', 'Pay', 'deploy', 'pay-lead', 't', 't');
         INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, created_at, changed_at, feedback)
           VALUES ('acme/pay', 1, 'https://github.com/acme/pay/pull/1', 'PR p1', 0, 'open', 'none', 'passed', 't', 't', '[]');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, pr_number, pr_url, pr_opened_at,
                                pull_request, created_txn, created_at, started_at)
           VALUES ('pay', 'p1', '/nowhere', 'main', 'open', 'cpd/pay/p1', '/nowhere/lead', 1, 'https://github.com/acme/pay/pull/1', 't',
                   (SELECT id FROM pull_requests WHERE number = 1), cpd_txn(), 't', 't');
         INSERT INTO pr_posts (plan, workflow, channel, ts, link, posted_at, last_posted_at)
           VALUES ('pay', 'p1', 'C1', '1793040197.000001', 'https://acme.slack.com/archives/C1/p1793040197000001', '{posted}', '{posted}');"
    ));
    Overdue { fx }
}

impl Overdue {
    /// The engine starts, ticks once and stops.
    fn tick(&self) {
        self.fx.watch_ticks(1).unwrap();
    }

    /// The one open `pr-ready` item of `cpd inbox list`: its id and label.
    fn item(&self) -> (i64, String) {
        let json = self.fx.cpd(&["inbox", "list"]).ok();
        let items: Vec<&Value> = json["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["kind"] == "pr-ready")
            .collect();
        assert_eq!(items.len(), 1, "{items:?}");
        (
            items[0]["id"].as_i64().unwrap(),
            items[0]["label"].as_str().unwrap().to_owned(),
        )
    }

    fn last_posted_at(&self) -> String {
        let conn = rusqlite::Connection::open_with_flags(
            self.fx.env.cpd_home().join("cpd.db"),
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        conn.query_row("SELECT last_posted_at FROM pr_posts", [], |r| r.get(0))
            .unwrap()
    }
}

#[test]
fn e8_relabels_the_pr_item_review_overdue_for_a_due_post_and_keeps_it_the_same_row() {
    let o = overdue(HOUR);
    o.tick();
    let (id, label) = o.item();
    assert_eq!(label, IN_REVIEW);

    // The post is now 25 h old and the pull request still unreviewed.
    let old = o.fx.stamp_ago((DAY + HOUR) * 1000);
    o.fx.env.sql(&format!(
        "UPDATE pr_posts SET posted_at = '{old}', last_posted_at = '{old}'"
    ));
    o.tick();
    assert_eq!(o.item(), (id, OVERDUE.to_owned()), "relabelled in place");
}

#[test]
fn e8_reverts_the_label_when_last_posted_at_moves_forward() {
    let o = overdue(DAY + HOUR);
    o.tick();
    let (id, label) = o.item();
    assert_eq!(label, OVERDUE);

    // A RePost sets last_posted_at to now; posted_at stays the first post.
    let now = o.fx.stamp_ago(0);
    o.fx.env
        .sql(&format!("UPDATE pr_posts SET last_posted_at = '{now}'"));
    o.tick();
    assert_eq!(o.item(), (id, IN_REVIEW.to_owned()), "reverted in place");
}

#[test]
fn e8_sends_nothing_and_leaves_last_posted_at_alone_for_a_due_post() {
    let o = overdue(DAY + HOUR);
    let before = o.last_posted_at();
    o.tick();
    o.tick();
    // The row is overdue (E8 computed it) ...
    assert_eq!(o.item().1, OVERDUE);
    // ... and nothing was sent: no helper ran and the post is untouched.
    assert!(
        o.fx.calls("node").is_empty(),
        "a Slack helper ran: {:?}",
        o.fx.calls("node")
    );
    assert_eq!(o.last_posted_at(), before);
}
