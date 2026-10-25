//! Engine step E8, the `curate-retire` and `curate-docs` Inbox items (contract §2.2 "Inbox item", §3.7; hs-d549,
//! hs-d551): one magenta item per Context and kind, its count following the pending rows, closed when none is.

use crate::common;

use std::fs;
use std::process::{Child, Stdio};
use std::time::Duration;

use common::{CPD, TestEnv, wait_until};
use cpd_core::context::curation::{Curation, Proposal, Reason, propose};
use cpd_core::context::{By, Draft, Record};
use cpd_core::error::CpdError;
use cpd_core::ids::{ContextId, RecordId};
use cpd_core::home::CpdHome;
use cpd_core::store::{Store, WriteTxn};
use serde_json::Value;

fn exec(tx: &WriteTxn<'_>, sql: &str) -> Result<(), CpdError> {
    tx.execute_batch(sql)?;
    Ok(())
}

/// A running `cpd watch`, SIGTERMed on drop (killed if it does not stop).
struct Engine(Child);

impl Drop for Engine {
    fn drop(&mut self) {
        let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(self.0.id() as i32), nix::sys::signal::Signal::SIGTERM);
        common::poll(Duration::from_secs(10), || (!matches!(self.0.try_wait(), Ok(None))).then_some(()));
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Fx {
    env: TestEnv,
}

/// A prepared store with Contexts `building` and `control`, and a tick of `common::TICK_SECONDS`.
fn fx() -> Fx {
    let env = TestEnv::new();
    fs::write(
        env.cpd_home().join("config.toml"),
        format!(
            "version = 1\nmachine = \"m1\"\ncontext_curation_min_records = 1\n[watch]\ntick_seconds = {}\n[tmux]\nsocket = \"{}\"\n",
            common::TICK_SECONDS,
            env.tmux_socket()
        ),
    )
    .unwrap();
    env.fake("cpd", &format!("exec '{CPD}' \"$@\""));
    env.fake_gh(r#"echo '{"data":{"search":{"issueCount":0,"nodes":[]},"reviewRequests":{"issueCount":0,"nodes":[]},"rateLimit":{"cost":1,"remaining":4987,"resetAt":"2026-10-20T11:00:00Z"}}}'"#);
    let fx = Fx { env };
    fx.store()
        .write("fixture", |tx| {
            exec(
                tx,
                "INSERT INTO contexts (id, title, created_at, created_on) VALUES ('building', 'B', 't', 'm1');
                 INSERT INTO contexts (id, title, created_at, created_on) VALUES ('control', 'C', 't', 'm1');",
            )
        })
        .unwrap();
    fx
}

impl Fx {
    fn store(&self) -> Store {
        Store::open(&CpdHome::resolve(Some(self.env.cpd_home().into()), None).unwrap(), 5000).unwrap()
    }

    fn start(&self) -> Engine {
        let child = self.env.command(CPD).arg("watch").envs([common::PR_POLL_EVERY_TICK]).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        let engine = Engine(child);
        wait_until("watch-started", || self.env.query::<i64>("SELECT count(*) FROM events WHERE kind = 'watch-started'") == 1);
        engine
    }

    /// Notes `<prefix>1 .. <prefix>n` (ids `hs-n<k>`) in `context`.
    fn records(&self, context: &str, ids: &[&str]) {
        let context = ContextId::parse(context).unwrap();
        self.store()
            .write("fixture", |tx| {
                for id in ids {
                    let record = Record { id: RecordId::parse(id).unwrap(), at: "2026-10-20T10:00:00.000Z".into(), by: By { session: None, plan: None }, text: format!("text of {id}"), draft: Draft::Note };
                    record.insert(tx, &context).unwrap();
                }
                Ok(())
            })
            .unwrap();
    }

    /// Curates `ids` of `context`; proposes the retirement of `retire` (all duplicates of the last of `ids`) and flags `docs`.
    fn propose(&self, context: &str, ids: &[&str], retire: &[&str], docs: &[&str]) {
        let curations: Vec<Curation> = ids
            .iter()
            .map(|id| Curation {
                record: RecordId::parse(id).unwrap(),
                topic: "engine".into(),
                short: format!("short of {id}"),
                doc_paths: docs.contains(id).then(|| vec!["docs/a.md".to_owned()]),
            })
            .collect();
        let proposals: Vec<Proposal> = retire.iter().map(|id| Proposal { record: RecordId::parse(id).unwrap(), reason: Reason::Duplicate, target: ids.last().unwrap().to_string() }).collect();
        propose(&mut self.store(), &ContextId::parse(context).unwrap(), &curations, &proposals).unwrap();
    }

    /// The open items of `kind` of `cpd inbox list`, as JSON.
    fn items(&self, kind: &str) -> Vec<Value> {
        let json = self.env.cpd(&["inbox", "list"]).ok();
        json["items"].as_array().unwrap().iter().filter(|i| i["kind"] == kind).cloned().collect()
    }

    fn assert_item(&self, kind: &str, context: &str, label: &str) {
        wait_until("curate item", || self.items(kind).iter().any(|i| i["context"] == context && i["label"] == label));
        let item = self.items(kind).into_iter().find(|i| i["context"] == context).unwrap();
        assert_eq!(item["urgency"], "blue", "{item}");
        assert_eq!(item["subject"], format!("{kind}:{context}"), "{item}");
        for null in ["plan", "job", "session", "record"] {
            assert!(item[null].is_null(), "{null} must be null: {item}");
        }
    }
}

#[test]
fn e8_opens_one_curate_retire_item_per_context_with_its_count() {
    let fx = fx();
    fx.records("building", &["hs-n1", "hs-n2", "hs-n3", "hs-n9"]);
    fx.records("control", &["hs-n4", "hs-n5"]);
    fx.propose("building", &["hs-n1", "hs-n2", "hs-n3", "hs-n9"], &["hs-n1", "hs-n2"], &[]);
    fx.propose("control", &["hs-n4", "hs-n5"], &["hs-n4"], &[]);
    let _engine = fx.start();
    fx.assert_item("curate-retire", "building", "building: 2 to retire");
    fx.assert_item("curate-retire", "control", "control: 1 to retire");
    assert_eq!(fx.items("curate-retire").len(), 2, "{:?}", fx.items("curate-retire"));
    assert!(fx.items("curate-docs").is_empty());

    // the count follows the pending rows, and the item closes when none is
    fx.env.cpd(&["context", "retire", "--context", "building", "--approve", "hs-n1"]).ok();
    fx.assert_item("curate-retire", "building", "building: 1 to retire");
    fx.env.cpd(&["context", "retire", "--context", "building", "--reject", "hs-n2"]).ok();
    wait_until("building closed", || fx.items("curate-retire").iter().all(|i| i["context"] != "building"));
    assert_eq!(fx.items("curate-retire").len(), 1);
}

#[test]
fn e8_opens_a_curate_docs_item_and_closes_it_when_none_pending() {
    let fx = fx();
    fx.records("building", &["hs-n1", "hs-n2", "hs-n3"]);
    fx.propose("building", &["hs-n1", "hs-n2", "hs-n3"], &[], &["hs-n1", "hs-n3"]);
    let _engine = fx.start();
    fx.assert_item("curate-docs", "building", "building: 2 doc candidates");
    assert!(fx.items("curate-retire").is_empty(), "{:?}", fx.items("curate-retire"));

    fx.env.cpd(&["context", "docs", "--context", "building", "--dismiss", "hs-n1,hs-n3"]).ok();
    wait_until("curate-docs closed", || fx.items("curate-docs").is_empty());
}

#[test]
fn e8_opens_the_curate_items_blue() {
    let fx = fx();
    fx.records("building", &["hs-n1", "hs-n2", "hs-n3"]);
    fx.propose("building", &["hs-n1", "hs-n2", "hs-n3"], &["hs-n1"], &["hs-n3"]);
    let _engine = fx.start();
    fx.assert_item("curate-retire", "building", "building: 1 to retire");
    fx.assert_item("curate-docs", "building", "building: 1 doc candidates");
    for kind in ["curate-retire", "curate-docs"] {
        assert!(fx.items(kind).iter().all(|i| i["urgency"] == "blue"), "{:?}", fx.items(kind));
    }
}

impl Fx {
    /// Rewrites `context_curation_min_records` in config.toml, every other line kept.
    fn set_floor(&self, floor: u32) {
        let path = self.env.cpd_home().join("config.toml");
        let text = fs::read_to_string(&path).unwrap();
        let lines: Vec<String> = text.lines().map(|l| if l.starts_with("context_curation_min_records") { format!("context_curation_min_records = {floor}") } else { l.to_owned() }).collect();
        fs::write(&path, lines.join("\n") + "\n").unwrap();
    }

    fn pending(&self, context: &str) -> (i64, i64) {
        (
            self.env.query(&format!("SELECT count(*) FROM record_retirements WHERE context = '{context}' AND status = 'pending'")),
            self.env.query(&format!("SELECT count(*) FROM record_curations WHERE context = '{context}' AND doc_candidate = 'pending'")),
        )
    }
}

/// hs-d592 R6: an archived Context has no curate item (derived from its state: unarchiving reopens them), its rows untouched.
#[test]
fn e8_opens_no_curate_item_for_an_archived_context_and_reopens_it_unarchived() {
    let fx = fx();
    fx.records("building", &["hs-n1", "hs-n2", "hs-n3", "hs-n4"]);
    fx.propose("building", &["hs-n1", "hs-n2", "hs-n3", "hs-n4"], &["hs-n1", "hs-n2"], &["hs-n3"]);
    let _engine = fx.start();
    fx.assert_item("curate-retire", "building", "building: 2 to retire");
    fx.assert_item("curate-docs", "building", "building: 1 doc candidates");
    let before = fx.pending("building");

    fx.env.sql("UPDATE contexts SET archived_at = '2026-10-20T10:00:00.000Z' WHERE id = 'building'");
    wait_until("both items closed", || fx.items("curate-retire").is_empty() && fx.items("curate-docs").is_empty());
    assert_eq!(fx.pending("building"), before, "the rows stay pending");
    assert_eq!(before, (2, 1));

    fx.env.sql("UPDATE contexts SET archived_at = NULL WHERE id = 'building'");
    fx.assert_item("curate-retire", "building", "building: 2 to retire");
    fx.assert_item("curate-docs", "building", "building: 1 doc candidates");
    assert_eq!(fx.pending("building"), before);
}

/// hs-d592 R2, R5, R6: a Context under `context_curation_min_records` has no curate item, the floor read at every tick.
#[test]
fn e8_opens_no_curate_item_for_a_context_under_the_floor() {
    let fx = fx();
    fx.records("building", &["hs-n1", "hs-n2", "hs-n3"]);
    fx.propose("building", &["hs-n1", "hs-n2", "hs-n3"], &["hs-n1", "hs-n2"], &["hs-n3"]);
    fx.records("control", &["hs-n4", "hs-n5", "hs-n6", "hs-n7", "hs-n8", "hs-n9"]);
    fx.propose("control", &["hs-n4", "hs-n5", "hs-n6", "hs-n7", "hs-n8", "hs-n9"], &["hs-n4"], &["hs-n9"]);
    let _engine = fx.start();
    fx.assert_item("curate-retire", "building", "building: 2 to retire");
    fx.assert_item("curate-docs", "building", "building: 1 doc candidates");
    fx.assert_item("curate-retire", "control", "control: 1 to retire");
    fx.assert_item("curate-docs", "control", "control: 1 doc candidates");
    let before = (fx.pending("building"), fx.pending("control"));

    fx.set_floor(5);
    wait_until("building's items closed", || fx.items("curate-retire").iter().chain(fx.items("curate-docs").iter()).all(|i| i["context"] != "building"));
    assert_eq!((fx.pending("building"), fx.pending("control")), before, "the rows stay pending");
    assert_eq!(fx.items("curate-retire").len(), 1, "control keeps its items");
    assert_eq!(fx.items("curate-docs").len(), 1, "control keeps its items");

    fx.set_floor(3);
    fx.assert_item("curate-retire", "building", "building: 2 to retire");
    fx.assert_item("curate-docs", "building", "building: 1 doc candidates");
    fx.assert_item("curate-retire", "control", "control: 1 to retire");
    assert_eq!((fx.pending("building"), fx.pending("control")), before);
}
