//! E14, the archive of an asleep Lead (contract §3.7, §2.2 "Archived", §8.2 `timeouts.archive_sleep_seconds`; hs-d630, hs-d633),
//! one `archive_leads::run` per tick on a prepared store. E14 archives a Lead (or Manager) Session whose `asleep_at` is older than
//! the configured threshold, taking its whole Plan through the `plan archive` cascade, and leaves a Lead asleep inside the
//! threshold alone. Every test has a control: a Lead that E14 does archive, so that "the recent one stays" is asserted after E14
//! showed it can archive.

use super::archive_leads;
use super::testkit::{Fx, ago_seconds};

/// Context `ctx`, `timeouts.archive_sleep_seconds` as given (the default when `None`).
fn fx(archive_sleep_seconds: Option<u32>) -> Fx {
    let threshold = archive_sleep_seconds
        .map(|n| format!("archive_sleep_seconds = {n}\n"))
        .unwrap_or_default();
    Fx::new(&format!("[timeouts]\n{threshold}"))
}

fn tick(fx: &mut Fx) {
    archive_leads::run(&mut fx.engine);
}

/// Plan `plan` of `ctx`, done two hours ago, its Lead `<plan>-lead` and one side fork `<plan>-fork`, both `stopped` and asleep
/// `asleep_seconds` ago.
fn plan(fx: &mut Fx, plan: &str, asleep_seconds: i64) {
    let asleep = ago_seconds(asleep_seconds);
    fx.fixture(&format!(
        "INSERT INTO sessions (id, role, context, plan, name, cwd, tier, state, state_at, created_at, asleep_at)
           VALUES ('{plan}-lead', 'lead', 'ctx', '{plan}', 'Lead {plan}', '/', 'O3', 'stopped', '{asleep}', 't', '{asleep}');
         INSERT INTO sessions (id, role, context, plan, forked_from, name, cwd, tier, state, state_at, created_at, asleep_at)
           VALUES ('{plan}-fork', 'lead', 'ctx', '{plan}', '{plan}-lead', 'Fork {plan}', '/', 'O3', 'stopped', '{asleep}', 't', '{asleep}');
         INSERT INTO plans (id, context, title, phase, lead_session, created_at, locked_at, done_at)
           VALUES ('{plan}', 'ctx', '{plan}', 'done', '{plan}-lead', 't', 't', '{done}');",
        done = ago_seconds(120 * 60)
    ));
}

fn plan_archived_at(fx: &mut Fx, plan: &str) -> Option<String> {
    fx.query(&format!(
        "SELECT archived_at FROM plans WHERE id = '{plan}'"
    ))
}

fn session_archived_at(fx: &mut Fx, session: &str) -> Option<String> {
    fx.query(&format!(
        "SELECT archived_at FROM sessions WHERE id = '{session}'"
    ))
}

fn session_asleep_at(fx: &mut Fx, session: &str) -> Option<String> {
    fx.query(&format!(
        "SELECT asleep_at FROM sessions WHERE id = '{session}'"
    ))
}

/// The values of column `column` of the Events of `kind`, in order.
fn event_column(fx: &mut Fx, column: &str, kind: &str) -> Vec<String> {
    fx.engine
        .store
        .read(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {column} FROM events WHERE kind = ?1 ORDER BY id"
            ))?;
            Ok(stmt
                .query_map([kind], |r| r.get(0))?
                .collect::<rusqlite::Result<Vec<String>>>()?)
        })
        .unwrap()
}

