//! `cpd inbox list | show | snooze | dismiss` (commands.md §21), run in this
//! process through `run_with` (`Fx`: a `ManualClock` for the time, a `FakeRunner`
//! for `date`; the engine's `locks/watch.lock` is a real flock on a temp file).
//! Fixture items come from `model::reconcile`, as the engine's E8 writes them.

use crate::common;

use std::fs::{self, File};

use common::Run;
use common::fx::Fx;
use cpd_core::clock::Clock;
use cpd_core::home::CpdHome;
use cpd_core::host::runner::Reply;
use cpd_core::ids::ContextId;
use cpd_core::inbox::model::{Condition, Kind, Urgency, reconcile};
use cpd_core::store::Store;
use nix::fcntl::{Flock, FlockArg};
use serde_json::{Value, json};
use time::format_description::well_known::Rfc3339;
use time::{Duration, OffsetDateTime};

pub(crate) const T0: &str = "2026-01-01T00:00:00.000Z";
pub(crate) const T1: &str = "2026-01-01T01:00:00.000Z";
pub(crate) const T2: &str = "2026-01-01T02:00:00.000Z";
pub(crate) const T3: &str = "2026-01-01T03:00:00.000Z";
pub(crate) const T4: &str = "2026-01-01T04:00:00.000Z";
const CLOSED_X: &str = "2026-01-02T00:00:00.000Z";
pub(crate) const CLOSED_Y: &str = "2026-01-03T00:00:00.000Z";

/// A temporary `$CPD_HOME` with an item of each rank in Contexts `ctx1` and `ctx2`.
pub(crate) struct Fixture {
    pub(crate) fx: Fx,
    pub(crate) store: Store,
    /// Item ids by subject.
    ids: Vec<(String, i64)>,
}

pub(crate) fn condition(kind: Kind, urgency: Urgency, subject: &str, context: &str) -> Condition {
    Condition {
        kind,
        urgency,
        subject: subject.to_owned(),
        label: format!("{subject} label"),
        url: None,
        context: Some(ContextId::parse(context).unwrap()),
        plan: None,
        job: None,
        session: None,
        record: None,
        event: None,
        opened_at: None,
    }
}

impl Fixture {
    /// Open, in order: `stuck` (red), `review` (yellow, ctx2, T2), `question`
    /// (yellow, T4), `unread` (magenta), `dialog` (white). Closed: `x` at
    /// 2026-01-02, then `y` at 2026-01-03.
    pub(crate) fn new() -> Fixture {
        let fx = Fx::with_ports();
        let env = &fx.env;
        fs::write(
            env.cpd_home().join("config.toml"),
            "version = 1\nmachine = \"m1\"\n",
        )
        .unwrap();
        let home = CpdHome::resolve(Some(env.cpd_home().into()), None).unwrap();
        let mut store = Store::open(&home, 5000).unwrap();
        store
            .write("fixture", |tx| {
                tx.execute_batch(
                    "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx1', 'One', 't', 'm1');
                     INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx2', 'Two', 't', 'm1');",
                )?;
                Ok(())
            })
            .unwrap();
        let mut all = vec![
            condition(Kind::JobStuck, Urgency::Red, "stuck", "ctx1"),
            condition(Kind::Dialog, Urgency::White, "x", "ctx1"),
            condition(Kind::Dialog, Urgency::White, "y", "ctx1"),
        ];
        reconcile(&mut store, &all, T0).unwrap();
        for (at, c) in [
            (
                T1,
                condition(Kind::Dialog, Urgency::White, "dialog", "ctx1"),
            ),
            (
                T2,
                condition(Kind::PlanReview, Urgency::Yellow, "review", "ctx2"),
            ),
            (
                T3,
                condition(Kind::Unread, Urgency::Magenta, "unread", "ctx1"),
            ),
            (
                T4,
                condition(Kind::Question, Urgency::Yellow, "question", "ctx1"),
            ),
        ] {
            all.push(c);
            reconcile(&mut store, &all, at).unwrap();
        }
        all.retain(|c| c.subject != "x");
        reconcile(&mut store, &all, CLOSED_X).unwrap();
        all.retain(|c| c.subject != "y");
        reconcile(&mut store, &all, CLOSED_Y).unwrap();

        let ids = store
            .read(|conn| {
                let mut stmt = conn.prepare("SELECT subject, id FROM inbox_items")?;
                let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
                Ok(rows.collect::<Result<Vec<(String, i64)>, _>>()?)
            })
            .unwrap();
        Fixture { fx, store, ids }
    }

