//! The finished rule of a Lead (contract §3.7 E13, E14; pro-d358): a Lead is finished only when its Plan is `done`, every
//! Workflow is `merged` or `cancelled`, no Job is `draft`, `queued`, `running` or `failed`, and its Session has no open,
//! undismissed Inbox item, a snoozed one included. On the fake ports: no process starts.

use super::testkit::{Fx, ago};
use super::{archive_leads, sleep};

const FUTURE: &str = "2999-01-01T00:00:00.000Z";

/// Plan `pay` is `done` long ago, its Workflow merged, Lead `lead1` idle since long ago, with nothing open.
fn finished() -> Fx {
    let mut fx = Fx::new("");
    let long_ago = ago(24 * 60);
    fx.fixture(&format!(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, runner)
           VALUES ('lead1', 'lead', 'ctx', 'pay', 'Lead', '/', 'O3', 'idle', '{long_ago}', 't', 'durable');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('pay', 'ctx', 'Pay', 'done', 'lead1', 't', 't', '{long_ago}');
         INSERT INTO workflows (plan, key, repo, base, status, lead_branch, lead_worktree, title, shape, created_txn, created_at)
           VALUES ('pay', 'repo', '/r', 'main', 'cancelled', 'cpd/pay', NULL, 'T', 'config', cpd_txn(), 't');"
    ));
    fx
}

fn failed_job(fx: &mut Fx) {
    fx.add_job(
        "j1",
        &[],
        "UPDATE jobs SET status = 'running' WHERE key = 'j1'; UPDATE jobs SET status = 'failed', failed_cause = 'crashed' WHERE key = 'j1';",
    );
}

fn snoozed_item(fx: &mut Fx) {
    fx.fixture(&format!(
        "INSERT INTO inbox_items (kind, urgency, subject, context, plan, session, label, opened_at, snoozed_until)
           VALUES ('question', 'yellow', 'Q', 'ctx', 'pay', 'lead1', 'Q', 't', '{FUTURE}');"
    ));
}

fn asleep(fx: &mut Fx) -> bool {
    fx.query::<i64>("SELECT count(*) FROM sessions WHERE id = 'lead1' AND asleep_at IS NOT NULL")
        == 1
}

fn archived(fx: &mut Fx) -> bool {
    fx.query::<i64>("SELECT count(*) FROM plans WHERE id = 'pay' AND archived_at IS NOT NULL") == 1
}

/// The Lead asleep long ago, so that E14 would archive it were it finished.
fn put_to_sleep(fx: &mut Fx) {
    let long_ago = ago(24 * 60 * 30);
    fx.fixture(&format!(
        "UPDATE sessions SET asleep_at = '{long_ago}' WHERE id = 'lead1';"
    ));
}

#[test]
fn a_finished_lead_with_nothing_open_is_still_put_to_sleep() {
    let mut fx = finished();
    sleep::run(&mut fx.engine);
    assert!(asleep(&mut fx));
}

#[test]
fn a_lead_with_a_failed_job_is_not_put_to_sleep() {
    let mut fx = finished();
    failed_job(&mut fx);
    sleep::run(&mut fx.engine);
    assert!(!asleep(&mut fx), "a failed Job keeps the Lead awake");
}

#[test]
fn a_lead_with_a_snoozed_open_inbox_item_is_not_put_to_sleep() {
    let mut fx = finished();
    snoozed_item(&mut fx);
    sleep::run(&mut fx.engine);
    assert!(!asleep(&mut fx), "a snoozed open item keeps the Lead awake");
}

#[test]
fn a_sleeping_lead_with_a_failed_job_is_not_archived_by_e14() {
    let mut fx = finished();
    failed_job(&mut fx);
    put_to_sleep(&mut fx);
    archive_leads::run(&mut fx.engine);
    assert!(!archived(&mut fx), "a failed Job keeps the Plan unarchived");
}

#[test]
fn a_sleeping_lead_with_a_snoozed_open_inbox_item_is_not_archived_by_e14() {
    let mut fx = finished();
    snoozed_item(&mut fx);
    put_to_sleep(&mut fx);
    archive_leads::run(&mut fx.engine);
    assert!(
        !archived(&mut fx),
        "a snoozed open item keeps the Plan unarchived"
    );
}
