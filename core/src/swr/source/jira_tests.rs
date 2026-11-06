//! The `jira` source (docs/architecture/swr-jira.md): the outbox op and its forward coalescing, the skip rule on the cache,
//! the overlay, the cached cloud id, the issue row written from the helper's one read, and gc. A temp store; the helper is
//! a `FakeRunner`; time is passed in from a `ManualClock`; no process, no network, no sleep.

use std::time::Duration;

use rusqlite::params;
use serde_json::{Value, json};
use time::OffsetDateTime;

use super::jira::{self, Issue, Jira, Wanted};
use super::{Cx, ExecOutcome};
use crate::clock::{Clock, ManualClock, timestamp};
use crate::config::Config;
use crate::home::CpdHome;
use crate::host::runner::{Call, Reply};
use crate::policy::McpServer;
use crate::ports::Fakes;
use crate::store::Store;
use crate::swr::gc;
use crate::swr::outbox::{self, Mutation, Retry};
use crate::swr::outbox_tests::{at, row, store};

const TICKET: &str = "EEC-1";
const KEY: &str = "jira:issue:EEC-1";
const DAY: Duration = Duration::from_secs(86_400);

fn context() -> Value {
    json!({"plan": "pay-4521", "workflow": "ui"})
}

fn enqueue(store: &mut Store, now: OffsetDateTime, target: &str, name: Option<&str>) -> i64 {
    jira::enqueue_transition(store, now, TICKET, target, name, context()).unwrap()
}

fn status(store: &mut Store, id: i64) -> String {
    row(store, id).status
}

/// A cached issue row.
fn put_issue(store: &mut Store, ticket: &str, name: &str, category: &str) {
    store
        .write("fixture", |tx| {
            tx.execute(
                "INSERT INTO swr_jira_issues (ticket, cloud_id, summary, status_name, status_category, estimates_json, synced_at)
                 VALUES (?1, 'cloud-1', 'Retry failed payouts', ?2, ?3, '{}', '2026-01-01T00:00:00Z')",
                params![ticket, name, category],
            )?;
            Ok::<(), crate::error::CpdError>(())
        })
        .unwrap();
}

fn wanted(target_name: &str, later: &[&str]) -> Wanted {
    Wanted {
        target_name: target_name.into(),
        later_names: later.iter().map(|s| (*s).to_owned()).collect(),
        estimate_fields: Vec::new(),
    }
}

#[test]
fn jira_transition_enqueues_an_outbox_mutation_for_ticket() {
    let (_dir, mut store) = store();
    let now = ManualClock::new().utc();
    let id = enqueue(&mut store, now, "in_review", Some("In Review"));
    let m = row(&mut store, id);
    assert_eq!(
        (m.system.as_str(), m.target_key.as_str(), m.op_kind.as_str()),
        ("jira", KEY, "transition")
    );
    assert_eq!(m.status, "pending");
    assert_eq!(m.submitted_at, now);
    let vars: Value = serde_json::from_str(&m.vars_json).unwrap();
    assert_eq!(
        vars,
        json!({"ticket": TICKET, "target": "in_review", "rank": 2})
    );
    let patch: Value = serde_json::from_str(m.patch_json.as_deref().unwrap()).unwrap();
    assert_eq!(
        patch,
        json!({"status_name": "In Review", "status_category": "indeterminate"})
    );
    let ctx: Value = serde_json::from_str(m.context_json.as_deref().unwrap()).unwrap();
    assert_eq!(ctx["plan"], "pay-4521");
    assert_eq!(ctx["workflow"], "ui");
    assert_eq!(
        (
            jira::rank("in_progress"),
            jira::rank("in_review"),
            jira::rank("done")
        ),
        (Some(1), Some(2), Some(3))
    );
}

#[test]
fn jira_later_target_supersedes_unstarted_earlier_targets() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    let progress = enqueue(&mut store, at(&clock, 0), "in_progress", None);
    let review = enqueue(&mut store, at(&clock, 1), "in_review", None);
    assert_eq!(status(&mut store, progress), "superseded");
    assert_eq!(status(&mut store, review), "pending");
    let done = enqueue(&mut store, at(&clock, 1), "done", None);
    assert_eq!(status(&mut store, review), "superseded");
    assert_eq!(status(&mut store, done), "pending");
}

#[test]
fn jira_earlier_target_is_dropped_when_a_later_one_is_pending() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    let review = enqueue(&mut store, at(&clock, 0), "in_review", None);
    let progress = enqueue(&mut store, at(&clock, 1), "in_progress", None);
    assert_eq!(
        status(&mut store, progress),
        "superseded",
        "inserted superseded at once"
    );
    assert_eq!(status(&mut store, review), "pending");
    let again = enqueue(&mut store, at(&clock, 1), "in_review", None);
    assert_eq!(
        status(&mut store, again),
        "superseded",
        "a repeat of a pending rank collapses too"
    );
    assert_eq!(status(&mut store, review), "pending");
}