    pub(crate) fn id(&self, subject: &str) -> i64 {
        self.ids.iter().find(|(s, _)| s == subject).unwrap().1
    }

    pub(crate) fn cpd(&self, args: &[&str]) -> Run {
        self.fx.cpd(args)
    }

    /// `cpd inbox <args>`, which must succeed.
    pub(crate) fn ok(&self, args: &[&str]) -> Value {
        let mut argv = vec!["inbox"];
        argv.extend(args);
        self.cpd(&argv).ok()
    }

    pub(crate) fn fail(&self, args: &[&str]) -> (i32, Value) {
        let mut argv = vec!["inbox"];
        argv.extend(args);
        let run = self.cpd(&argv);
        (run.exit, run.error())
    }

    pub(crate) fn events(&self, kind: &str) -> Vec<Value> {
        self.cpd(&["events", "list", "--kind", kind]).ok()["events"]
            .as_array()
            .unwrap()
            .clone()
    }

    /// Holds `locks/watch.lock` as an engine does, until dropped.
    pub(crate) fn engine(&self) -> Flock<File> {
        let path = self.fx.env.cpd_home().join("locks/watch.lock");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        Flock::lock(File::create(path).unwrap(), FlockArg::LockExclusiveNonblock)
            .map_err(|(_, e)| e)
            .unwrap()
    }
}

