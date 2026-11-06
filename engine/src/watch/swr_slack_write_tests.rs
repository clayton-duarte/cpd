//! E24 draining the `slack` outbox (docs/architecture/swr-slack.md §4.3): a refused reaction parks its mutation `failed`
//! and raises one `error` Event `slack_refused`, scoped to the Plan for a merge reaction, once. The helpers are scripted
//! `node` routes on the `FakeRunner`; the clock is the `ManualClock`; no Slack, no process, no sleep. One test per process.

use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use serde_json::json;
use time::OffsetDateTime;

use super::swr;
use super::testkit::Fx;
use cpd_core::host::runner::{Call, Handler, Reply};
use cpd_core::ports::Fakes;
use cpd_core::swr::outbox::{self, Coalesce, Op};

const CHANNEL: &str = "C0POST001";
const TS: &str = "1760951522.011300";
const REFUSED: &str = r#"{"ok":false,"error":{"code":"slack_refused","message":"Slack refused the reaction.","details":{"reason":"invalid_name"}}}"#;
const EMPTY_CHANNEL: &str = r#"{"ok":true,"channel":"C0TEAM001","read":0,"posts":[]}"#;

/// The reaction helper refuses (exit 3); the channel helper reads an empty channel. Reaction runs are recorded.
struct Helper(Arc<Mutex<Vec<Call>>>);

impl Handler for Helper {
    fn handle(&self, call: &Call) -> Reply {
        if call.args.iter().any(|a| a.ends_with("slack-react.ts")) {
            self.0.lock().unwrap().push(call.clone());
            return Reply::exit(3, &format!("{REFUSED}\n"), "");
        }
        Reply::exit(0, &format!("{EMPTY_CHANNEL}\n"), "")
    }
}

/// An engine with Plan `pay` and a pending merge reaction of its Workflow `repo` on `CHANNEL`/`TS`.
fn fx() -> (Fx, Fakes, Arc<Mutex<Vec<Call>>>) {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("pi"), "").unwrap();
    std::fs::set_permissions(bin.join("pi"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::create_dir_all(root.join("install")).unwrap();
    std::fs::write(root.join("install/current-version"), "1.2.0\n").unwrap();
    for package in ["pi-coding-agent", "pi-ai"] {
        let dist = root
            .join("install/releases/1.2.0/node_modules/@earendil-works")
            .join(package)
            .join("dist");
        std::fs::create_dir_all(&dist).unwrap();
        std::fs::write(dist.join("index.js"), "").unwrap();
    }
    let adapter = root.join("adapter");
    std::fs::create_dir_all(&adapter).unwrap();
    std::fs::write(adapter.join("package.json"), "{}").unwrap();
    let mut fx = Fx::with_plan(&format!(
        "pi_adapter = \"{}\"\n[inbox]\nteam_channel = \"C0TEAM001\"\nslack_user_id = \"U0USER001\"\n",
        adapter.display()
    ));
    // The temp dir lives as long as the process: one test per process.
    std::mem::forget(tmp);
    // SAFETY: one test per process (nextest); the variables point at this test's own temp dir.
    unsafe {
        std::env::set_var("CPD_HOME", fx.engine.home.root());
        let old = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![bin];
        paths.extend(std::env::split_paths(&old));
        std::env::set_var("PATH", std::env::join_paths(paths).unwrap());
    }
    let fakes = Fakes::new();
    fx.engine.ports = fakes.ports.clone();
    let reacts = Arc::new(Mutex::new(Vec::new()));
    let _ = (*fakes.runner)
        .clone()
        .route("node", Arc::new(Helper(reacts.clone())));
    let vars =
        json!({"channel": CHANNEL, "emoji": "merge", "plan": "pay", "ts": TS, "workflow": "repo"});
    outbox::enqueue(
        &mut fx.engine.store,
        OffsetDateTime::from_unix_timestamp(1_767_225_600).unwrap(),
        Op {
            system: "slack".into(),
            target_key: format!("slack:channel:{CHANNEL}"),
            op_kind: "reaction".into(),
            vars: vars.clone(),
            patch: None,
            context: None,
        },
        Coalesce::Dedupe(vars.to_string()),
    )
    .unwrap();
    (fx, fakes, reacts)
}

#[test]
fn slack_refused_reaction_parks_with_one_error_event() {
    let (mut fx, _fakes, reacts) = fx();

    swr::run(&mut fx.engine);

    let parked: (String, Option<String>) = fx
        .engine
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT status, error FROM swr_mutations WHERE op_kind = 'reaction'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    assert_eq!(
        parked,
        ("failed".to_owned(), Some("invalid_name".to_owned())),
        "§4.3: the row is failed with Slack's error name"
    );
    let events = fx.events("error");
    let refused: Vec<_> = events
        .iter()
        .filter(|(_, _, _, data)| data["code"] == "slack_refused")
        .collect();
    assert_eq!(refused.len(), 1, "one slack_refused Event: {events:?}");
    assert_eq!(
        refused[0].0.as_deref(),
        Some("pay"),
        "a merge reaction's Event is scoped to its Plan"
    );

    // A parked row is never tried again and raises no second Event.
    swr::run(&mut fx.engine);
    assert_eq!(reacts.lock().unwrap().len(), 1, "the helper ran once");
    assert_eq!(
        fx.events("error")
            .iter()
            .filter(|(_, _, _, data)| data["code"] == "slack_refused")
            .count(),
        1
    );
}

/// The old E17 `incident_mode_holds_the_step`, for E24: incident mode holds the drain and the Slack fetch, so the queued
/// reaction stays queued and no Slack call is made; the next tick after the incident ends drains it.
#[test]
fn incident_mode_holds_the_drain_until_it_ends() {
    let (mut fx, _fakes, reacts) = fx();
    fx.engine
        .store
        .write("incident on", |tx| {
            Ok(tx.execute_batch(
                "INSERT INTO incident (id, started_at, started_by, reason, started_txn) VALUES (1, 't', 'mgr', 'why', cpd_txn())",
            )?)
        })
        .unwrap();

    swr::run(&mut fx.engine);

    assert!(
        reacts.lock().unwrap().is_empty(),
        "no reaction is sent under incident mode"
    );
    let status: String = fx
        .engine
        .store
        .read(|c| {
            Ok(c.query_row(
                "SELECT status FROM swr_mutations WHERE op_kind = 'reaction'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(status, "pending", "the reaction stays queued");
    let queries: i64 = fx
        .engine
        .store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM swr_queries", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(queries, 0, "the Slack fetch is held too");
    assert!(fx.events("error").is_empty(), "a hold raises no error");

    fx.engine
        .store
        .write("incident end", |tx| {
            Ok(tx.execute_batch("DELETE FROM incident")?)
        })
        .unwrap();
    swr::run(&mut fx.engine);

    assert_eq!(reacts.lock().unwrap().len(), 1, "the next tick drains it");
}

/// The old E17 `nothing_runs_and_nothing_is_reported_when_nothing_is_queued`, for E24: with no mutation queued, the drain
/// sends no reaction and raises no error.
#[test]
fn nothing_is_sent_or_reported_when_nothing_is_queued() {
    let (mut fx, _fakes, reacts) = fx();
    fx.engine
        .store
        .write("empty outbox", |tx| {
            Ok(tx.execute_batch("DELETE FROM swr_mutations")?)
        })
        .unwrap();

    swr::run(&mut fx.engine);

    assert!(reacts.lock().unwrap().is_empty(), "no reaction is sent");
    assert!(
        fx.events("error").is_empty(),
        "nothing is reported for an empty outbox"
    );
}