#[test]
fn jira_in_flight_transition_is_never_superseded() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    let progress = enqueue(&mut store, at(&clock, 0), "in_progress", None);
    let claimed = outbox::claim_next(&mut store, at(&clock, 1), "jira")
        .unwrap()
        .unwrap();
    assert_eq!(claimed.id, progress);
    let review = enqueue(&mut store, at(&clock, 1), "in_review", None);
    assert_eq!(status(&mut store, progress), "in_flight");
    assert_eq!(status(&mut store, review), "pending");
    let done = enqueue(&mut store, at(&clock, 1), "done", None);
    assert_eq!(status(&mut store, progress), "in_flight");
    assert_eq!(status(&mut store, review), "superseded");
    assert_eq!(status(&mut store, done), "pending");
}

#[test]
fn jira_skips_when_cached_status_is_at_or_past_target() {
    let (_dir, mut store) = store();
    assert!(
        !jira::skips_on_cache(&mut store, TICKET, &wanted("In Review", &["Done"])).unwrap(),
        "no cached row: the helper runs"
    );
    put_issue(&mut store, TICKET, "In Review", "indeterminate");
    let skips = |store: &mut Store, w: Wanted| jira::skips_on_cache(store, TICKET, &w).unwrap();
    assert!(
        skips(&mut store, wanted("In Review", &["Done"])),
        "at the target"
    );
    assert!(
        skips(&mut store, wanted("In Progress", &["In Review", "Done"])),
        "at a later target"
    );
    assert!(
        !skips(&mut store, wanted("Done", &[])),
        "behind: the cached status is before the target"
    );
}

#[test]
fn jira_skips_when_cached_status_category_is_done() {
    let (_dir, mut store) = store();
    put_issue(&mut store, TICKET, "Won't Do", "done");
    assert!(jira::skips_on_cache(&mut store, TICKET, &wanted("In Review", &["Done"])).unwrap());
}

#[test]
fn jira_overlay_shows_the_target_status_while_pending() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    put_issue(&mut store, TICKET, "In Progress", "indeterminate");
    assert_eq!(
        jira::effective(&mut store, TICKET).unwrap(),
        Some(Issue {
            status_name: "In Progress".into(),
            status_category: "indeterminate".into()
        })
    );
    enqueue(&mut store, at(&clock, 1), "done", Some("Done"));
    assert_eq!(
        jira::effective(&mut store, TICKET).unwrap(),
        Some(Issue {
            status_name: "Done".into(),
            status_category: "done".into()
        }),
        "the patch shows the target while the mutation is pending"
    );
}

#[test]
fn jira_failed_transition_drops_the_overlay() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    put_issue(&mut store, TICKET, "In Progress", "indeterminate");
    let id = enqueue(&mut store, at(&clock, 1), "in_review", Some("In Review"));
    outbox::claim_next(&mut store, at(&clock, 1), "jira")
        .unwrap()
        .unwrap();
    outbox::fail(
        &mut store,
        id,
        at(&clock, 1),
        "jira_refused: no_transition",
        Retry::Never,
    )
    .unwrap();
    assert_eq!(
        jira::effective(&mut store, TICKET).unwrap(),
        Some(Issue {
            status_name: "In Progress".into(),
            status_category: "indeterminate".into()
        }),
        "a failed mutation shows the cached row"
    );
}

/// A home, a config, fakes, and the server of the machine policy.
struct Rig {
    _dir: tempfile::TempDir,
    store: Store,
    home: CpdHome,
    config: Config,
    fakes: Fakes,
}

fn rig() -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().join("cpd").into()), None).unwrap();
    let store = Store::open(&home, 5000).unwrap();
    let config = Config::parse(
        "version = 1\nmachine = \"test\"\n",
        std::path::Path::new("config.toml"),
    )
    .unwrap()
    .0;
    Rig {
        _dir: dir,
        store,
        home,
        config,
        fakes: Fakes::new(),
    }
}

fn server() -> McpServer {
    McpServer {
        url: "https://mcp.atlassian.com/v1/mcp/authv2".into(),
        description: None,
        exposure: None,
        tool_exposure: None,
        oauth: None,
        headers: None,
    }
}

fn helper() -> Call {
    Call {
        tool: "jira".into(),
        program: "node".into(),
        args: vec!["jira-transition.ts".into()],
        envs: Vec::new(),
        cwd: None,
        deadline: Duration::from_secs(120),
        stdin: None,
        stderr_to: None,
    }
}

const MOVED: &str = r#"{"ok":true,"outcome":"moved","ticket":"EEC-1","target":"in_review","cloud_id":"cloud-new","from":"In Progress","to":"In Review","summary":"Retry failed payouts","estimates":{"customfield_10016":null}}"#;