fn subjects(json: &Value) -> Vec<&str> {
    json["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["subject"].as_str().unwrap())
        .collect()
}

fn time(value: &Value) -> OffsetDateTime {
    OffsetDateTime::parse(value.as_str().unwrap(), &Rfc3339).unwrap()
}

#[test]
fn list_is_in_order_with_engine_down_first_when_no_engine_runs() {
    let fx = Fixture::new();
    // Two days on: the items opened at T0 are older than a day.
    fx.fx.advance(std::time::Duration::from_secs(2 * 86_400));
    let list = fx.ok(&["list"]);
    assert_eq!(list["engine"], json!({"running": false}));
    assert_eq!(
        subjects(&list),
        ["engine", "stuck", "review", "question", "unread", "dialog"]
    );
    let down = &list["items"][0];
    assert_eq!(down["id"], Value::Null);
    assert_eq!(down["kind"], "engine-down");
    assert_eq!(down["urgency"], "red");
    assert_eq!(down["label"], "engine is down");
    assert_eq!(down["context"], Value::Null);
    assert_eq!(
        list["hidden"],
        json!({"snoozed": 0, "dismissed": 0, "claimed": 0})
    );
    // The highest Event id: the fixture's reconcile calls appended Events.
    assert!(list["events_cursor"].as_i64().unwrap() > 0);

    let stuck = &list["items"][1];
    assert_eq!(stuck["id"], fx.id("stuck"));
    assert_eq!(stuck["kind"], "job-stuck");
    assert_eq!(stuck["urgency"], "red");
    assert_eq!(stuck["label"], "stuck label");
    assert_eq!(stuck["context"], "ctx1");
    assert_eq!(stuck["opened_at"], T0);
    assert_eq!(stuck["closed_at"], Value::Null);
    // Opened in 2026 and still open: older than a day.
    assert!(stuck["age_seconds"].as_i64().unwrap() > 86_400);
}

#[test]
fn a_running_engine_adds_no_engine_down() {
    let fx = Fixture::new();
    let _engine = fx.engine();
    let list = fx.ok(&["list"]);
    assert_eq!(list["engine"], json!({"running": true}));
    assert_eq!(
        subjects(&list),
        ["stuck", "review", "question", "unread", "dialog"]
    );
}

/// A Manager Session of `ctx1` (`archived`: archived, so not live).
fn manager(fx: &mut Fixture, id: &str, archived: bool) {
    let archived = if archived {
        "'2026-01-02T00:00:00.000Z'"
    } else {
        "NULL"
    };
    fx.store
        .write("fixture", |tx| {
            tx.execute_batch(&format!(
                "INSERT INTO sessions (id, role, context, plan, attempt, name, file, cwd, tier, tmux_pane, state, state_label, state_at, created_at, archived_at)
                 VALUES ('{id}', 'manager', 'ctx1', NULL, NULL, 'ctx1-manager-{id}', '/sessions/{id}.jsonl', '/repo', 'O4', NULL, 'idle', NULL,
                         '2026-01-02T00:00:00.000Z', '2026-01-02T00:00:00.000Z', {archived});"
            ))?;
            Ok(())
        })
        .unwrap();
}

#[test]
fn engine_down_shows_only_when_no_live_manager_exists() {
    let mut fx = Fixture::new();
    assert_eq!(
        subjects(&fx.ok(&["list"]))[0],
        "engine",
        "no Manager: the User is the one to act"
    );
    // an archived Manager is not live
    manager(&mut fx, "m-old", true);
    assert_eq!(subjects(&fx.ok(&["list"]))[0], "engine");
    // a live one handles the engine (the tree restarts it, a failure goes to the Manager): no item, and `engine` still says it is down
    manager(&mut fx, "m-live", false);
    let list = fx.ok(&["list"]);
    assert_eq!(list["engine"], json!({"running": false}));
    assert_eq!(
        subjects(&list),
        ["stuck", "review", "question", "unread", "dialog"]
    );
    assert_eq!(subjects(&fx.ok(&["list", "--all"]))[0], "stuck");
}

#[test]
fn hidden_items_leave_the_list_and_are_counted() {
    let fx = Fixture::new();
    let _engine = fx.engine();
    fx.ok(&["snooze", &fx.id("dialog").to_string(), "1h"]);
    fx.ok(&["dismiss", &fx.id("unread").to_string()]);

    let list = fx.ok(&["list"]);
    assert_eq!(subjects(&list), ["stuck", "review", "question"]);
    assert_eq!(
        list["hidden"],
        json!({"snoozed": 1, "dismissed": 1, "claimed": 0})
    );

    let all = fx.ok(&["list", "--all"]);
    assert_eq!(
        subjects(&all),
        ["stuck", "review", "question", "unread", "dialog"]
    );
    assert_eq!(
        all["hidden"],
        json!({"snoozed": 1, "dismissed": 1, "claimed": 0})
    );
    let hidden: Vec<&Value> = all["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| &i["hidden"])
        .collect();
    assert_eq!(
        hidden,
        [
            &Value::Null,
            &Value::Null,
            &Value::Null,
            &json!("dismissed"),
            &json!("snoozed")
        ]
    );
}

#[test]
fn a_snooze_that_has_passed_shows_the_item_again() {
    let mut fx = Fixture::new();
    let _engine = fx.engine();
    let dialog = fx.id("dialog");
    fx.ok(&["snooze", &dialog.to_string(), "15m"]);
    assert_eq!(fx.ok(&["list"])["hidden"]["snoozed"], 1);
    fx.store
        .write("fixture", |tx| {
            tx.execute(
                "UPDATE inbox_items SET snoozed_until = '2000-01-01T00:00:00.000Z' WHERE id = ?1",
                [dialog],
            )?;
            Ok(())
        })
        .unwrap();
    let list = fx.ok(&["list"]);
    assert_eq!(
        subjects(&list),
        ["stuck", "review", "question", "unread", "dialog"]
    );
    assert_eq!(
        list["hidden"],
        json!({"snoozed": 0, "dismissed": 0, "claimed": 0})
    );
}

#[test]
fn context_keeps_only_that_contexts_items_and_drops_engine_down() {
    let fx = Fixture::new();
    let list = fx.ok(&["list", "--context", "ctx2"]);
    assert_eq!(subjects(&list), ["review"]);
    let list = fx.ok(&["list", "--context", "ctx1"]);
    assert_eq!(subjects(&list), ["stuck", "question", "unread", "dialog"]);
    let (exit, error) = fx.fail(&["list", "--context", "nope"]);
    assert_eq!(
        (exit, error["code"].as_str()),
        (4, Some("context_not_found"))
    );
}

#[test]
fn closed_lists_newest_first_with_a_limit_and_a_fixed_age() {
    let fx = Fixture::new();
    let list = fx.ok(&["list", "--closed"]);
    assert_eq!(subjects(&list), ["y", "x"]);
    let y = &list["items"][0];
    assert_eq!(y["closed_at"], CLOSED_Y);
    // closed_at minus opened_at: T0 to 2026-01-03.
    assert_eq!(y["age_seconds"], 2 * 86_400);
    assert_eq!(list["items"][1]["age_seconds"], 86_400);
    assert_eq!(
        subjects(&fx.ok(&["list", "--closed", "--limit", "1"])),
        ["y"]
    );
    // Open items are not listed, so neither is engine-down, but they are counted as hidden.
    assert_eq!(
        list["hidden"],
        json!({"snoozed": 0, "dismissed": 0, "claimed": 0})
    );
}

#[test]
fn list_flags_that_do_not_go_together_are_usage() {
    let fx = Fixture::new();
    for args in [
        &["list", "--all", "--closed"][..],
        &["list", "--limit", "5"],
        &["list", "--closed", "--limit", "0"],
    ] {
        let (exit, error) = fx.fail(args);
        assert_eq!(
            (exit, error["code"].as_str()),
            (2, Some("usage")),
            "{args:?}"
        );
    }
}

#[test]
fn show_prints_an_open_or_a_closed_item() {
    let fx = Fixture::new();
    let shown = fx.ok(&["show", &fx.id("review").to_string()]);
    let item = &shown["item"];
    assert_eq!(item["id"], fx.id("review"));
    assert_eq!(item["kind"], "plan-review");
    assert_eq!(item["urgency"], "yellow");
    assert_eq!(item["subject"], "review");
    assert_eq!(item["context"], "ctx2");
    for link in [
        "plan",
        "job",
        "session",
        "record",
        "event",
        "url",
        "closed_at",
        "snoozed_until",
        "dismissed_at",
    ] {
        assert_eq!(item[link], Value::Null, "{link}");
    }
    let closed = fx.ok(&["show", &fx.id("x").to_string()]);
    assert_eq!(closed["item"]["closed_at"], CLOSED_X);

    let (exit, error) = fx.fail(&["show", "999"]);
    assert_eq!((exit, error["code"].as_str()), (4, Some("item_not_found")));
}

/// `snooze <id> <duration>`'s `snoozed_until`, and the time just before and after.
fn snoozed(
    fx: &Fixture,
    id: i64,
    duration: &str,
) -> (OffsetDateTime, OffsetDateTime, OffsetDateTime) {
    let before = fx.fx.fakes.clock.utc();
    let out = fx.ok(&["snooze", &id.to_string(), duration]);
    let after = fx.fx.fakes.clock.utc();
    assert_eq!(out["item"]["id"], id);
    let until = time(&out["item"]["snoozed_until"]);
    // The stored value is the printed one.
    assert_eq!(
        fx.ok(&["show", &id.to_string()])["item"]["snoozed_until"],
        out["item"]["snoozed_until"]
    );
    (before, until, after)
}

#[test]
fn snooze_15m_and_1h_are_that_long_from_now() {
    let fx = Fixture::new();
    let id = fx.id("dialog");
    for (duration, length) in [("15m", Duration::minutes(15)), ("1h", Duration::hours(1))] {
        let (before, until, after) = snoozed(&fx, id, duration);
        // Times are printed to the millisecond.
        let slack = Duration::milliseconds(1);
        assert!(
            until >= before + length - slack && until <= after + length,
            "{duration}: {until}"
        );
    }
    // Snoozing again replaced the time and appended another Event, scoped as the item is.
    let events = fx.events("inbox-snoozed");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0]["data"]["item"], id);
    assert_eq!(events[0]["context"], "ctx1");
    assert_eq!(
        events[1]["data"]["until"],
        fx.ok(&["show", &id.to_string()])["item"]["snoozed_until"]
    );
    // opened_at is kept.
    assert_eq!(fx.ok(&["show", &id.to_string()])["item"]["opened_at"], T1);
}

