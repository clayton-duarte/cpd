//! Tests of write-time curation: the Bifrost judge adapter and its fake. A `FakeRunner` on a `ManualClock`: no process.

use std::io::ErrorKind;
use std::path::Path;
use std::sync::Arc;

use serde_json::{Value, json};

use super::curate::{Judged, Verdict};
use super::curate_judge::{BifrostJudge, JudgePort, JudgeRequest, RecordView};
use crate::clock::ManualClock;
use crate::config::Config;
use crate::error::{CpdError, ErrorCode};
use crate::fakes::judge::FakeJudge;
use crate::home::CpdHome;
use crate::host::runner::{FakeRunner, Reply};
use crate::ids::{RecordId, RecordKind};

const KEY: &str = "sk-bf-secret-key";

struct Fx {
    _dir: tempfile::TempDir,
    home: CpdHome,
    config: Config,
    runner: FakeRunner,
}

fn fixture(reply: Reply) -> Fx {
    let dir = tempfile::tempdir().unwrap();
    let home = CpdHome::resolve(Some(dir.path().as_os_str().to_owned()), None).unwrap();
    let adapter = dir.path().join("adapter");
    std::fs::create_dir_all(&adapter).unwrap();
    std::fs::write(adapter.join("package.json"), "{}").unwrap();
    let (config, _) = Config::parse(
        &format!(
            "version = 1\nmachine = \"m1\"\npi_adapter = \"{}\"\n",
            adapter.display()
        ),
        Path::new("config.toml"),
    )
    .unwrap();
    let runner = FakeRunner::new(ManualClock::new()).on("node", |_| true, reply);
    Fx {
        _dir: dir,
        home,
        config,
        runner,
    }
}

impl Fx {
    fn judge(&self) -> BifrostJudge {
        BifrostJudge::new(&self.home, &self.config, Arc::new(self.runner.clone()))
    }
}

fn id(text: &str) -> RecordId {
    RecordId::parse(text).unwrap()
}

fn pair_request<'a>(new: &'a RecordId, old: &'a RecordId) -> JudgeRequest<'a> {
    JudgeRequest {
        new: RecordView {
            id: new,
            kind: RecordKind::Decision,
            at: "2026-11-03T12:00:00Z",
            topic: None,
            text: "Use Postgres for the store.",
        },
        neighbour: Some(RecordView {
            id: old,
            kind: RecordKind::Note,
            at: "2026-10-01T09:00:00Z",
            topic: Some("storage"),
            text: "The store is SQLite.",
        }),
    }
}

fn good() -> Value {
    json!({"ok": true, "verdict": "supersedes", "quote": "Use Postgres", "reason": "newer choice",
           "topic": "storage", "short": "Store moves to Postgres"})
}

fn answer(value: &Value) -> Reply {
    Reply::ok(&format!("{value}\n"))
}

fn judged() -> Judged {
    Judged {
        verdict: Verdict::Supersedes,
        quote: "Use Postgres".to_owned(),
        reason: "newer choice".to_owned(),
        topic: "storage".to_owned(),
        short: "Store moves to Postgres".to_owned(),
    }
}

fn failed(fx: &Fx) -> CpdError {
    let (new, old) = (id("m1-d2"), id("m1-n1"));
    fx.judge().judge(&pair_request(&new, &old)).unwrap_err()
}

#[test]
fn judge_request_goes_on_stdin_and_no_key_in_argv_envs_or_errors() {
    let fx = fixture(answer(&good()));
    let (new, old) = (id("m1-d2"), id("m1-n1"));
    fx.judge().judge(&pair_request(&new, &old)).unwrap();
    let calls = fx.runner.calls();
    assert_eq!(calls.len(), 1);
    let call = &calls[0];
    assert!(call.program.ends_with("node"), "{}", call.program);
    assert!(
        call.args.iter().any(|a| a.ends_with("lib/judge.ts")),
        "{:?}",
        call.args
    );
    assert_eq!(call.deadline, std::time::Duration::from_secs(120));
    let stdin: Value = serde_json::from_slice(call.stdin.as_ref().expect("stdin")).unwrap();
    assert_eq!(stdin["model"], "vertex/gemini-3.8-flash");
    assert_eq!(stdin["new"]["id"], "m1-d2");
    assert_eq!(stdin["new"]["kind"], "decision");
    assert_eq!(stdin["new"]["text"], "Use Postgres for the store.");
    assert_eq!(stdin["neighbour"]["id"], "m1-n1");
    assert_eq!(stdin["neighbour"]["topic"], "storage");
    let root = fx.home.root().display().to_string();
    assert!(call.envs.contains(&("CPD_HOME".to_owned(), Some(root))));
    for var in ["CPD_PLAN", "CPD_SESSION", "CPD_ATTEMPT"] {
        assert!(call.envs.contains(&(var.to_owned(), None)), "{var}");
    }
    let shown = format!("{:?} {:?}", call.args, call.envs);
    assert!(!shown.contains(KEY));

    // An error carries no key either, even when the helper's stderr echoed one.
    let fx = fixture(Reply::exit(1, "", &format!("boom {KEY}")));
    let error = failed(&fx);
    assert!(!format!("{error:?} {}", error.to_json()).contains(KEY));
}