#[test]
fn a_lead_asleep_past_the_threshold_archives_its_done_plan() {
    let mut fx = fx(None);
    plan(&mut fx, "pay", 2 * 24 * 3600);
    plan(&mut fx, "recent", 60);
    for _ in 0..4 {
        tick(&mut fx);
    }

    // The Plan, its Lead and its side fork are archived; archiving wakes the Lead (`asleep_at` cleared).
    assert!(plan_archived_at(&mut fx, "pay").is_some());
    assert!(session_archived_at(&mut fx, "pay-lead").is_some());
    assert!(session_archived_at(&mut fx, "pay-fork").is_some());
    assert_eq!(
        session_asleep_at(&mut fx, "pay-lead"),
        None,
        "archiving wakes the Session (contract §2.2 'Asleep')"
    );
    assert_eq!(session_asleep_at(&mut fx, "pay-fork"), None);
    assert_eq!(event_column(&mut fx, "plan", "plan-archived"), ["pay"]);
    let mut sessions = event_column(&mut fx, "session", "session-archived");
    sessions.sort();
    assert_eq!(sessions, ["pay-fork", "pay-lead"]);

    // The Lead asleep inside the threshold stays asleep and unarchived.
    assert_eq!(plan_archived_at(&mut fx, "recent"), None);
    assert_eq!(session_archived_at(&mut fx, "recent-lead"), None);
    assert!(
        session_asleep_at(&mut fx, "recent-lead").is_some(),
        "a Lead asleep inside the threshold stays"
    );
}

/// E14 (contract §3.7, hs-d716): a Plan with `deleted_at` set (a tombstone, §2.2 "Deleted") is never an archive
/// candidate. E14 leaves it alone, appends no `error`, and never retries it; its orphan Lead stays asleep.
#[test]
fn a_deleted_plan_is_not_archived_and_appends_no_error() {
    let mut fx = fx(Some(1));
    plan(&mut fx, "gone", 2 * 24 * 3600);
    // Tombstone the Plan the way `plan delete` does: `deleted_at` is set, the row is never hard-deleted (§2.2 "Deleted").
    fx.fixture("UPDATE plans SET deleted_at = 't' WHERE id = 'gone';");
    // Match a real delete: the Plan's side forks are gone, its Lead is left behind as an orphan.
    fx.fixture("DELETE FROM sessions WHERE plan = 'gone' AND id <> 'gone-lead';");

    for _ in 0..4 {
        tick(&mut fx);
    }

    let errors = fx
        .engine
        .store
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT count(*) FROM events WHERE kind = 'error'",
                [],
                |r| r.get::<_, i64>(0),
            )?)
        })
        .unwrap();
    assert_eq!(errors, 0, "E14 must not error on a deleted Plan");
    let not_found = fx
        .engine
        .store
        .read(|conn| {
            Ok(conn.query_row(
                "SELECT count(*) FROM events WHERE kind = 'error' AND json_extract(data, '$.code') = 'plan_not_found'",
                [],
                |r| r.get::<_, i64>(0),
            )?)
        })
        .unwrap();
    assert_eq!(
        not_found, 0,
        "E14 must not append plan_not_found for a deleted Plan"
    );

    assert_eq!(
        plan_archived_at(&mut fx, "gone"),
        None,
        "a tombstoned Plan is never archived"
    );
    assert_eq!(session_archived_at(&mut fx, "gone-lead"), None);
    assert!(
        session_asleep_at(&mut fx, "gone-lead").is_some(),
        "the orphan Lead stays asleep; E14 archives a live Plan, not the orphan"
    );
}

#[test]
fn the_configured_threshold_decides() {
    // `archive_sleep_seconds = 1`: a Lead asleep 2 s is past the threshold and its Plan is archived.
    let mut short = fx(Some(1));
    plan(&mut short, "past", 2);
    tick(&mut short);
    assert!(plan_archived_at(&mut short, "past").is_some());
    assert!(session_archived_at(&mut short, "past-lead").is_some());
    assert_eq!(session_asleep_at(&mut short, "past-lead"), None);

    // The default (86400): the same 2 s of sleep is inside it, so the Plan stays.
    let mut default = fx(None);
    plan(&mut default, "past", 2);
    for _ in 0..4 {
        tick(&mut default);
    }
    assert_eq!(
        plan_archived_at(&mut default, "past"),
        None,
        "the default threshold does not archive a Lead asleep 2 s"
    );
    assert_eq!(session_archived_at(&mut default, "past-lead"), None);
    assert!(session_asleep_at(&mut default, "past-lead").is_some());
    assert!(event_column(&mut default, "plan", "plan-archived").is_empty());
}