#[test]
fn snooze_tomorrow_is_the_next_0900_at_the_offset_date_prints() {
    let fx = Fixture::new();
    let id = fx.id("dialog");
    // 09:00 at +0530 is 03:30 UTC; at -0800 it is 17:00 UTC.
    for (offset, hour, minute) in [("+0530", 3, 30), ("-0800", 17, 0), ("+0000", 9, 0)] {
        let _ =
            (*fx.fx.fakes.runner)
                .clone()
                .once("date", |_| true, Reply::ok(&format!("{offset}\n")));
        let (before, until, _) = snoozed(&fx, id, "tomorrow");
        assert_eq!(
            (
                until.hour(),
                until.minute(),
                until.second(),
                until.millisecond()
            ),
            (hour, minute, 0, 0),
            "{offset}"
        );
        assert!(
            until > before && until <= before + Duration::hours(24),
            "{offset}: {until}"
        );
    }
    assert_eq!(fx.fx.calls("date")[0].args, ["+%z"]);
    assert_eq!(fx.fx.calls("date").len(), 3);
}

#[test]
fn snooze_tomorrow_refuses_when_date_fails_and_changes_nothing() {
    let fx = Fixture::new();
    let id = fx.id("dialog");
    let _ = (*fx.fx.fakes.runner)
        .clone()
        .once("date", |_| true, Reply::exit(1, "", "no zone\n"));
    let (exit, error) = fx.fail(&["snooze", &id.to_string(), "tomorrow"]);
    assert_eq!((exit, error["code"].as_str()), (1, Some("spawn_failed")));
    assert_eq!(error["details"]["stderr"], "no zone");
    let _ = (*fx.fx.fakes.runner)
        .clone()
        .once("date", |_| true, Reply::ok("garbage\n"));
    let (exit, error) = fx.fail(&["snooze", &id.to_string(), "tomorrow"]);
    assert_eq!((exit, error["code"].as_str()), (1, Some("spawn_failed")));
    assert_eq!(
        fx.ok(&["show", &id.to_string()])["item"]["snoozed_until"],
        Value::Null
    );
    assert!(fx.events("inbox-snoozed").is_empty());
}