#[test]
fn judge_valid_reply_round_trips_every_field() {
    let fx = fixture(answer(&good()));
    let (new, old) = (id("m1-d2"), id("m1-n1"));
    assert_eq!(fx.judge().judge(&pair_request(&new, &old)), Ok(judged()));
}

#[test]
fn judge_unknown_verdict_is_judge_bad_reply() {
    let mut reply = good();
    reply["verdict"] = json!("maybe");
    let fx = fixture(answer(&reply));
    assert_eq!(failed(&fx).code(), ErrorCode::JudgeBadReply);
}

#[test]
fn judge_non_json_reply_is_judge_bad_reply() {
    let fx = fixture(Reply::ok("not json at all\n"));
    assert_eq!(failed(&fx).code(), ErrorCode::JudgeBadReply);
}

#[test]
fn judge_missing_field_is_judge_bad_reply() {
    for field in ["verdict", "quote", "reason", "topic", "short"] {
        let mut reply = good();
        reply.as_object_mut().unwrap().remove(field);
        let fx = fixture(answer(&reply));
        assert_eq!(failed(&fx).code(), ErrorCode::JudgeBadReply, "{field}");
    }
}

#[test]
fn judge_failed_exit_io_and_timeout_are_judge_unreachable() {
    let exit = fixture(Reply::exit(1, "", "node: crashed"));
    assert_eq!(failed(&exit).code(), ErrorCode::JudgeUnreachable);
    let io = fixture(Reply::io(ErrorKind::NotFound));
    assert_eq!(failed(&io).code(), ErrorCode::JudgeUnreachable);
    let slow = fixture(Reply::ok("{}").takes(std::time::Duration::from_secs(121)));
    assert_eq!(failed(&slow).code(), ErrorCode::JudgeUnreachable);
}

#[test]
fn judge_error_envelope_with_a_status_is_judge_refused() {
    let envelope = json!({"ok": false, "error": {"code": "judge_refused", "message": "gateway said no",
                                                  "details": {"status": 429}}});
    let fx = fixture(answer(&envelope));
    let error = failed(&fx);
    assert_eq!(error.code(), ErrorCode::JudgeRefused);
    assert_eq!(error.details()["status"], 429);
}

#[test]
fn judge_label_request_has_no_neighbour_and_reads_topic_and_short() {
    let reply = json!({"ok": true, "verdict": "independent", "quote": "", "reason": "",
                       "topic": "storage", "short": "Which store?"});
    let fx = fixture(answer(&reply));
    let new = id("m1-n3");
    let request = JudgeRequest {
        new: RecordView {
            id: &new,
            kind: RecordKind::Note,
            at: "2026-11-03T12:00:00Z",
            topic: None,
            text: "Which store do we use?",
        },
        neighbour: None,
    };
    let got = fx.judge().judge(&request).unwrap();
    assert_eq!(got.verdict, Verdict::Independent);
    assert_eq!(
        (got.topic.as_str(), got.short.as_str()),
        ("storage", "Which store?")
    );
    assert_eq!(got.quote, "");
    let call = &fx.runner.calls()[0];
    let stdin: Value = serde_json::from_slice(call.stdin.as_ref().expect("stdin")).unwrap();
    assert_eq!(stdin["neighbour"], Value::Null);
    assert_eq!(stdin["new"]["kind"], "note");
}

#[test]
fn fake_judge_replays_scripted_replies_in_order_and_records_requests() {
    let refused = CpdError::new(ErrorCode::JudgeRefused, "no").with("status", 500);
    let fake = FakeJudge::new(vec![Ok(judged()), Err(refused.clone())]);
    let (new, old) = (id("m1-d2"), id("m1-n1"));
    let request = pair_request(&new, &old);
    assert_eq!(fake.judge(&request), Ok(judged()));
    assert_eq!(fake.judge(&request), Err(refused));
    let calls = fake.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].new.id, new);
    assert_eq!(
        calls[0].neighbour.as_ref().unwrap().topic.as_deref(),
        Some("storage")
    );
    assert_eq!(FakeJudge::answering(judged()).judge(&request), Ok(judged()));

    // The fake records what the real adapter sends: the same request, whichever judge is behind the port.
    let fx = fixture(answer(&good()));
    fx.judge().judge(&request).unwrap();
    let sent: Value =
        serde_json::from_slice(fx.runner.calls()[0].stdin.as_ref().expect("stdin")).unwrap();
    assert_eq!(sent["new"]["id"], calls[0].new.id.to_string());
    assert_eq!(
        sent["neighbour"]["text"],
        calls[0].neighbour.as_ref().unwrap().text
    );
}