/// Runs the claimed transition to `in_review` on a helper that answers `MOVED`; returns the rig, the outcome and the helper's calls.
fn run_moved(rig: &mut Rig) -> (ExecOutcome, Vec<Call>, Mutation) {
    (*rig.fakes.runner)
        .clone()
        .on("node", |_| true, Reply::ok(MOVED));
    let now = rig.fakes.clock.utc();
    enqueue(&mut rig.store, now, "in_review", Some("In Review"));
    let m = outbox::claim_next(&mut rig.store, now, "jira")
        .unwrap()
        .unwrap();
    let mut cx = Cx {
        store: &mut rig.store,
        ports: &rig.fakes.ports,
        home: &rig.home,
        config: &rig.config,
    };
    let outcome = Jira.run(
        &mut cx,
        &m,
        &wanted("In Review", &["Done"]),
        &server(),
        &helper(),
    );
    (outcome, rig.fakes.runner.calls(), m)
}

fn stdin_of(call: &Call) -> Value {
    serde_json::from_slice(call.stdin.as_deref().expect("the helper's stdin")).unwrap()
}

fn atlassian(rig: &mut Rig, cloud: &str, fetched: OffsetDateTime) {
    let at = timestamp(fetched);
    rig.store
        .write("fixture", |tx| {
            tx.execute(
                "INSERT INTO swr_atlassian (id, cloud_id, fetched_at) VALUES (1, ?1, ?2)",
                params![cloud, at],
            )?;
            Ok::<(), crate::error::CpdError>(())
        })
        .unwrap();
}

#[test]
fn jira_cloud_id_is_cached_and_passed_to_the_helper() {
    // Fresh (under 24 h): sent as `cloud_id`.
    let mut fresh = rig();
    let now = fresh.fakes.clock.utc();
    atlassian(&mut fresh, "cloud-cached", now - time::Duration::hours(23));
    let (_, calls, _) = run_moved(&mut fresh);
    assert_eq!(calls.len(), 1, "one helper spawn");
    assert_eq!(stdin_of(&calls[0])["cloud_id"], "cloud-cached");

    // Older than 24 h: sent as null; the answer's id is then cached.
    let mut old = rig();
    let now = old.fakes.clock.utc();
    atlassian(&mut old, "cloud-cached", now - time::Duration::hours(25));
    let (_, calls, _) = run_moved(&mut old);
    assert_eq!(stdin_of(&calls[0])["cloud_id"], Value::Null);
    let cached: String = old
        .store
        .read(|c| {
            Ok(
                c.query_row("SELECT cloud_id FROM swr_atlassian WHERE id = 1", [], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    assert_eq!(cached, "cloud-new");
}

#[test]
fn jira_issue_row_is_written_from_the_helper_read() {
    let mut rig = rig();
    let (_, calls, _) = run_moved(&mut rig);
    let request = stdin_of(&calls[0]);
    assert_eq!(request["ticket"], TICKET);
    assert_eq!(request["target_name"], "In Review");
    assert_eq!(request["later_names"], json!(["Done"]));
    let (summary, name, category, cloud): (String, String, String, String) = rig
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT summary, status_name, status_category, cloud_id FROM swr_jira_issues WHERE ticket = 'EEC-1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )?)
        })
        .unwrap();
    assert_eq!(summary, "Retry failed payouts");
    assert_eq!(name, "In Review", "the status after the move");
    assert_eq!(category, "indeterminate");
    assert_eq!(cloud, "cloud-new");
}

#[test]
fn jira_issue_rows_unused_for_seven_days_are_garbage_collected() {
    let (_dir, mut store) = store();
    let clock = ManualClock::new();
    let t0 = clock.utc();
    put_issue(&mut store, "EEC-1", "Done", "done");
    put_issue(&mut store, "EEC-2", "In Progress", "indeterminate");
    // EEC-2 has a pending transition: it is observed.
    jira::enqueue_transition(&mut store, t0, "EEC-2", "in_review", None, context()).unwrap();
    jira::mark_inactive(&mut store, t0).unwrap();

    let ttl = Duration::from_secs(7 * 86_400);
    gc::sweep(&mut store, t0 + 6 * DAY, ttl).unwrap();
    let tickets = |store: &mut Store| -> Vec<String> {
        store
            .read(|c| {
                let mut stmt = c.prepare("SELECT ticket FROM swr_jira_issues ORDER BY ticket")?;
                Ok(stmt
                    .query_map([], |r| r.get(0))?
                    .collect::<Result<Vec<_>, _>>()?)
            })
            .unwrap()
    };
    assert_eq!(
        tickets(&mut store),
        ["EEC-1", "EEC-2"],
        "younger than seven days"
    );
    gc::sweep(&mut store, t0 + 8 * DAY, ttl).unwrap();
    assert_eq!(
        tickets(&mut store),
        ["EEC-2"],
        "the unused row goes; the one with a pending transition stays"
    );
}