#[test]
fn snooze_refuses_what_it_cannot_hide() {
    let fx = Fixture::new();
    let (exit, error) = fx.fail(&["snooze", "999", "1h"]);
    assert_eq!((exit, error["code"].as_str()), (4, Some("item_not_found")));
    let (exit, error) = fx.fail(&["snooze", &fx.id("x").to_string(), "1h"]);
    assert_eq!((exit, error["code"].as_str()), (3, Some("item_closed")));
    let (exit, error) = fx.fail(&["snooze", &fx.id("dialog").to_string(), "2h"]);
    assert_eq!((exit, error["code"].as_str()), (2, Some("usage")));
    assert!(fx.events("inbox-snoozed").is_empty());
}

#[test]
fn dismiss_is_idempotent_and_the_item_stays_hidden() {
    let fx = Fixture::new();
    let id = fx.id("unread").to_string();
    let first = fx.ok(&["dismiss", &id]);
    assert_eq!(first["item"]["id"], fx.id("unread"));
    assert!(first["item"]["dismissed_at"].is_string());
    assert_eq!(first.get("already"), None);
    let cursor = fx.ok(&["list"])["events_cursor"].clone();
    let events = fx.events("inbox-dismissed");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["data"], json!({"item": fx.id("unread")}));
    assert_eq!(events[0]["context"], "ctx1");

    let again = fx.ok(&["dismiss", &id]);
    assert_eq!(again["already"], true);
    assert_eq!(again["item"]["dismissed_at"], first["item"]["dismissed_at"]);
    // Nothing was written: no Event, and no new transaction.
    assert_eq!(fx.events("inbox-dismissed").len(), 1);
    assert_eq!(fx.ok(&["list"])["events_cursor"], cursor);
    let txns = |fx: &Fixture| -> i64 { fx.store_count("SELECT count(*) FROM txns") };
    let count = txns(&fx);
    fx.ok(&["dismiss", &id]);
    assert_eq!(txns(&fx), count);
}

#[test]
fn dismiss_refuses_what_it_cannot_hide() {
    let fx = Fixture::new();
    let (exit, error) = fx.fail(&["dismiss", "999"]);
    assert_eq!((exit, error["code"].as_str()), (4, Some("item_not_found")));
    let (exit, error) = fx.fail(&["dismiss", &fx.id("x").to_string()]);
    assert_eq!((exit, error["code"].as_str()), (3, Some("item_closed")));
    assert!(fx.events("inbox-dismissed").is_empty());
}

impl Fixture {
    /// One integer from a query on a fresh connection.
    pub(crate) fn store_count(&self, sql: &str) -> i64 {
        let home = CpdHome::resolve(Some(self.fx.env.cpd_home().into()), None).unwrap();
        Store::open(&home, 5000)
            .unwrap()
            .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }
}
