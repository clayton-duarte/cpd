//! Engine step E8, the `drafts-parked` Inbox item (contract §2.2 "Inbox item" `drafts-parked`, §3.7 "Parked drafts"; hs-d822,
//! which supersedes hs-d731; hs-d733; Plan `draft-never-parked`): a Plan whose `parked_drafts` is not empty, with a wake W newer than T (the last Event
//! that changes a draft's legality) and a turn its Lead finished after W (a `session-state` Event `working` → `idle`), is one
//! open magenta item linked to the Plan and its Lead. W is written here by hand (what E20 sends is `watch_parked.rs`'s), as an
//! Event with `data.from` null; the engine runs in-process on virtual time with the fake tmux server.

use crate::common;

use common::fx::Fx as Ip;
use serde_json::Value;

/// Plan `pay` (`dispatch`) and its Lead `lead1`, `idle` in a live pane, the engine in-process on virtual time.
fn fx() -> Ip {
    let env = Ip::with_ports();
    env.engine_config_on_tmux("");
    env.engine_routes_on_tmux();
    let out = env.tmux(&[
        "new-session",
        "-d",
        "-s",
        "x",
        "-P",
        "-F",
        "#{pane_id}",
        "sleep",
        "600",
    ]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let pane = String::from_utf8(out.stdout).unwrap().trim().to_owned();
    env.env.sql("INSERT INTO contexts (id, title, created_at, created_on) VALUES ('ctx', 'Ctx', 't', 'm1');");
    env.engine_plan();
    // A started PR has its open Job before a Lead adds a draft: E4 adds nothing (no `job-added` Event) after the wake W.
    env.add_cpd_job_in("repo", "open-repo", "open", "");
    env.env.sql(&format!("UPDATE sessions SET tmux_pane = '{pane}', state = 'idle', state_at = 't' WHERE id = 'lead1';"));
    env
}

/// The wake of the keys `keys`, as E20 sends it: a `session-message` to `lead1` with no sender.
fn wake(fx: &Ip, keys: &str) {
    let text = format!(
        "Drafts of Plan pay are parked: {keys} (no pending Plan review, no unfinished need). Resolve each now: edit it to a full Spec and Tier and call cpd_plan_review; or cancel it and add it queued when it only carries out a recorded decision; or cancel it (cpd job cancel <k> --reason \"<why>\"). Nothing runs it otherwise."
    );
    let data = serde_json::json!({"from": null, "text": text, "wake": true});
    fx.env.sql(&format!(
        "INSERT INTO events (txn, at, kind, context, plan, session, text, data) VALUES (cpd_txn(), 't', 'session-message', 'ctx', 'pay', 'lead1', 'x', '{}');",
        data.to_string().replace('\'', "''")
    ));
}

/// An Event of `kind` of Plan `pay`, as a command appends it.
fn event(fx: &Ip, kind: &str, job: &str) {
    fx.env.sql(&format!("INSERT INTO events (txn, at, kind, context, plan, job, text, data) VALUES (cpd_txn(), 't', '{kind}', 'ctx', 'pay', '{job}', 'x', '{{}}');"));
}

/// The Lead's state, through `cpd session state` (which writes the `session-state` Event).
fn state(fx: &Ip, to: &str) {
    fx.cpd(&["session", "state", to, "--session", "lead1"]).ok();
}

/// The Lead finishes a turn: `working`, then `idle`.
fn turn(fx: &Ip) {
    state(fx, "working");
    state(fx, "idle");
}

/// Parked full drafts `keys` of PR `repo`, no Plan review.
fn drafts(fx: &Ip, keys: &[&str]) {
    for key in keys {
        fx.add_draft_in("repo", key);
    }
}

fn tick(fx: &Ip) {
    fx.watch_ticks(1).unwrap();
}

/// The open `drafts-parked` items of `cpd inbox list`.
fn items(fx: &Ip) -> Vec<Value> {
    let json = fx.cpd(&["inbox", "list"]).ok();
    json["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i["kind"] == "drafts-parked")
        .cloned()
        .collect()
}

fn labels(fx: &Ip) -> Vec<String> {
    items(fx)
        .iter()
        .map(|i| i["label"].as_str().unwrap().to_owned())
        .collect()
}

/// How many Events there are, but the two every fresh engine run writes (`watch-started`, `watch-stopped`).
fn last_event(fx: &Ip) -> i64 {
    fx.env
        .query("SELECT count(*) FROM events WHERE kind NOT IN ('watch-started', 'watch-stopped')")
}

#[test]
fn opens_after_the_lead_finishes_a_turn() {
    let fx = fx();
    drafts(&fx, &["d1", "d2"]);
    wake(&fx, "d1, d2");
    turn(&fx);
    tick(&fx);

    let [item] = &items(&fx)[..] else {
        panic!("one drafts-parked item: {:?}", items(&fx))
    };
    assert_eq!(item["urgency"], "magenta", "{item}");
    assert_eq!(item["subject"], "drafts-parked:pay", "{item}");
    assert_eq!(item["label"], "pay: 2 parked drafts", "{item}");
    assert_eq!(
        (item["plan"].as_str(), item["session"].as_str()),
        (Some("pay"), Some("lead1")),
        "linked to the Plan and its Lead: {item}"
    );
    assert_eq!(item["context"], "ctx", "{item}");

    // The Lead works again (a chat turn of the User): the item is open only while the Lead is idle.
    state(&fx, "working");
    tick(&fx);
    assert_eq!(
        items(&fx),
        Vec::<Value>::new(),
        "the item closes while the Lead works"
    );

    // Back to idle: it opens again, with the same label. A second pass writes nothing.
    state(&fx, "idle");
    tick(&fx);
    assert_eq!(
        labels(&fx),
        ["pay: 2 parked drafts"],
        "the Lead is idle again, S non-empty, W > T"
    );
    let before = last_event(&fx);
    tick(&fx);
    assert_eq!(
        last_event(&fx),
        before,
        "a pass with no change writes nothing"
    );
    assert_eq!(items(&fx).len(), 1);
}

#[test]
fn closes_while_the_lead_is_not_idle() {
    let fx = fx();
    drafts(&fx, &["d1", "d2"]);
    wake(&fx, "d1, d2");
    turn(&fx);
    tick(&fx);
    assert_eq!(labels(&fx), ["pay: 2 parked drafts"]);

    for to in ["working", "waiting", "blocked"] {
        if to == "working" {
            state(&fx, to);
        } else {
            fx.cpd(&[
                "session",
                "state",
                to,
                "--label",
                "a question",
                "--session",
                "lead1",
            ])
            .ok();
        }
        tick(&fx);
        assert_eq!(
            items(&fx),
            Vec::<Value>::new(),
            "the Lead is {to}: the item closes"
        );
        let closed = fx.cpd(&["inbox", "list", "--closed"]).ok();
        assert!(
            closed["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|i| i["kind"] == "drafts-parked"),
            "closed, not deleted ({to}): {closed}"
        );

        state(&fx, "idle");
        tick(&fx);
        assert_eq!(
            labels(&fx),
            ["pay: 2 parked drafts"],
            "the Lead is idle again after {to}: the item reopens"
        );
    }
}

#[test]
fn no_item_before_a_finished_turn() {
    let fx = fx();
    drafts(&fx, &["d1", "d2"]);
    // A turn finished BEFORE the wake does not count.
    turn(&fx);
    wake(&fx, "d1, d2");
    tick(&fx);
    assert_eq!(
        items(&fx),
        Vec::<Value>::new(),
        "the finished turn is before W"
    );

    // The Lead is `working` after W: the turn is not finished yet.
    state(&fx, "working");
    tick(&fx);
    assert_eq!(items(&fx), Vec::<Value>::new(), "no turn finished since W");

    // It finishes it: the item opens.
    state(&fx, "idle");
    tick(&fx);
    assert_eq!(
        labels(&fx),
        ["pay: 2 parked drafts"],
        "the Lead finished a turn since W"
    );
}

#[test]
fn closes_when_resolved() {
    let fx = fx();
    drafts(&fx, &["d1", "d2"]);
    fx.add_job("q1", &[], "");
    wake(&fx, "d1, d2");
    turn(&fx);
    tick(&fx);
    assert_eq!(labels(&fx), ["pay: 2 parked drafts"]);

    // A need on a queued Job no longer un-parks a complete draft (hs-n677): d2 stays a parked draft.
    fx.env
        .sql("INSERT INTO dependencies (plan, job, needs) VALUES ('pay', 'd2', 'q1');");
    tick(&fx);
    assert_eq!(
        labels(&fx),
        ["pay: 2 parked drafts"],
        "a need does not un-park a complete draft"
    );

    // Cancelling both drafts empties S, whatever W and T say.
    fx.env.sql("UPDATE jobs SET status = 'cancelled', cancel_reason = 'x', cancel_requested_at = 't' WHERE plan = 'pay' AND key IN ('d1', 'd2');");
    tick(&fx);
    assert_eq!(
        items(&fx),
        Vec::<Value>::new(),
        "S is empty: the item closes"
    );
    let closed = fx.cpd(&["inbox", "list", "--closed"]).ok();
    assert!(
        closed["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["kind"] == "drafts-parked"),
        "closed, not deleted: {closed}"
    );
}

#[test]
fn closes_when_a_state_change_passes_the_wake() {
    let fx = fx();
    drafts(&fx, &["d1", "d2"]);
    wake(&fx, "d1, d2");
    turn(&fx);
    tick(&fx);
    assert_eq!(items(&fx).len(), 1);

    // The Lead edits a draft and leaves both parked: T moves past W.
    event(&fx, "job-edited", "d1");
    tick(&fx);
    assert_eq!(
        items(&fx),
        Vec::<Value>::new(),
        "T moved past W: the item closes (E20 wakes the idle Lead again)"
    );
    tick(&fx);
    assert_eq!(
        items(&fx),
        Vec::<Value>::new(),
        "no item until the Lead finishes another turn after the new wake"
    );

    // E20's new wake is the newest `session-message`; the turn after it opens the item again.
    let wakes: i64 = fx
        .env
        .query("SELECT count(*) FROM events WHERE kind = 'session-message' AND session = 'lead1'");
    assert_eq!(wakes, 2, "E20 sent a new wake after the edit");
    turn(&fx);
    tick(&fx);
    assert_eq!(
        labels(&fx),
        ["pay: 2 parked drafts"],
        "a turn finished after the new wake"
    );
}

#[test]
fn label_counts_the_drafts() {
    let fx = fx();
    drafts(&fx, &["d1"]);
    wake(&fx, "d1");
    turn(&fx);
    tick(&fx);
    assert_eq!(labels(&fx), ["pay: 1 parked draft"]);

    // A second draft added without an Event of T's kinds (as a test of the count only).
    drafts(&fx, &["d2"]);
    tick(&fx);
    assert_eq!(labels(&fx), ["pay: 2 parked drafts"]);
    assert_eq!(items(&fx).len(), 1, "one item per Plan");
}

#[test]
fn an_archived_plan_has_none() {
    let fx = fx();
    drafts(&fx, &["d1"]);
    wake(&fx, "d1");
    turn(&fx);
    fx.env
        .sql("UPDATE plans SET archived_at = 't' WHERE id = 'pay';");
    tick(&fx);
    assert_eq!(items(&fx), Vec::<Value>::new());
}
