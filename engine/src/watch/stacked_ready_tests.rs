//! E8's `pr-ready` rule and stacked pull requests (contract §2.2, §3.7; pro-d118): a PR whose base is not the repo's
//! default branch is never ready; a NULL base or default branch is unknown, not ready. Rows are inserted with
//! `base_ref` and `default_branch`, the columns of the `pr-base` migration.

use super::inbox;
use super::testkit::Fx;

const PR_READY_OPEN: &str = "SELECT count(*) FROM inbox_items WHERE kind = 'pr-ready' AND label LIKE '% ready to merge' || char(10) || '%' AND closed_at IS NULL";
const REPO: &str = "missionlane/card-dashboard";

fn fx() -> Fx {
    let mut fx = Fx::with_plan("[inbox]\nstuck_job_minutes = 1\n");
    fx.fixture("UPDATE sessions SET state = 'working' WHERE id = 'lead1';");
    fx
}

fn tick(fx: &mut Fx) {
    inbox::run(&mut fx.engine);
}

/// An open, clean, non-draft pull request `number` with the given base and default branch (SQL literals).
fn polled(fx: &mut Fx, number: i64, review: &str, base: &str, default: &str) {
    fx.fixture(&format!(
        "INSERT INTO pull_requests (repo, number, url, title, draft, state, review_decision, checks, merge_state,
                                   base_ref, default_branch, created_at, changed_at)
         VALUES ('{REPO}', {number}, 'https://github.com/{REPO}/pull/{number}', 'T{number}', 0, 'open', '{review}',
                 'passed', 'clean', {base}, {default}, 't', 't');"
    ));
}

fn closed_events(fx: &mut Fx) -> usize {
    fx.events("inbox-closed").len()
}

/// A stacked clean PR opens no item, owned by a Plan or not.
#[test]
fn stacked_ready_inbox_opens_no_pr_ready_for_a_stacked_pr() {
    let mut fx = fx();
    polled(&mut fx, 3777, "approved", "'feature-a'", "'main'");
    polled(&mut fx, 3778, "none", "'feature-b'", "'main'");
    fx.fixture(
        "UPDATE prs SET status = 'open', pull_request = (SELECT id FROM pull_requests WHERE number = 3778),
                        pr_number = 3778, pr_url = 'https://github.com/missionlane/card-dashboard/pull/3778', pr_opened_at = 't'
          WHERE plan = 'pay';",
    );
    for _ in 0..2 {
        tick(&mut fx);
    }
    assert_eq!(fx.query::<i64>(PR_READY_OPEN), 0);
}

/// An open item closes at the next reconcile when the PR becomes stacked: one `inbox-closed` Event.
#[test]
fn stacked_ready_inbox_closes_an_open_pr_ready_when_the_pr_becomes_stacked() {
    let mut fx = fx();
    polled(&mut fx, 3777, "approved", "'main'", "'main'");
    tick(&mut fx);
    assert_eq!(fx.query::<i64>(PR_READY_OPEN), 1);
    fx.fixture("UPDATE pull_requests SET base_ref = 'feature-a' WHERE number = 3777;");
    tick(&mut fx);
    assert_eq!(fx.query::<i64>(PR_READY_OPEN), 0);
    assert_eq!(closed_events(&mut fx), 1);
}

/// Stacked, then retargeted to the default branch: the next reconcile opens the item.
#[test]
fn stacked_ready_inbox_opens_pr_ready_when_the_base_returns_to_the_default_branch() {
    let mut fx = fx();
    polled(&mut fx, 3777, "approved", "'feature-a'", "'main'");
    tick(&mut fx);
    assert_eq!(fx.query::<i64>(PR_READY_OPEN), 0);
    fx.fixture("UPDATE pull_requests SET base_ref = 'main' WHERE number = 3777;");
    tick(&mut fx);
    assert_eq!(fx.query::<i64>(PR_READY_OPEN), 1);
}

/// A NULL base_ref or a NULL default_branch is unknown: not ready.
#[test]
fn stacked_ready_inbox_opens_no_pr_ready_when_the_base_is_unknown() {
    let mut fx = fx();
    polled(&mut fx, 1, "approved", "NULL", "'main'");
    polled(&mut fx, 2, "approved", "'main'", "NULL");
    polled(&mut fx, 3, "none", "NULL", "NULL");
    for _ in 0..2 {
        tick(&mut fx);
    }
    assert_eq!(
        fx.query::<i64>(
            "SELECT count(*) FROM pull_requests WHERE base_ref IS NULL OR default_branch IS NULL"
        ),
        3
    );
    assert_eq!(fx.query::<i64>(PR_READY_OPEN), 0);
}

/// The positive control: clean, base = default branch.
#[test]
fn stacked_ready_inbox_still_opens_pr_ready_for_a_clean_pr_on_the_default_branch() {
    let mut fx = fx();
    polled(&mut fx, 3777, "approved", "'main'", "'main'");
    tick(&mut fx);
    assert_eq!(fx.query::<i64>(PR_READY_OPEN), 1);
}
